// SPDX-License-Identifier: MIT
// gfx1030 wave32 implementation of the pinned renderer's gfx12 fragment ABI.
// Uses ordered FP32 FMA, DPP8 broadcasts and lane shuffles; no WMMA/FP8 opcodes.
// Include before the upstream sources, after HIP's intrinsic declarations.
#pragma once
#include <hip/hip_runtime.h>
#include <hip/hip_fp16.h>
#include <hip/hip_fp8.h>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <type_traits>

#if defined(__HIP_DEVICE_COMPILE__) && !defined(__gfx1030__)
#error "Flightdeck's RDNA2 backend must be compiled for gfx1030"
#endif

namespace flightdeck_rdna2 {
__host__ __device__ inline uint32_t bits(float x) {
    uint32_t b;
    __builtin_memcpy(&b, &x, sizeof(b));
    return b;
}
__host__ __device__ inline float value(uint32_t b) {
    float x;
    __builtin_memcpy(&x, &b, sizeof(x));
    return x;
}
// OCP E4M3FN, including signed zero, subnormals, saturation and signed NaNs.
__host__ __device__ inline uint8_t encode(float x) {
    const uint32_t b = bits(x), a = b & 0x7fffffffu;
    const uint8_t sign = uint8_t((b >> 24) & 0x80u);
    if (a > 0x7f800000u) return sign | 0x7f;
    if (a >= 0x43e00000u) return sign | 0x7e;
    if (a < 0x3c800000u)
        return sign | uint8_t(nearbyintf(fabsf(x) * 512.f));
    const uint32_t rounded = (a + 0x7ffffu + ((a >> 20) & 1u)) & 0xfff00000u;
    const uint32_t code = ((rounded >> 23) - 120u) * 8u + ((rounded >> 20) & 7u);
    return sign | uint8_t(code > 126u ? 126u : code);
}
__host__ __device__ inline float decode(uint8_t b) {
    const uint32_t e = (b >> 3) & 15u, m = b & 7u;
    if (e == 15u && m == 7u) return value(0x7fc00000u | (uint32_t(b & 0x80u) << 24));
    const float x = e ? value(((e + 120u) << 23) | (m << 20)) : float(m) / 512.f;
    return b & 0x80u ? -x : x;
}
// Exact E4M3FN -> FP16: normal exponents differ by eight; preserve subnormals and signed NaNs.
__host__ __device__ inline _Float16 decode_half(uint8_t b) {
    const unsigned a = b & 127u;
    uint16_t magnitude = uint16_t((a << 7u) + 0x2000u);
    if (a < 8u) {
        const _Float16 sub = _Float16(float(a) * (1.f / 512.f));
        __builtin_memcpy(&magnitude, &sub, sizeof(magnitude));
    }
    if (a == 127u) magnitude = 0x7e00u;
    const uint16_t raw = magnitude | (uint16_t(b & 128u) << 8u);
    _Float16 value;
    __builtin_memcpy(&value, &raw, sizeof(value));
    return value;
}
using F8 = float __attribute__((ext_vector_type(8)));
using H8 = _Float16 __attribute__((ext_vector_type(8)));
using H2 = _Float16 __attribute__((ext_vector_type(2)));
using I2 = int __attribute__((ext_vector_type(2)));
using U4 = uint32_t __attribute__((ext_vector_type(4)));

// Match the existing fused C32 projection: a single FP16 rounding of
// accumulator + residual * scale. An intermediate FP32 rounding changes rare
// halfway cases with real weights. This is separate from ordered FP32 MMA.
__device__ inline float residual_half(float residual, float scale, float acc) {
    float result;
    asm("v_fma_mixlo_f16 %0, %1, %2, %3" : "=v"(result)
        : "v"(residual), "v"(scale), "v"(acc));
    return __half2float(__ushort_as_half(uint16_t(bits(result))));
}

template<unsigned E>
__device__ inline float mma_row(uint32_t rows, H2 bv, float c) {
    // HIP defaults allow LLVM to merge consecutive half-input FMAs into dot2,
    // which changes rounding. Preserve this order in production, not just tests.
    #pragma clang fp contract(off)
    const uint32_t abits = __builtin_amdgcn_mov_dpp8(rows, E * 0x249249u);
    H2 av;
    __builtin_memcpy(&av, &abits, sizeof(av));
    c = __builtin_fmaf(float(av[0]), float(bv[0]), c);
    return __builtin_fmaf(float(av[1]), float(bv[1]), c);
}

// A lane l owns A[row=l%16][k=l/16*8+e] and B[k=l/16*8+e][col=l%16].
// C owns [row=l/16*8+e][col=l%16]. All 32 lanes must participate.
// Keep K ordered: the graph has explicit half/FP8 rounding at block boundaries.
__device__ inline F8 mma_half(H8 a, H8 b, F8 c) {
    #pragma clang fp contract(off)
    U4 ap, bp;
    __builtin_memcpy(&ap, &a, sizeof(ap));
    __builtin_memcpy(&bp, &b, sizeof(bp));
    const unsigned lane = threadIdx.x & 31u;
    #pragma unroll
    for (unsigned pair = 0; pair < 8; ++pair) {
        // One cross-lane gather prepares two repeated groups of eight rows.
        // DPP8 then broadcasts within each group without using the LDS crossbar.
        const uint32_t rows = __shfl(ap[pair & 3u], (lane & 7u) + (lane / 16u) * 8u + (pair / 4u) * 16u, 32);
        const uint32_t bbits = __shfl(bp[pair & 3u], (lane & 15u) + (pair / 4u) * 16u, 32);
        H2 bv;
        __builtin_memcpy(&bv, &bbits, sizeof(bv));
        c[0] = mma_row<0>(rows, bv, c[0]); c[1] = mma_row<1>(rows, bv, c[1]);
        c[2] = mma_row<2>(rows, bv, c[2]); c[3] = mma_row<3>(rows, bv, c[3]);
        c[4] = mma_row<4>(rows, bv, c[4]); c[5] = mma_row<5>(rows, bv, c[5]);
        c[6] = mma_row<6>(rows, bv, c[6]); c[7] = mma_row<7>(rows, bv, c[7]);
    }
    return c;
}
__device__ inline F8 mma_fp8(I2 a, I2 b, F8 c) {
    H8 af{}, bf{};
    #pragma unroll
    for (unsigned i = 0; i < 8; ++i) {
        af[i] = decode_half(uint8_t(uint32_t(a[i / 4]) >> ((i % 4) * 8)));
        bf[i] = decode_half(uint8_t(uint32_t(b[i / 4]) >> ((i % 4) * 8)));
    }
    return mma_half(af, bf, c);
}
__device__ inline int pack(float a, float b, int old, bool high) {
    const uint32_t pair = uint32_t(encode(a)) | (uint32_t(encode(b)) << 8);
    return int(high ? (uint32_t(old) & 0xffffu) | (pair << 16)
                    : (uint32_t(old) & 0xffff0000u) | pair);
}
__device__ inline float unpack(int packed, int byte) {
    return decode(uint8_t(uint32_t(packed) >> (unsigned(byte) * 8)));
}

// The small fragment surface used by dlss5_linalg.hpp. No rocWMMA dependency.
struct matrix_a {};
struct matrix_b {};
struct accumulator {};
struct row_major {};
struct col_major {};
enum layout_t { mem_row_major, mem_col_major };
using hfloat16_t = _Float16;
struct float8_t {
    uint8_t data;
    __host__ __device__ float8_t() = default;
    __host__ __device__ explicit float8_t(float v) : data(encode(v)) {}
    __host__ __device__ operator float() const { return decode(data); }
};
static_assert(sizeof(float8_t) == 1);
template<class Role, unsigned M, unsigned N, unsigned K, class T, class Layout = row_major>
struct fragment {
    static_assert(M == 16 && N == 16 && K == 16);
    static constexpr unsigned num_elements = 8;
    T elements[8];
    __device__ T& operator[](unsigned i) { return elements[i]; }
    __device__ const T& operator[](unsigned i) const { return elements[i]; }
};
template<class Role, class T, class Layout>
__device__ inline void load_matrix_sync(fragment<Role, 16, 16, 16, T, Layout>& f,
                                        const T* p, unsigned stride,
                                        layout_t memory = mem_row_major) {
    const unsigned lane = threadIdx.x & 31u;
    #pragma unroll
    for (unsigned e = 0; e < 8; ++e) {
        unsigned row, col;
        if constexpr (std::is_same_v<Role, matrix_a>) {
            row = lane % 16; col = lane / 16 * 8 + e;
        } else {
            row = lane / 16 * 8 + e; col = lane % 16;
        }
        const bool column = std::is_same_v<Layout, col_major> || memory == mem_col_major;
        f[e] = p[column ? col * stride + row : row * stride + col];
    }
}
template<class T, class Layout>
__device__ inline void store_matrix_sync(T* p, const fragment<accumulator, 16, 16, 16, T, Layout>& f,
                                         unsigned stride, layout_t memory) {
    const unsigned lane = threadIdx.x & 31u;
    #pragma unroll
    for (unsigned e = 0; e < 8; ++e) {
        const unsigned row = lane / 16 * 8 + e, col = lane % 16;
        p[memory == mem_col_major ? col * stride + row : row * stride + col] = f[e];
    }
}
template<class Role, class T, class Layout>
__device__ inline void fill_fragment(fragment<Role, 16, 16, 16, T, Layout>& f, T v) {
    #pragma unroll
    for (unsigned i = 0; i < 8; ++i) f[i] = v;
}
template<class T, class LA, class LB>
__device__ inline void mma_sync(fragment<accumulator, 16, 16, 16, float>& out,
                                const fragment<matrix_a, 16, 16, 16, T, LA>& a,
                                const fragment<matrix_b, 16, 16, 16, T, LB>& b,
                                const fragment<accumulator, 16, 16, 16, float>& c) {
    static_assert(std::is_same_v<T, hfloat16_t> || std::is_same_v<T, float8_t>);
    H8 af{}, bf{};
    F8 cf{};
    #pragma unroll
    for (unsigned i = 0; i < 8; ++i) {
        if constexpr (std::is_same_v<T, float8_t>) { af[i] = decode_half(a[i].data); bf[i] = decode_half(b[i].data); }
        else { af[i] = a[i]; bf[i] = b[i]; }
        cf[i] = c[i];
    }
    const F8 result = mma_half(af, bf, cf);
    #pragma unroll
    for (unsigned i = 0; i < 8; ++i) out[i] = result[i];
}
} // namespace flightdeck_rdna2

// Only the pinned backend sees these mappings. HIP's headers were parsed above.
#define __builtin_amdgcn_wmma_f32_16x16x16_f16_w32_gfx12 flightdeck_rdna2::mma_half
#define __builtin_amdgcn_wmma_f32_16x16x16_fp8_fp8_w32_gfx12 flightdeck_rdna2::mma_fp8
#define __builtin_amdgcn_cvt_pk_fp8_f32 flightdeck_rdna2::pack
#define __builtin_amdgcn_cvt_f32_fp8 flightdeck_rdna2::unpack

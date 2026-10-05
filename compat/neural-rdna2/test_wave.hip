// SPDX-License-Identifier: MIT
// Independent host oracles for the gfx1030 adapter; no model/game inputs needed.
#include "wave.hpp"
#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <limits>
#include <random>
#include <string>
#include <vector>

static void check(hipError_t e) {
    if (e != hipSuccess) { std::fprintf(stderr, "%s\n", hipGetErrorString(e)); std::exit(2); }
}
template<class T> struct Buffer {
    T* p{};
    explicit Buffer(size_t n) { check(hipMalloc(&p, n * sizeof(T))); }
    ~Buffer() { if (p) (void)hipFree(p); }
    Buffer(const Buffer&) = delete;
};
static float reference_decode(unsigned b) {
    const unsigned magnitude = b & 127u, exponent = magnitude >> 3, mantissa = magnitude & 7u;
    float x = magnitude == 127 ? std::numeric_limits<float>::quiet_NaN()
        : exponent == 0 ? std::ldexp(float(mantissa), -9)
        : std::ldexp(1.f + float(mantissa) / 8.f, int(exponent) - 7);
    return std::copysign(x, b & 128 ? -1.f : 1.f);
}
static uint8_t reference_encode(float x) {
    const unsigned sign = std::signbit(x) ? 128 : 0;
    if (std::isnan(x)) return uint8_t(sign | 127);
    if (std::fabs(x) >= 448.f) return uint8_t(sign | 126);
    unsigned best = 0;
    double distance = std::numeric_limits<double>::infinity();
    for (unsigned candidate = 0; candidate <= 126; ++candidate) {
        const double d = std::fabs(double(std::fabs(x)) - double(reference_decode(candidate)));
        if (d < distance || (d == distance && !(candidate & 1))) { distance = d; best = candidate; }
    }
    return uint8_t(sign | best);
}
__global__ void conversion(const float* input, uint8_t* encoded, float* decoded, int* pairs, size_t n) {
    const unsigned i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= n) return;
    encoded[i] = flightdeck_rdna2::encode(input[i]);
    decoded[i * 2] = flightdeck_rdna2::decode(uint8_t(i));
    decoded[i * 2 + 1] = float(flightdeck_rdna2::decode_half(uint8_t(i)));
    const int low = flightdeck_rdna2::pack(input[i], -input[i], 0x12345678, false);
    pairs[i * 2] = low;
    pairs[i * 2 + 1] = flightdeck_rdna2::pack(input[i], -input[i], low, true);
}
__global__ void matrices(const float* a, const float* b, const float* initial, float* output, bool half) {
    using namespace flightdeck_rdna2;
    const unsigned tile = blockIdx.x * (blockDim.x / 32) + threadIdx.x / 32;
    const unsigned lane = threadIdx.x & 31u, row = lane % 16, first = lane / 16 * 8;
    F8 c{}; H8 ah{}, bh{}; I2 af{}, bf{};
    for (unsigned e = 0; e < 8; ++e) {
        const float x = a[tile * 256 + row * 16 + first + e];
        const float y = b[tile * 256 + (first + e) * 16 + row];
        ah[e] = _Float16(x); bh[e] = _Float16(y);
        af[e / 4] |= int(uint32_t(encode(x)) << ((e % 4) * 8));
        bf[e / 4] |= int(uint32_t(encode(y)) << ((e % 4) * 8));
        c[e] = initial[tile * 256 + (first + e) * 16 + row];
    }
    c = half ? mma_half(ah, bh, c) : mma_fp8(af, bf, c);
    for (unsigned e = 0; e < 8; ++e) output[tile * 256 + (first + e) * 16 + row] = c[e];
}
__global__ void residuals(const float* r, const float* s, const float* acc, float* out, unsigned n) {
    const unsigned i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < n) out[i] = flightdeck_rdna2::residual_half(r[i], s[i], acc[i]);
}
int main() {
    check(hipSetDevice(0));
    hipDeviceProp_t prop{}; check(hipGetDeviceProperties(&prop, 0));
    if (std::string(prop.gcnArchName).substr(0, 7) != "gfx1030") return 3;
    std::printf("device=%s arch=%s\n", prop.name, prop.gcnArchName);
    std::vector<float> inputs;
    for (unsigned b = 0; b < 256; ++b) inputs.push_back(reference_decode(b));
    for (unsigned b = 0; b < 126; ++b) {
        const float middle = (reference_decode(b) + reference_decode(b + 1)) / 2.f;
        for (float x : {std::nextafter(middle, -INFINITY), middle, std::nextafter(middle, INFINITY)}) {
            inputs.push_back(x); inputs.push_back(-x);
        }
    }
    inputs.insert(inputs.end(), {INFINITY, -INFINITY, 0.f, -0.f});
    std::mt19937 rng(0x6900);
    for (unsigned i = 0; i < 65536; ++i) {
        const uint32_t bits = rng(); float x; std::memcpy(&x, &bits, 4); inputs.push_back(x);
    }
    const size_t n = inputs.size();
    Buffer<float> di(n), dd(n * 2); Buffer<uint8_t> de(n); Buffer<int> dp(n * 2);
    check(hipMemcpy(di.p, inputs.data(), n * 4, hipMemcpyHostToDevice));
    conversion<<<(n + 255) / 256, 256>>>(di.p, de.p, dd.p, dp.p, n);
    check(hipGetLastError()); check(hipDeviceSynchronize());
    std::vector<uint8_t> encoded(n); std::vector<float> decoded(n * 2); std::vector<int> pairs(n * 2);
    check(hipMemcpy(encoded.data(), de.p, n, hipMemcpyDeviceToHost));
    check(hipMemcpy(decoded.data(), dd.p, n * 8, hipMemcpyDeviceToHost));
    check(hipMemcpy(pairs.data(), dp.p, n * 8, hipMemcpyDeviceToHost));
    size_t bad = 0;
    for (size_t i = 0; i < n; ++i) {
        const uint8_t expected = reference_encode(inputs[i]);
        const unsigned pair = expected | (unsigned(reference_encode(-inputs[i])) << 8);
        bad += encoded[i] != expected;
        bad += uint32_t(pairs[i * 2]) != (0x12340000u | pair);
        bad += uint32_t(pairs[i * 2 + 1]) != (pair | (pair << 16));
        const float want = reference_decode(i & 255u);
        for (unsigned decoder = 0; decoder < 2; ++decoder) {
            const float got = decoded[i * 2 + decoder];
            bad += (std::isnan(want) ? !std::isnan(got) : got != want)
                || std::signbit(got) != std::signbit(want);
        }
    }
    std::printf("E4M3 codes/midpoints/random/packs FP32+FP16 decoders cases=%zu mismatches=%zu\n", n, bad);
    const unsigned tiles = 48, count = tiles * 256;
    std::vector<float> a(count), b(count), c(count), out(count);
    Buffer<float> da(count), db(count), dc(count), dy(count);
    for (unsigned variant = 0; variant < 3; ++variant) {
        const bool half = variant != 0;
        for (unsigned i = 0; i < count; ++i) {
            a[i] = half ? float(_Float16(int(rng() % 511) - 255)) / 256.f : reference_decode(rng() % 127 | (rng() & 128));
            b[i] = half ? float(_Float16(int(rng() % 511) - 255)) / 128.f : reference_decode(rng() % 127 | (rng() & 128));
            c[i] = float(int(rng() % 127) - 63) / 64.f;
            if (variant == 2) {
                // Full finite FP16 range, including subnormals and cancellation.
                // Small fixed-point values alone cannot detect a reordered dot2.
                const uint16_t edges[] = {0, 1, 0x3ff, 0x400, 0x3555, 0x3c00, 0x7bff, 0x8001};
                const uint16_t abits = i % 4 == 0 ? edges[(i / 4) % 8]
                    : uint16_t((rng() % 0x7c00u) | (rng() & 0x8000u));
                const uint16_t bbits = uint16_t((rng() % 0x7c00u) | (rng() & 0x8000u));
                _Float16 ah, bh;
                std::memcpy(&ah, &abits, 2); std::memcpy(&bh, &bbits, 2);
                a[i] = float(ah); b[i] = float(bh);
                c[i] = std::ldexp(float(int(rng() % 127) - 63), int(rng() % 41) - 20);
            }
        }
        check(hipMemcpy(da.p, a.data(), count * 4, hipMemcpyHostToDevice));
        check(hipMemcpy(db.p, b.data(), count * 4, hipMemcpyHostToDevice));
        check(hipMemcpy(dc.p, c.data(), count * 4, hipMemcpyHostToDevice));
        for (unsigned threads : {32u, 64u, 128u, 256u, 512u}) {
            matrices<<<tiles / (threads / 32), threads>>>(da.p, db.p, dc.p, dy.p, half);
            check(hipGetLastError()); check(hipDeviceSynchronize());
            check(hipMemcpy(out.data(), dy.p, count * 4, hipMemcpyDeviceToHost));
            size_t errors = 0;
            for (unsigned t = 0; t < tiles; ++t) for (unsigned r = 0; r < 16; ++r) for (unsigned col = 0; col < 16; ++col) {
                float expected = c[t * 256 + r * 16 + col];
                for (unsigned k = 0; k < 16; ++k) expected = std::fma(a[t * 256 + r * 16 + k], b[t * 256 + k * 16 + col], expected);
                errors += out[t * 256 + r * 16 + col] != expected;
            }
            bad += errors;
            std::printf("MMA %s threads=%u outputs=%u exact_mismatches=%zu\n", variant == 2 ? "FP16-wide" : half ? "FP16" : "FP8", threads, count, errors);
        }
    }
    for (unsigned i = 0; i < count; ++i) {
        a[i] = float(_Float16(std::ldexp(float(int(rng() % 2047) - 1023), -10)));
        b[i] = float(_Float16(std::ldexp(float(int(rng() % 2047) - 1023), -10)));
        c[i] = std::ldexp(float(int(rng() % 65535) - 32767), int(rng() % 17) - 16);
    }
    // Counterexamples to rounding via FP32; both signs and magnitudes occur.
    a[0] = 0x1.908p-12f; b[0] = 0x1.c2p-1f; c[0] = 0x1.2874p-1f;
    a[1] = -0x1.6b4p+0f; b[1] = 0x1.a6cp-1f; c[1] = 0x1.719dcp+3f;
    check(hipMemcpy(da.p, a.data(), count * 4, hipMemcpyHostToDevice));
    check(hipMemcpy(db.p, b.data(), count * 4, hipMemcpyHostToDevice));
    check(hipMemcpy(dc.p, c.data(), count * 4, hipMemcpyHostToDevice));
    residuals<<<(count + 255) / 256, 256>>>(da.p, db.p, dc.p, dy.p, count);
    check(hipGetLastError()); check(hipDeviceSynchronize());
    check(hipMemcpy(out.data(), dy.p, count * 4, hipMemcpyDeviceToHost));
    size_t errors = 0;
    for (unsigned i = 0; i < count; ++i) {
        // These bounded operands fit exactly in double before the FP16 cast.
        const float expected = float(_Float16(double(a[i]) * double(b[i]) + double(c[i])));
        errors += out[i] != expected;
    }
    bad += errors;
    std::printf("C32 single-round residual outputs=%u exact_mismatches=%zu\n", count, errors);
    return bad ? 1 : 0;
}

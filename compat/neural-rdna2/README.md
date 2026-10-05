# Experimental RX 6900 XT neural backend

The `0.2.6.1-flightdeck.gfx1030.4` port executes the Linux neural renderer on
**gfx1030 / RDNA2**. Flightdeck's model importer and launch binding are Rust;
the GPU kernels are HIP compiled to gfx1030 machine code. Python is not a
runtime dependency. Actual model inference, Wine transport and D3D12 texture
submission work on the tested RX 6900 XT, but **MSFS interception remains
unverified and the measured 234 ms
per frame is unsuitable for real-time play**. The integration stays off by default.

`wave.hpp` implements the renderer's 16×16×16 FP8/FP16 matrix operations with
ordered FP32 fused multiply-add, packed half operands and DPP8 broadcasts.
Preparing eight-row groups reduces cross-lane gathers from 72 to 16 per matrix
operation; 64 DPP8 broadcasts deliver operands within those groups. FP8 values
are exactly representable in FP16. Direct E4M3FN-to-half conversion avoids the
intermediate FP32 conversion in matrix operands and preserves all finite codes,
signed zeros and signed NaNs. Software encoding covers subnormals and ties-to-even.
RDNA4 fragment layouts and all 71 graph
blocks are retained. No WMMA hardware or GPU architecture spoofing is required.

`upstream.patch` adds the gfx1030 compile path, fixes an alignment declaration,
corrects the split C32 scheduler's buffer format and fixes the FP16 raster height
check. The split C32 projection explicitly preserves the fused reference's single
FP16 rounding of its residual epilogue. This differs from the separate, ordered
FP32 rounding required inside matrix accumulation. Flightdeck selects split C32
only for gfx1030; RDNA4 retains upstream's fused default. Separately pinned
library hashes prevent selecting a backend for the wrong GPU family.

## Build and test

Source: [guentra/dlss5-amd-hip-linux at 3c7740e6](https://github.com/guentra/dlss5-amd-hip-linux/tree/3c7740e62610d4bca3fd82669002288b4293a8b4).
Use its [source archive](https://codeload.github.com/guentra/dlss5-amd-hip-linux/tar.gz/3c7740e62610d4bca3fd82669002288b4293a8b4).
The build script verifies SHA-256 before extraction. The tested compiler is
ROCm 7.1.1 / AMD Clang 20.0.0, with native C/C++ linkers, GNU patch/tar and HIP
7.1. Official AMD package inputs are recorded in `toolchain.lock.json`.
No package installation or driver change is performed.

```sh
bash compat/neural-rdna2/build.sh \
  /path/to/linux-source.tar.gz /path/to/rocm-7.1.1 build/neural-gfx1030

# Explicit GPU execution on an idle gfx1030 card; synthetic fixtures.
bash compat/neural-rdna2/test.sh build/neural-gfx1030 /path/to/rocm-7.1.1
```

The output directory must be empty. Outputs include `libdlss5_hip.so`, patched
source, test executables, `bench-normal`, compiler identification and checksums.
Tests generate roughly 600 MiB of synthetic tables; those fixtures cannot enable
the game's real-model profile. An unexpected compiler/linker change can alter
the library hash and requires qualification before updating the launcher lock.
Two builds in different directories produced the same 16,303,528-byte library:
`0841f0206917740c294ff2ed2a1ea37b0ea6be71db7238ec1777eb185a8e65e7`.
Per-file HIP compilation IDs are fixed so absolute build paths do not change it.

To prepare the experimental bundle, copy the verified upstream `0.2.6.1` bundle
to a separate directory and replace only `bin/libdlss5_hip.so` with this build.
Retain upstream's licenses, add-on and original PE trampoline. Import your local
model and configure the binding with the [development CLI](../../docs/neural-rendering.md).

## Validation on RX 6900 XT, 2026-10-05

Detailed inputs, hashes, raw timings and results are in `validation.json`.

- HIP reports `AMD Radeon RX 6900 XT`, `gfx1030`; the Rust probe matches Vulkan
  by PCI address and selects its unique device UUID, despite different driver
  names. A GPU-memory round trip succeeds. The original RDNA4 bundle is rejected.
- All 14 GPU test groups pass. Conversion checks cover 66,552 inputs, testing
  both FP32 and direct FP16 decoders including signed NaNs and zero. Independent
  scalar oracles check 184,320 FP8/FP16 matrix outputs across 32–512 threads per
  group, including finite FP16 extremes and subnormals. The primitive test uses
  the renderer's production compiler flags. All outputs match exactly.
- A separate double-precision oracle checks 12,288 C32 residual epilogues,
  including two real-model halfway cases. Dense fused/split C32 encoder and
  decoder chains check another 32,768 outputs with every window shift.
- Quantization, attention, residuals, linear layers, logical operators, FFN,
  graph operators, complete multihead blocks and the 640×1024 ViT block pass
  upstream CPU-reference tests. The synthetic full graph checks all 71 blocks,
  finite/nonconstant traces, exact replay, seed/history/reset, damaged input and
  reinitialization. Its normal untraced GPU time was 228.258 ms.
- The CPU-only Rust importer produces **223 tables / 629,178,920 bytes** identical
  to the independently run pinned upstream decoder for the exact `WEIGHTS_HT`
  archive. The committed table lock contains counts and SHA-256 hashes, not
  coefficients. Tampered tables fail even if their manifest hashes are changed.
- With those actual model weights, baseline and optimized outputs match all
  **24,883,200 RGB bytes** on a 1080p gradient and four additional static public
  evaluation images: earth, portrait, landscape and stone texture. Repeated
  frames are finite and identical. These inputs are not MSFS captures or a
  comparison with NVIDIA output.
- Flightdeck Default, Steam Proton Experimental and CachyOS each pass two
  real-model gradient frames against the native baseline and two synthetic
  frames against a closed-form oracle through the sealed-preload Wine → PE →
  native HIP path. Both weight sets also pass the V3 packed BGRA8 route: all
  8,294,400 bytes match the separate V2 float-frame reference and alpha is
  preserved.
- A separate real D3D12 test loads the actual ReShade add-on, uses the pinned
  upstream `NativeHipLive` header and inserts its VKD3D submission marker before
  a consumer on the same command list. Each runner passes three 1080p RGBA8
  frames: all **8,294,400 bytes** match direct HIP output, RGB actually changes,
  alpha is exact, and the same input/seed replays exactly. GPU completion and
  device health are checked. These tests use disposable prefixes and do not
  exercise the game's FFX dispatch hook.

The verified raw archive has SHA-256
`836f445d06ecd2e59bb9f17b84b91c143396fd76ccda1c9dc7fe81d5edd548f4`.
Its provenance is recorded as an archive, not as an original DLL. The local
research container was read only as data; it was neither executed nor shipped.
The reconstructed AMD-consumer channel/bridge maps are still experimental.
Agreement with the upstream decoder and gfx1030 reference does **not** establish
NVIDIA model equivalence or useful image quality.

### Corrections to earlier development results

The superseded gfx1030.2 primitive test disabled floating-point contraction while
production compilation did not. Running it with production flags exposed
22,500 matrix mismatches: the compiler merged ordered FMAs into `dot2` operations.
The current implementation prevents that contraction locally, and its test now
uses production flags. Earlier synthetic-only exactness claims did not cover
this error.

Actual model weights then exposed rare C32 projection rounding differences in
an intermediate gfx1030.3 candidate. The explicit single-round FP16 epilogue
fixes these cases. Only the final hash above is accepted. Performance and
correctness results below were rerun against that final build.

The D3D12 test also exposed a launcher selection error: HIP reports
`AMD Radeon RX 6900 XT`, while RADV/DXVK reports
`Radeon RX 6800/6800 XT / 6900 XT (radv ...)`. Filtering on the HIP name hid the
correct Vulkan adapter. Flightdeck now matches PCI identity, selects the Vulkan
UUID and normalizes only that adapter's display name for the upstream HIP API.
Vendor/device IDs and the adapter LUID remain intact. C driver fixtures cover
missing optional metadata, invalid PCI fields, changing extension counts and
Vulkan 1.0 fallback; ambiguous matches are rejected.

## Performance comparison

ABBA measurements on the RX 6900 XT, 14 warm GPU samples per implementation and
weight set. Each invocation runs eight 1080p frames and discards the first timing.
Both versions use the same gradient, seed 7 and `DLSS5_MH_PROD=1`. The original
uses fused C32; the optimized version uses the corrected split C32 path.

| Weights | Original gfx1030.1 median (range) | Final gfx1030.4 median (range) | Speedup |
| --- | ---: | ---: | ---: |
| Actual NR model | 866.073 ms (860.952–869.183) | 233.727 ms (232.943–234.348) | 3.71× |
| Synthetic fixture | 861.438 ms (859.374–870.781) | 226.700 ms (225.225–227.300) | 3.80× |

Real-model GPU time falls by **73.0%**, but still permits only about **4 inferences
per second before the game's work**. These are network timings on one GPU,
not MSFS FPS or a comparison against NVIDIA DLSS. Model allocations were about
4.29 GiB in the synthetic graph test. Tracing is excluded from the comparison.

A separate ABBA comparison with the previous verified gfx1030.3 backend uses
split C32 in both versions: median **248.549 → 235.013 ms**, a further **5.45%**
GPU-time reduction, with identical output. All eight combinations of the
existing ViT split/LDS tuning switches were slower than the default on this GPU;
their results are recorded without changing that default.

```sh
bash compat/neural-rdna2/benchmark.sh \
  /path/to/original-build /path/to/optimized-build /path/to/rocm-7.1.1 \
  /path/to/weight-cache build/neural-benchmark
```

Both build directories need `bench-normal` linked from the pinned upstream
`src/bench.hip` and their own kernel/network objects. The build script produces
it automatically. Every final RGB byte must match; the executable also checks
all blocks, finite output and repeatability. Logs, outputs, checksums and warm
samples are retained. Use separately named output directories for synthetic and
real-model runs.

For additional inputs, `bench-normal --input image.rgba --weights CACHE --seed 7
--runs 2 --output image.rgb` accepts 1920×1080 little-endian RGBA32F and writes
RGB32F. Run each build with its C32 policy, then compare the output files.
The four evaluation images were Lanczos-resized to cover 1920×1080 and center
cropped, with RGB8 divided by 255, alpha 1 and no color-space linearization.
Their pinned source revision and hashes are recorded in `validation.json`.

MSFS interception, HDR, VR, cockpit legibility and in-game temporal quality remain
unverified. The upstream synchronous readback/upload route and incomplete live
history also remain. No public release or default enablement is implied.
Original upstream code and Flightdeck's adapter/tests are MIT-licensed; see
`UPSTREAM-LICENSE`. No NVIDIA weights or proprietary binaries are included in
this source directory.

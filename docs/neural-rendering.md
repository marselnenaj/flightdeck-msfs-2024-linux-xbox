# Experimental AMD neural rendering

This is an **experimental CLI integration**, included in 0.2.3 and disabled by default. Flightdeck
can bind the pinned Linux HIP bridge to its licensed MSFS 2024 launch path.
**Working neural rendering in MSFS has not been demonstrated.** There is no
automatic installation, default enablement or public-release support claim.
Actual model inference is verified on the RX 6900 XT at about 234 ms per 1080p
frame, which is too slow for real-time gameplay.

The integration uses [guentra/dlss5-amd-hip-linux](https://github.com/guentra/dlss5-amd-hip-linux),
not the Windows-only AMDNR runtime. It is a community neural-renderer experiment,
not NVIDIA-certified DLSS support. Its FSR interception still has to be qualified
with MSFS. In particular, a loaded DLL or a successful bridge call does not mean
that the game supplies frames to the neural network.

## Requirements

- Linux x86_64 and native ROCm/HIP 7, with working runtime dependencies and GPU
  access. Flightdeck does not install or change system drivers.
- One compatible AMD GPU and its matching native backend: `gfx1030` (RX 6800/6900
  family) uses [Flightdeck's RDNA2 port](../compat/neural-rdna2/README.md);
  `gfx1201` (RX 9070 family) or upstream's unverified `gfx1200` (RX 9060 family)
  uses the original RDNA4 library. Other RX 6000/7000 architectures are not enabled.
- The exact `0.2.6.1` bundle at upstream revision
  `3c7740e62610d4bca3fd82669002288b4293a8b4`. Extract it into a permanent local
  directory. Flightdeck verifies its seven required components against
  [fixed hashes](../compat/neural-rendering.lock.json). The RDNA2 bundle keeps the
  six Windows/configuration components and replaces only `bin/libdlss5_hip.so`
  with the pinned `0.2.6.1-flightdeck.gfx1030.4` build.
- A complete weight cache imported from your legitimately obtained NR DLL
  310.8.0.0 or its exact `WEIGHTS_HT` resource archive. The CPU-only Rust importer
  verifies the source hash and all 223 decoded table hashes. Existing caches
  from the pinned upstream converter are accepted only with the same table
  hashes. No model weights or NVIDIA binaries are shipped here.
- A runner whose 64-bit `ntdll.dll` exports `__wine_get_unix_env`. The selected
  Flightdeck/default or alternative Proton runner is inspected, without assuming
  that a particular brand/version has this export.

Import, validation and the Flightdeck launch binding are Rust. They do not invoke
Python, run the source DLL or download model files. Keep the selected bundle,
weight cache and ROCm library in place.

## Configure and turn off

Use Flightdeck 0.2.3 or build the current tree. The earlier 0.2.2 binary does not
have these commands. The examples use the development binary; an installed
0.2.3 also accepts them through `flightdeck`. Substitute your actual paths:

```sh
target/debug/flightdeck-rust neural-weights \
  --source /path/to/nvngx_dlssnr.dll --output /path/to/new-weight-cache

target/debug/flightdeck-rust neural-rendering --runtime /path/to/runtime probe \
  --bundle /path/to/dlss5-amd-hip-linux \
  --hip-library /opt/rocm/lib/libamdhip64.so.7

target/debug/flightdeck-rust neural-rendering --runtime /path/to/runtime enable-experimental \
  --bundle /path/to/dlss5-amd-hip-linux \
  --weights /path/to/converted-weight-cache \
  --hip-library /opt/rocm/lib/libamdhip64.so.7

target/debug/flightdeck-rust neural-rendering --runtime /path/to/runtime status
target/debug/flightdeck-rust neural-rendering --runtime /path/to/runtime disable
```

`neural-weights` also accepts the exact raw archive (SHA-256
`836f445d06ecd2e59bb9f17b84b91c143396fd76ccda1c9dc7fe81d5edd548f4`).
Its manifest distinguishes a verified archive from a verified original DLL;
it never labels an archive as the original DLL. Unknown DLLs/archives are
rejected. Import writes about 600 MiB into a private staging directory and
publishes a complete cache atomically. An existing output directory is preserved.
This creates a cache; enabling the experimental game profile is a separate step.

`probe` checks the bundle and runner and runs ROCm enumeration plus a small
GPU-memory round trip in a bounded child. It matches the supported HIP GPU to an
AMD Vulkan adapter by PCI address, requires a unique Vulkan device UUID and
verifies the native backend's architecture. Missing or ambiguous identities
prevent enabling this experiment. An RDNA4
library is rejected on gfx1030, even though HIP itself works on that card.
Private ROCm dependencies under `lib/rocm_sysdeps/lib` are available to the HIP
child without changing the launcher or Vulkan probe's library search path.
This does not run the model or launch the game.

`enable-experimental` takes the installation's play lease, requires an idle
Windows profile, validates the converted cache and saves an explicit per-runtime
profile. Runner, ROCm, GPU identity or model changes require reconfiguration.
Profiles saved before PCI/UUID matching also require reconfiguration. `disable` works
even when an input is missing or the saved configuration is damaged. Close the
game before changing the profile.

## What happens at launch

Flightdeck places the matching ReShade loader, add-on, PE trampoline and modified
VKD3D pair in the temporary `.xodus-launch-*` view. Existing graphics proxies or
add-ons cause a conflict error. The original game files and installed renderer
remain untouched. Removing the launch view removes the overlay.

Only the game's Wine command receives the HIP preload and DLL overrides; the
launcher, account broker and companion helpers keep their normal environment.
The native bridge lives in a sealed memory file, exposed through the launcher's
live `/proc/PID/fd` path, so spaces in paths cannot split `LD_PRELOAD`. GDK/store
overrides and unrelated preloads remain intact. ROCm is passed by absolute path
without replacing `LD_LIBRARY_PATH`.

Inherited GPU selectors are rejected for this experiment: selecting different
devices for HIP and Vulkan would be invalid. Flightdeck selects the matched
Vulkan UUID through DXVK. Its display name is normalized to the actual HIP
product name because the upstream bridge uses that name to find the device.
This fixes HIP/RADV naming differences; vendor/device IDs and the LUID stay
unchanged. Unrelated DXVK settings remain intact. Logs from the add-on are retained in
`private/neural-rendering-logs` inside the runtime. `status` describes configuration;
it deliberately never reports MSFS rendering as verified.

## Validation and limits

The optional bridge test calls `dlss5_run_frame(NULL)` through the **actual**
Windows DLL and requires the Linux backend's `not initialized` response. A
missing bridge returns a different error. The test uses a disposable prefix and
needs neither model weights nor a supported GPU:

```sh
FLIGHTDECK_TEST_NR_BUNDLE=/absolute/path/to/dlss5-amd-hip-linux \
FLIGHTDECK_TEST_NR_WINE=/absolute/path/to/runner/files/bin/wine \
cargo test --lib real_wine_bridge_calls_linux_through_sealed_preload -- --ignored --nocapture
```

Automated tests also cover sealed-preload lifetime, paths with spaces, proxy
collisions, mixed DLL override groups, malformed PE exports, incomplete weight
manifests, configuration recovery and the disabled path.

The real PE-to-Linux call, synthetic inference and actual-model inference passed
locally on 2026-10-05 with three distinct runner executables (SHA-256 checked,
each test with a new disposable prefix):

| Runner | Wine executable SHA-256 prefix | Synthetic / actual model / D3D12 texture |
| --- | --- | --- |
| Flightdeck Xodus `11.0-20260803-3-g7c0b4354` | `041dd0e1ff3f799c` | Passed / passed / passed |
| Steam Proton Experimental `11.0-20261001` | `7a6de49c00d8ed2ba` | Passed / passed / passed |
| CachyOS `10.0-sunset-slr` | `d98a8ea517867ed9d` | Passed / passed / passed |

Fresh-prefix initialization also starts 32-bit Wine helpers, which report that
the x86_64 preload cannot be loaded in those helpers. The 64-bit probe still
reaches the Linux backend. This test does not establish 32-bit support.

The Rust suite and Clippy pass. A native driver fixture separately exercises the
HIP probe's memory round trip, unsupported architecture and driver-error paths.
A C Vulkan fixture checks PCI metadata, invalid fields and optional-extension
failures while preserving basic discovery and Vulkan 1.0 compatibility.
These fixtures do not execute a real GPU.

On the local RX 6900 XT, all 14 GPU test groups pass, including FP8/FP16
primitives with production compiler flags, independent C32 rounding oracles,
multihead/ViT blocks and the full 71-block graph. The Rust importer produces
223 tables (629,178,920 bytes) identical to the independently run upstream
decoder from the exact SHA-verified resource archive. That archive was obtained
for local research and recorded as an archive; no NVIDIA DLL was executed or
installed and no model data is included in Flightdeck.

Using those actual model weights, the optimized backend and original gfx1030
reference produce byte-identical 1080p RGB output for a gradient and four static
evaluation images. Median GPU time falls from **866.073 to 233.727 ms**, a
**3.71×** speedup in this network benchmark. Synthetic weights measure 226.700 ms.
A separate comparison with the previous verified gfx1030.3 build shows a
further 5.45% GPU-time reduction from direct FP8-to-half operand conversion.
About four actual-model inferences per second, before the game's work, remain
unsuitable for real-time play. These measurements are not MSFS frame rates.

Each runner's actual-model test compares two float frames with the native
baseline. Both synthetic and actual weights also pass the packed BGRA8 route
against a separate float-frame reference, including all output bytes, alpha
preservation and repeatability. Reproduce the optional inference tests with:

```sh
FLIGHTDECK_TEST_NR_BUNDLE=/absolute/path/to/gfx1030-bundle \
FLIGHTDECK_TEST_NR_WINE=/absolute/path/to/runner/files/bin/wine \
FLIGHTDECK_TEST_NR_HIP_LIBRARY=/absolute/path/to/libamdhip64.so.7 \
FLIGHTDECK_TEST_NR_SYNTHETIC_WEIGHTS=/absolute/path/to/synthetic-cache \
FLIGHTDECK_TEST_NR_WEIGHTS=/absolute/path/to/imported-model-cache \
FLIGHTDECK_TEST_NR_REFERENCE=/absolute/path/to/baseline-gradient.rgb \
cargo test --lib real_wine_bridge_runs_ -- --ignored --nocapture --test-threads=1

FLIGHTDECK_TEST_NR_ARCHIVE=/absolute/path/to/weights-ht.bin \
cargo test --lib real_archive_import_matches_all_independent_upstream_table_hashes \
  -- --ignored --nocapture
```

The D3D12 test also loads the real ReShade add-on, records the upstream live
submission marker and reads its result with a consumer on the same command list.
Three full RGBA8 outputs per runner match the direct HIP reference byte for byte,
including varying alpha. This exercises actual GPU texture transfer and queue
ordering. The FFX dispatch hook itself is not invoked by this test.

It requires MinGW C++, the hash-verified upstream source archive, Flightdeck's
verified `dxgi.dll` from its graphics bundle and a freshly built launcher:

```sh
cargo build --locked
FLIGHTDECK_TEST_NR_BUNDLE=/absolute/path/to/gfx1030-bundle \
FLIGHTDECK_TEST_NR_WINE=/absolute/path/to/runner/files/bin/wine \
FLIGHTDECK_TEST_NR_HIP_LIBRARY=/absolute/path/to/libamdhip64.so.7 \
FLIGHTDECK_TEST_NR_WEIGHTS=/absolute/path/to/imported-model-cache \
FLIGHTDECK_TEST_NR_SOURCE_ARCHIVE=/absolute/path/to/linux-source.tar.gz \
FLIGHTDECK_TEST_NR_DXGI=/absolute/path/to/resources/graphics/dxgi.dll \
FLIGHTDECK_TEST_NR_LAUNCHER=/absolute/path/to/target/debug/flightdeck-rust \
cargo test --lib real_wine_d3d12_texture_runs_model_before_same_list_consumer \
  -- --ignored --nocapture --test-threads=1
```

The test supplies the selected runner's own D3DCompiler dependencies in its
temporary view, as a normal Proton prefix does. No game installation is needed.
Read-only inspection of the local MSFS 2024 installation found
`amd_fidelityfx_dx12.dll` version `1.0.1.41314` with `ffxDispatch`, a loader version
recognized by the pinned upstream hook. This is a prerequisite check, not proof
that MSFS frames pass through the hook.

The [RDNA2 build and evidence](../compat/neural-rdna2/README.md) describe fixture
creation, baseline generation, compiler-rounding corrections and raw timing
samples. The optimized backend uses DPP8 broadcasts and ordered FP32 accumulation;
its split C32 path preserves the fused reference's single FP16 epilogue rounding.
The RDNA4 kernel policy is unchanged.

The coefficient layouts include reconstructed AMD-consumer channel and bridge
maps. Agreement with that reference establishes implementation consistency,
not equivalence to NVIDIA output. Model image quality, MSFS FSR-hook compatibility,
HDR, cockpit/instrument legibility, game frametimes and VR remain unverified.
Upstream also uses synchronous readback/upload and incomplete live temporal
history. The separate black loading-video issue is unaffected by this binding.

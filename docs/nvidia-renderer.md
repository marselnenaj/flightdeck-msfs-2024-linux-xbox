# NVIDIA renderer corrections

Flightdeck 0.1.11 corrects the ignored NVIDIA Low Latency opt-out in the pinned
DXVK and makes the complete D3D11/D3D12 compatibility profile automatic. It
includes the VKD3D corrections from 0.1.9. This does not yet establish that the
black main MSFS scene is fixed on NVIDIA hardware.

See [Steam evidence and completion criteria](nvidia-steam-parity.md) for the
source comparison, confirmed NVIDIA reference cases and remaining game checks.

## DXVK correction

The pinned DXVK revision is
[`6227b633e8d528952873c4c146773663964fcb31`](https://github.com/doitsujin/dxvk/tree/6227b633e8d528952873c4c146773663964fcb31).
`DxvkOptions` reads `dxvk.disableNvLowLatency2`, but `disableUnusedFeatures`
never consults it. Disabling NVAPI or hiding the vendor also leaves DXVK's
independent Reflex tracker available. MSFS can create D3D11 devices alongside
its D3D12 swapchains.

`compat/patches/dxvk-nvidia-low-latency.patch` honors the explicit opt-out before
DXVK derives enabled device extensions. It retains the existing 32-bit guard.
The only other source change identifies this build as
`v3.0.2-10-g6227b633e8d5289-flightdeck-ll1`.
Automatic and Compatibility set both `dxvk.disableNvLowLatency2 = True` and
`dxvk.latencySleep = False`, in addition to the VKD3D exclusion and NVAPI/NGX
opt-out. The latency tracker setting also works with the old DXVK, but only the
patched bundle restores its extension opt-out. Features mode leaves the normal
path available, subject to explicit inherited overrides.

## VKD3D corrections

The bundled runner contains VKD3D revision
`651f17762e439feeef22dbb4ee7eff167ee503d4` from July 2026. Its NVIDIA low-latency
swapchain bookkeeping follows creation/destruction of Vulkan presentation
surfaces, rather than the DXGI object's lifetime. The backport contains:

- [Separate low-latency registration and guard it by extension availability](https://github.com/HansKristian-Work/vkd3d-proton/commit/a74426fb3fe92642186a270941a39a997737b3ef).
- [Count swapchains at registration/unregistration](https://github.com/HansKristian-Work/vkd3d-proton/commit/7a3eb926b959ab27ad3cbb6a028381d807b39e0b).
- [Defer low-latency demotion until another swapchain is actually used](https://github.com/HansKristian-Work/vkd3d-proton/commit/e14020081a14f2d595d03049b091fea748684a4f).

The published `compat/patches/vkd3d-nvidia-low-latency.patch` applies these
changes to the existing renderer. The development tree now carries them in
`compat/patches/vkd3d-renderer.patch` together with the layout correction below.
It does not replace Wine or the Fenix overlay. All
NVIDIA modes use the corrections in the full installer package. AMD/Intel-only
starts retain their existing renderer.

Automatic and Compatibility exclude `VK_NV_low_latency2` in VKD3D. Features
mode restores normal extension availability unless an exclusion was explicitly
inherited. The patched lifetime handling remains relevant when features are enabled.

The “never been rendered to” warning alone does not establish the cause of a
missing scene. Earlier launcher defaults set `VKD3D_DEBUG=info`, which is below
`warn` in VKD3D and suppressed this warning. 0.1.11 defaults to `warn`; absence
of the warning in an older info-level log cannot demonstrate a fix.

## 3D texture layout correction (unreleased)

The renderer candidate adds upstream
[`6831d28e5e71a252e740e474ecbad37d27c3f205`](https://github.com/HansKristian-Work/vkd3d-proton/commit/6831d28e5e71a252e740e474ecbad37d27c3f205)
to the same pinned VKD3D revision. With `VK_KHR_maintenance9`, image barriers
for 2D-array-compatible 3D images interpret the layer count as depth slices.
The old copy helper uses `layerCount = 1`, leaving the other slices in their
previous layouts. The correction uses `VK_REMAINING_ARRAY_LAYERS` for 3D
resources, including copy source/destination transitions and initialization.
This follows the [Vulkan barrier rules](https://docs.vulkan.org/refpages/latest/refpages/source/VkImageMemoryBarrier2.html)
and also works without maintenance9. It does not disable the extension.

The only additional source changes fix the build/version templates so exported
sources retain an unambiguous identity instead of inheriting a parent Git
repository's version. The build ID is `628afa6f9cfece4`: the first 15 hex digits
of SHA-256 over sorted `path + NUL + patched SHA-256 + newline` records for the
four changed files under `libs/vkd3d/`. The version string is
`651f17762e439fe-flightdeck-r2`. Both templates, all changed-source hashes,
recursive submodule revisions and binary hashes are recorded in the graphics lock.

The isolated test demonstrates a real layout bug and its correction on AMD.
It does not establish that this bug causes the reported NVIDIA black main view.
Confirmation requires the corrected DLL pair to load on an affected NVIDIA
system and the main globe/map and cockpit to render. The public 0.1.16 package
does not contain this candidate.

## Installation contract

`compat/graphics.lock.json` pins the runner and replacement hashes for
D3D12, D3D12Core, DXGI, D3D11 and D3D10Core, their license bundle and both source
revisions/patches. DXVK also records all recursive source dependencies and the
input/output hashes of its two changed source files. The full installer requires `--graphics` and
checks it against the frozen source archive. Launcher installation and wheel
building also verify the payload.

Before launch, under the runtime lease, all source and destination DLLs are
checked before copying. Only known runner files, absent files or earlier managed
copies are replaced. A custom DLL preserves the complete set. A copy failure stops
launch; retry completes a partial update. The shared runner is unchanged.
Changing the runner restores managed prefix DLLs from that runner as a set.
A source-only launcher without the bundle also restores managed copies;
otherwise it uses the existing runner. The old D3D12-only marker is accepted when upgrading. Launchers predating
0.1.11 cannot restore the five-file marker; use the current source-only launcher
to restore managed files before manually downgrading to an older launcher.

## Build

Use MinGW, Meson, Ninja, glslang and Wine WIDL. Clone VKD3D-Proton, check out the
pinned revision, initialize its recursive submodules, and apply the patch with
`git apply --check` followed by `git apply`. From that checkout:

```sh
meson setup ../vkd3d-build --cross-file build-win64.txt --buildtype release \
  --prefix /vkd3d -Denable_tests=true -Denable_extras=false -Denable_trace=false
ninja -C ../vkd3d-build -j8
mkdir ../graphics
x86_64-w64-mingw32-strip ../vkd3d-build/libs/d3d12/d3d12.dll -o ../graphics/d3d12.dll
x86_64-w64-mingw32-strip ../vkd3d-build/libs/d3d12core/d3d12core.dll -o ../graphics/d3d12core.dll
cat COPYING LICENSE AUTHORS > ../graphics/LICENSE
```

Append the original license texts from DXIL-SPIRV, DXBC-SPIRV, SPIRV-Cross,
SPIRV-Tools, SPIRV-Headers and Vulkan-Headers to the bundle's `LICENSE`, retaining
their copyright notices. The VKD3D source archive includes all these files.

The bundle's `manifest.json` contains only `schema: 2` and the `base`/`files`
maps from the graphics lock. Maintainers must review and update output hashes
when rebuilding with different compiler inputs. No proprietary NVIDIA DLL or
host graphics driver is downloaded or embedded.

Build DXVK from its pinned revision with every recursive submodule at the
revision recorded in `compat/graphics.lock.json`. Apply its patch to a fresh
export and verify the changed-file hashes before compiling:

```sh
meson setup ../dxvk-build --cross-file build-win64.txt --buildtype release \
  --prefix /dxvk -Denable_d3d8=false -Denable_d3d9=false \
  -Ddxbc-spirv:enable_tests=false
ninja -C ../dxvk-build -j8
x86_64-w64-mingw32-strip ../dxvk-build/src/dxgi/dxgi.dll -o ../graphics/dxgi.dll
x86_64-w64-mingw32-strip ../dxvk-build/src/d3d11/d3d11.dll -o ../graphics/d3d11.dll
x86_64-w64-mingw32-strip ../dxvk-build/src/d3d10/d3d10core.dll -o ../graphics/d3d10core.dll
```

Append DXVK, DXBC-SPIRV, libdisplay-info, both SPIRV-Headers revisions,
Vulkan-Headers, MinGW DirectX headers and OpenVR license texts to the existing
bundle license. The corresponding DXVK source archive contains all pinned
submodules, original notices, the patch and graphics lock. The 0.1.11 DXVK build
used MinGW GCC 16.2.0, Meson 1.10.0 and Ninja 1.13.2.

Create the standard checked launcher source archive, then pass this bundle to
`scripts/full-installer-release.py` alongside the pinned 0.1.9 native archive. The full installer also contains
the [Store session recovery correction](store-session-refresh.md).
Distribute the corresponding patched VKD3D and DXVK sources, pinned submodules and their
original licenses with the release, separately from the WineGDK/Xodus sources.

## Validation

```sh
python3 scripts/check-graphics-rendering.py --runner /path/to/runner \
  --bundle /path/to/graphics --output build/new-render-check
```

This creates an isolated prefix and compares the original libraries, the patched
set with features available, and the patched set with the automatic profile.
The program keeps D3D11 and D3D12 devices active together. It checks every pixel
in readback buffers, presents frames, creates an initially unused second D3D12
swapchain, changes sizes and buffer counts, closes it and continues presenting
on the primary. The D3D11 swapchain renders and resizes alongside both D3D12
windows. No game or account is used.

All three cases passed on an AMD Radeon RX 6900 XT: 148 D3D12 plus 48 D3D11
rendered/read-back/presented frames per case. Logs confirm the patched DXVK
build and effective configuration. This verifies loading and basic mixed-API
rendering, not the NVIDIA driver's extension path or MSFS rendering.
The earlier VKD3D-only probe also passed the upstream
`test_unbound_rtv_rendering` test (32 assertions).

### Volume layout regression

With a Vulkan driver supporting maintenance9, the Khronos validation layer,
MinGW and a working graphical session:

```sh
python3 scripts/check-volume-layout.py --runner /path/to/runner \
  --baseline /path/to/released-graphics --bundle /path/to/candidate-graphics \
  --output build/volume-layout-check
```

Use `--validation-layer-path /path/to/layer-json-directory` if the validation
layer is supplied outside the system loader's search path. The script creates
an isolated prefix, enables validation only for its child processes, and uses
no game or account. Its result records the loaded bundle's hashes.

`tests/graphics/volume-copy.c` uploads a pattern varying across all three axes
to a 32 × 16 × 4 render-target-capable volume, copies the whole texture, and
reads all 2,048 pixels back. Passing requires active validation in all four
runs, reproduction of the released renderer's layout error beyond slice zero,
zero validation errors with the correction, and zero errors with maintenance9
disabled in both builds. An unsupported extension cannot silently produce a
passing regression result. Pixel readback alone is insufficient: on AMD the
old renderer also returns correct pixels despite eight layout validation errors.

# NVIDIA renderer corrections

Flightdeck 0.1.9 addresses two source-level defects. It does not
establish that the reported black main MSFS scene is fixed on NVIDIA hardware.

See [Steam evidence and completion criteria](nvidia-steam-parity.md) for the
source comparison, confirmed NVIDIA reference cases and remaining game checks.

## Scope

The bundled runner contains VKD3D revision
`651f17762e439feeef22dbb4ee7eff167ee503d4` from July 2026. Its NVIDIA low-latency
swapchain bookkeeping follows creation/destruction of Vulkan presentation
surfaces, rather than the DXGI object's lifetime. The backport contains:

- [Separate low-latency registration and guard it by extension availability](https://github.com/HansKristian-Work/vkd3d-proton/commit/a74426fb3fe92642186a270941a39a997737b3ef).
- [Count swapchains at registration/unregistration](https://github.com/HansKristian-Work/vkd3d-proton/commit/7a3eb926b959ab27ad3cbb6a028381d807b39e0b).
- [Defer low-latency demotion until another swapchain is actually used](https://github.com/HansKristian-Work/vkd3d-proton/commit/e14020081a14f2d595d03049b091fea748684a4f).

`compat/patches/vkd3d-nvidia-low-latency.patch` applies exactly these changes to
the existing renderer. It does not replace Wine or the Fenix overlay. Both
NVIDIA modes use the backport in the full installer package. AMD/Intel-only
starts retain their existing renderer.

Compatibility mode additionally disables `VK_NV_low_latency2` in VKD3D. Its
extension discovery is independent of NVAPI loading and vendor-name hiding:
turning off NVAPI or Frame Generation did not bypass this code path. Existing
extension exclusions are retained. Automatic restores normal extension
availability unless an exclusion was explicitly inherited.

The warning about presenting a buffer that has never been rendered to is not
suppressed. VKD3D allows such a present and clears the output; the warning alone
does not establish the cause of the missing scene. These upstream lifetime fixes
are not a proven diagnosis of the tester's black main window.

## Installation contract

`compat/graphics.lock.json` pins the input runner's D3D12 DLL pair, rebuilt pair,
license, source revision and patch. The full installer requires `--graphics` and
checks it against the frozen source archive. Launcher installation and wheel
building also verify the payload.

Before launch, under the runtime lease, both source and destination DLLs are
checked before copying. Only known runner files, absent files or earlier managed
copies are replaced. A custom DLL preserves the pair. A copy failure stops
launch; retry completes a partial update. The shared runner is unchanged.
Changing the runner restores managed prefix DLLs from that runner as a pair.
A source-only launcher without the bundle also restores managed copies;
otherwise it uses the existing runner. Launchers predating 0.1.9 do not
understand this restoration marker.

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

The bundle's `manifest.json` contains only `schema: 1` and the `base`/`files`
maps from the graphics lock. Maintainers must review and update output hashes
when rebuilding with different compiler inputs. No proprietary NVIDIA DLL or
host graphics driver is downloaded or embedded.

Create the standard checked launcher source archive, then pass this bundle to
`scripts/full-installer-release.py` alongside the pinned 0.1.9 native archive. The full installer also contains
the [Store session recovery correction](store-session-refresh.md).
Distribute the corresponding patched VKD3D sources, pinned submodules and their
original licenses with the release, separately from the WineGDK/Xodus sources.

## Validation

```sh
python3 scripts/check-graphics-rendering.py --runner /path/to/runner \
  --bundle /path/to/graphics --output build/new-render-check
```

This creates an isolated prefix and compares the original renderer, backport
and backport with the low-latency extension excluded. The D3D12 program checks
every pixel in readback buffers, calls Present, creates an initially unused
second swapchain, renders both windows, changes sizes and buffer counts, closes
the second window and continues rendering the first. It uses no game or account.
`--visible` keeps each test window in front for compositor-output inspection.

The local run passed on an AMD Radeon RX 6900 XT. Both compositor screenshots
also contained the expected green/blue image over the entire captured client
area. The upstream `test_unbound_rtv_rendering` shader-rendering test also passed
all 32 assertions with the patched renderer. These tests verify compilation,
loading and basic render/swapchain behavior, not NVIDIA driver paths, DLSS,
Reflex, MSFS rendering or flight stability on RTX 4060/5060 Ti.

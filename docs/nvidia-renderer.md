# NVIDIA renderer corrections

This page records technical investigations and their validation at the time.
For current support, see [NVIDIA graphics](graphics.md): normal rendering is
confirmed by user testing, with remaining issues such as missing startup video.

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

The 0.1.16 `compat/patches/vkd3d-nvidia-low-latency.patch` applies these
changes to the existing renderer. The 0.1.17 release carries them in
`compat/patches/vkd3d-renderer.patch` together with the layout correction below.
It does not replace Wine or the Fenix overlay. All
NVIDIA modes use the corrections in the full installer package. AMD/Intel-only
starts retain their existing renderer.

### Confirmed follow-up: black main view persists

On 2 October 2026, the affected tester `autopilot01tr` reported testing **0.1.18**
in **Automatic** mode. The log identifies corrected VKD3D build
`628afa6f9cfece4`, but the main globe/map and cockpit remain black. The
"never been rendered to" warning also remains. This is a confirmed negative
result for that system: the layout correction below did not fix its black main
view. It remains a separately demonstrated bug fix, not an established cause of
the NVIDIA issue. The warning alone still cannot identify the cause.

### Intro startup workaround

Included in the 0.2.0-dev.1 native prerelease and its Python reference source.

The [black-world-map report](https://forums.flightsimulator.com/t/black-world-map-after-su3/734967)
describes a black main map/cockpit with a rendered second view on an RTX 4090.
Follow-ups [5](https://forums.flightsimulator.com/t/black-world-map-after-su3/734967/5)
and [17](https://forums.flightsimulator.com/t/black-world-map-after-su3/734967/17)
report that starting with `-FastLaunch` avoids the problem. A separate
[May 2026 report](https://forums.flightsimulator.com/t/blank-map-screen-after-starting-msfs/761359/14)
also confirms this workaround after black loading and game views. These are
Windows Store-edition reports with a closely matching symptom, not a confirmed
diagnosis of Flightdeck's Linux/NVIDIA failure.

[Microsoft's black-loading-screen guide](https://flightsimulator.zendesk.com/hc/en-us/articles/4406047066770-How-to-fix-a-black-loading-screen)
also documents this argument for Store and Steam. That guide names the MSFS
2020 package; the 2024 evidence above comes from firsthand reports. The flag
skips the opening logos: [2024 reports distinguish it from the later loading
video](https://forums.flightsimulator.com/t/any-quick-launch-trick-for-2024-can-we-turn-off-that-video-and-the-logos/667368/12),
which can still play. It is not a general codec repair.

Flightdeck previously omitted this argument. Both launch bridges now add
`-FastLaunch` to the actual MSFS process for 2020 and 2024, including the default
runner, selected Proton versions and Fenix-managed launches. The change avoids
the game's initial intro path without replacing the renderer or removing game
files. Explicit arguments retain their exact boundaries and order, and an
existing case-insensitive `-FastLaunch` is not duplicated. Set
`FLIGHTDECK_FAST_LAUNCH=0` in the launch environment to disable automatic
insertion for an intro comparison. It does not remove an explicitly supplied
argument. This change is not included in published 0.1.22.

The loader regression exercises both editions, both loader modes, inherited
image descriptors, argument preservation and the opt-out. The real Proton probe
also requires the Windows PE to receive `-FastLaunch`; a launcher-side string
alone cannot make that check pass. Its synthetic rendering test still cannot
qualify the reported RTX hardware or actual MSFS rendering.

### Further evidence checked on 4 October 2026

- [November 2024 reports](https://forums.flightsimulator.com/t/black-screen-but-with-ui-elements-visible/666278/6)
  describe a black world that remains visible through blurred menu panels.
  This suggests a problem after at least some scene rendering in those cases;
  it does not prove a particular shader, overlay or Linux driver fault.
- [September 2025 follow-up](https://forums.flightsimulator.com/t/black-world-map-after-su3/734967/6):
  changing NVIDIA's Windows VSync override from Fast to application-controlled
  fixed the black map/loading screen for that reporter.
  [February 2025 report](https://forums.flightsimulator.com/t/black-screen-but-with-ui-elements-visible/666278/19)
  independently reports avoiding Fast VSync. These Windows driver controls are
  not equivalent evidence for forcing a Vulkan presentation mode on Linux.
- [August 2026 / SU6 beta](https://forums.flightsimulator.com/t/blank-map-screen-after-starting-msfs/761359/15):
  VSync on, Reflex off and a restart restored rendering. Settings could then be
  enabled again. The report changes several variables together, so it does not
  isolate VSync as the cause. Flightdeck's saved-Reflex reconciliation covers
  the disabled-Reflex part; it does not force VSync for every NVIDIA user.
- [Proton video issue #8267](https://github.com/ValveSoftware/Proton/issues/8267)
  reports H.264 decoding failure and color bars on AMD in 2024. Another
  [firsthand reply](https://github.com/ValveSoftware/Proton/issues/8255#issuecomment-2499121024)
  reports broken startup videos with working world textures afterwards.
  Video errors alone therefore do not establish the cause of a black world.
- [Proton issue #9385](https://github.com/ValveSoftware/Proton/issues/9385)
  records a January 2026 black launch on RTX 3060 / driver 590.48.01 with
  Experimental. It supplies no confirmed remedy and insufficient detail to
  identify it with Flightdeck's primary-view failure.
- [NVIDIA 615.71.09 report](https://forums.developer.nvidia.com/t/615-71-09-rtx-5070-ti-linux-black-flag-resynced-freezes-in-gameplay-with-vk-error-device-lost-works-on-610-57-04/384169):
  another DX12 game freezes with `VK_ERROR_DEVICE_LOST` on an RTX 5070 Ti.
  Its comparison also changes the kernel. This matches the tester's driver
  version, but neither the application nor the reported error, and does not
  establish a driver downgrade as a fix for Flightdeck.

These findings strengthen the intro/presentation investigation, but do not
establish one universal NVIDIA fix. Full-display hangs, VR-only crashes,
black login dialogs and missing aircraft instruments are separate symptoms.

### Default-runner DLL mapping correction

Included in the 0.2.0-dev.1 native prerelease and its Python reference source.

Extending `scripts/check-proton.py` with `--default-runner` exposed a separate
reproducible failure: the synthetic main EXE receives `-FastLaunch`, but
`LoadLibraryW` of a mapped working-directory DLL fails with Windows error 193.
The exact pinned Wine source at `b1dd32734a34472a28eb5be9922df06e07ac0834`
consults `WINE_DLL_FILE_MAP` in `open_main_image` in
`dlls/ntdll/unix/loader.c`; its ordinary DLL loader does not consult that map.
Thus supplying aliases alone does not redirect delayed DLL loads.

Both bridges now expose the already-open image descriptors through the game
view in default mode too, including its working directory. The default EXE
still uses the header stub and Wine's native image mapping. Normal resources
remain links to their originals; encrypted files are untouched. Selected
Proton continues to use the portable EXE path. The probe verifies both
working-directory and module-directory loads against deliberately invalid
on-disk DLL placeholders. This is a loader correction, not evidence that the
affected MSFS installation has an encrypted DLL at the point of failure.

Final local checks pass the default path through Python and Rust, and the
Experimental path including profile return. All use synthetic PEs on AMD/RADV.
CachyOS `cachyos-10.0-sunset-slr` passes the loader/argument checks, but two later
multiwindow-renderer runs time out after 90 seconds in the host compositor.
The same PE and prefix pass all 148 D3D12 and 48 D3D11 frame checks inside a
1280x800 Wine desktop; they also pass after that setting is removed and Wine is
restarted. An earlier complete CachyOS run, including profile return, passed.
This is an intermittent rendering-test result, not a demonstrated requirement
for virtual desktops or a reproduction of the NVIDIA MSFS problem. No automatic
desktop-mode change follows from it.

### Experimental follow-up and saved NVIDIA options

The [3 October 2026 follow-up](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/1#issuecomment-5973980230)
uses Flightdeck 0.1.21, Steam Proton Experimental, Automatic graphics, RTX 4080
and NVIDIA 615.71.09. DXVK `v3.1.1-47-g685301564ea3486` and VKD3D build
`44cf7c2042168f3` load; the entire window is black and the blank-present warning
remains. This is another negative result, not evidence that switching Proton
resolved the issue.

In that [exact DXVK revision](https://github.com/doitsujin/dxvk/blob/685301564ea3486/src/dxvk/dxvk_device_filter.cpp),
`Found device` is logged before the UUID/name/CPU filters. Listing NVIDIA,
the AMD iGPU and llvmpipe does not establish that all are offered to the game or
that the wrong GPU was selected. The same revision supports UUID filtering.
Its [vendor override](https://github.com/doitsujin/dxvk/blob/685301564ea3486/src/dxgi/dxgi_adapter.cpp)
uses RX 6700 XT IDs when hiding NVIDIA; an AMD label is not evidence of an AMD
driver. In [VKD3D](https://github.com/HansKristian-Work/vkd3d-proton/blob/44cf7c2042168f3/libs/vkd3d/swapchain.c),
`user index` refers to a swapchain backbuffer, not a GPU index.

The 0.2.0-dev.1 launcher reconciles saved MSFS 2024 Video options with the
effective NVIDIA mode. Previously it disabled the runtime APIs while retaining
saved DLSS, Reflex and DLSS frame-generation requests. It now backs up and
adjusts only these known values, and restores unchanged managed values when
features are enabled again. Both implementations share the undo format;
malformed/linked settings are preserved. This addresses a configuration
inconsistency, not a confirmed diagnosis of the tester's black viewport.

A synthetic comparison using the published 0.1.22 Python implementation
reproduces the inconsistent saved values; the candidate replaces them and
restores the original bytes on a mode change. The
[Python](../tests/native_graphics_settings.rs) and
[native](../tests/native_graphics_settings.rs) regressions also cover migration,
user changes, interrupted writes, optional package-metadata failures and
UTF-8/UTF-16 configuration files. The GPU/driver are mocked in this comparison;
it does not exercise NVIDIA rendering or demonstrate a black-screen fix.

The 0.1.22 portable-loader working-directory correction is separate and was not
present in the 0.1.21 report. The exact newer DXVK also still ignores
`disableNvLowLatency2` when deriving device extensions; `latencySleep = False`
disables its tracker but does not establish extension exclusion. The bundled
DXVK patch applies to the default stack, not arbitrary selected Proton DLLs.

The experimental Proton selection now enables a complete alternative Wine,
DXVK and VKD3D trial with a separate prefix and a return to the original profile.
See [Proton trials](runtime.md#experimental-proton-selection). Neither a
successful synthetic rendering test nor the new selector establishes that the
NVIDIA MSFS issue is resolved.

Automatic and Compatibility exclude `VK_NV_low_latency2` in VKD3D. Features
mode restores normal extension availability unless an exclusion was explicitly
inherited. The patched lifetime handling remains relevant when features are enabled.

The “never been rendered to” warning alone does not establish the cause of a
missing scene. Earlier launcher defaults set `VKD3D_DEBUG=info`, which is below
`warn` in VKD3D and suppressed this warning. 0.1.11 defaults to `warn`; absence
of the warning in an older info-level log cannot demonstrate a fix.

## 3D texture layout correction (0.1.17)

The [0.1.17 release](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.17) adds upstream
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
It does not establish that this bug causes the reported NVIDIA black main view;
the 0.1.18 follow-up above confirms no improvement on the reporting system.
Further investigation needs to compare the main globe/map and cockpit with a
complete alternative Proton environment and record the selected version and
renderer. Build `628afa6f9cfece4` identifies the corrected Flightdeck renderer;
seeing that build in a log establishes that it loaded, not that the NVIDIA issue
is resolved. To end a Proton trial, use **Return to Flightdeck environment** in
the launcher.

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

Create the checked launcher source archive and use `scripts/native-release.py`
with the component archive selected by the current `compat/bootstrap.lock.json`
and this matching graphics bundle. See [native package commands](../BUILDING.md#native-packages). The full installer also contains
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

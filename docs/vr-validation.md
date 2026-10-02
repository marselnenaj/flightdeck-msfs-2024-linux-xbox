# OpenXR integration validation

## What the implementation does

Flightdeck starts Wine directly so that the Xodus Store bridge retains its
inherited descriptors. Proton's Steam helper normally initializes VR registry
state; the direct launcher did not run it. An OpenXR runtime on Linux therefore
was insufficient by itself.

`flightdeck/vr.py` discovers native runtime manifests without loading their
libraries. Explicit checks and enabled game starts run `vr_probe.py` in a child
process, with bounded output and a 15-second deadline. The probe uses the
OpenXR loader, `XR_KHR_vulkan_enable`, the runtime's required Vulkan extensions
and its selected physical device. It returns required extensions, native
vendor/device IDs and the device UUID.

Before an enabled launch, Flightdeck creates a Windows manifest in the selected
runtime's `private/vr` directory and imports the required `HKCU\Software\Wine\VR`
state into that installation's Wine prefix. Registry import has a separate
20-second deadline. The game keeps the original Store descriptor map.

- Native `XR_RUNTIME_JSON` points at the selected Linux runtime.
- `WINEXR_RUNTIME_JSON` becomes `XR_RUNTIME_JSON` inside Wine and points at the
  Windows `wineopenxr.dll` manifest. Wine keeps native XR variables separately.
- `VR_PATHREG_OVERRIDE` preserves host OpenVR registration when Flightdeck's
  runtime script isolates the game's XDG directories.
- The compositor's UUID selects the DXGI adapter. Existing NVIDIA compatibility
  settings remain in effect. Conflicting manual selection is rejected.

This follows the contracts of the pinned
[Xodus Proton runner](https://github.com/xodus-gaming/Proton/tree/7c0b435495814349735c913fde78da906aecea52)
and Wine's OpenXR implementation. It does not replace the runner, invoke Steam's
game loader or change the host's active runtime registration.

## Evidence recorded for 0.1.18

On 2 October 2026:

- Native loader + Monado `045931d12f1cc9afde942f7905db08e6f51b9d8e`,
  `SIMULATED_ENABLE=1`, `XRT_COMPOSITOR_NULL=1`.
- AMD Radeon RX 6900 XT, native Vulkan vendor/device `1002:73bf`.
- An isolated Wine prefix with Flightdeck's runner and packaged graphics DLLs.
- The real Windows test `tests/graphics/openxr-stereo.c` verified that the
  Windows manifest is reachable after Wine's environment translation.
- Both DirectX 11 and DirectX 12 created an OpenXR session, imported a two-eye
  swapchain, obtained valid tracked views, cleared the GPU images, and submitted
  **eight stereo frames** to the native runtime. Both exited successfully.
- All four combinations passed: D3D11 and D3D12 with the runner's original
  renderer, and D3D11 and D3D12 with the packaged graphics corrections.
- The game-style private XDG directories were active for the Windows tests.
- Service tests cover discovery precedence, malformed manifests/settings,
  headset/driver failures, timeouts, cancellation, GPU conflicts, NVIDIA
  compatibility, Store environment preservation and concurrent-operation locks.
- Chromium tests exercise saved preferences, status polling, page reloads,
  successful and failed checks, busy states, translations and narrow layouts.

This is a real runtime and GPU integration test with simulated tracking. The
null compositor does not show an image in a physical headset. It does not
establish MSFS flight compatibility, physical tracking quality, streaming,
SteamVR device support, NVIDIA hardware performance or motion-controller input.

## Reproduce the Windows test

`scripts/check-vr.py` creates a new disposable prefix, compiles the Windows
test, calls the real Flightdeck preparation path and checks both DirectX APIs.
Pass `--bundle` to also test the hash-verified packaged graphics libraries:

```sh
python scripts/check-vr.py --runner /path/to/runner \
  --runtime-json /path/to/openxr_monado-dev.json --simulate \
  --bundle /path/to/graphics --output /tmp/new-flightdeck-vr-check
```

The output directory must not already exist. The script needs MinGW-w64,
OpenXR headers, a working native runtime, and access to the graphical session.
It stores individual logs and a machine-readable result in the output directory.

Use a native OpenXR runtime that is already working. For a hardware-free test,
build Monado with `XRT_BUILD_DRIVER_SIMULATED=ON`,
`XRT_MODULE_COMPOSITOR_NULL=ON` and `XRT_FEATURE_SERVICE=OFF`, then set
`SIMULATED_ENABLE=1`, `XRT_COMPOSITOR_NULL=1` and point `XR_RUNTIME_JSON` at its
build-tree `openxr_monado-dev.json`. The tested build also enabled the main
compositor because that revision links it in the in-process OpenXR target.
Install the normal Monado build dependencies, including Eigen and Vulkan
headers. No system runtime registration is needed for this test.

Compile the test with MinGW-w64 and Khronos OpenXR headers. Place the `openxr`
header directory in a separate include root, avoiding the host's entire
`/usr/include` in the Windows compiler's search path:

```sh
x86_64-w64-mingw32-gcc -O2 -Wall -Wextra -Wno-missing-field-initializers \
  -I/path/to/cross-headers tests/graphics/openxr-stereo.c \
  -o /path/to/check/openxr-stereo.exe -ld3d11 -ld3d12 -ldxgi -ldxguid
```

Prepare an **isolated disposable** runtime tree with `runner` pointing at the
Flightdeck runner, an initialized prefix in `local/msfs-prefix`, and
`private/vr-settings.json` containing `{"schema":1,"mode":"auto"}`. Use the
same trusted DXGI/D3D11/D3D12 DLLs and overrides as the game installation. Do not
use a running game's prefix. From the source checkout, call
`flightdeck.vr.prepare(test_runtime, environment)` and pass the returned
environment to the runner's Wine executable. Run `openxr-stereo.exe d3d11`,
then `openxr-stereo.exe d3d12`, with a 60-second outer timeout for each process.
The native `XR_RUNTIME_JSON` must select the intended test runtime before
preparation. A successful process prints `PASS` and exits zero.

For a headset acceptance test, connect the real device, disable simulated/null
mode, check the headset in Flightdeck, and run the intended MSFS edition.
Verify both eyes, head tracking, repeated VR entry/exit, a cockpit flight and
clean game shutdown. Record the headset, runtime, GPU, driver, simulator and
Flightdeck versions, and whether the NVIDIA compatibility or features mode was
used. A successful launcher check alone is insufficient for this acceptance.

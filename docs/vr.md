# Virtual Reality

[Deutsch](vr.de.md)

Flightdeck 0.1.18 adds optional OpenXR integration for MSFS 2024 and 2020.
Choose **Setup → Virtual Reality**, save a runtime, connect the headset, and
select **Check headset**. Start MSFS through Flightdeck and switch to VR in the
simulator with **Ctrl+Tab** (the default binding).

VR is off by default and is saved separately for each simulator installation.
The VR application and connected headset must be ready before each game launch.
Changing the setting takes effect at the next launch.

## Choose a runtime

| Setting | Use |
| --- | --- |
| Automatic | The active Linux OpenXR runtime, including an explicit `XR_RUNTIME_JSON` override. |
| WiVRn | An installed native WiVRn runtime, for supported standalone headsets such as Quest or Pico. |
| SteamVR | The OpenXR runtime from the installed SteamVR application. Start SteamVR and connect its headset first. |
| Monado | An installed Monado OpenXR runtime and a headset supported by its driver. |
| Off | Flightdeck does not prepare or check VR at game launch. |

Install the VR runtime and the **64-bit Linux OpenXR loader** through your
distribution or the runtime's official installation instructions. Flightdeck
does not install drivers, headset firmware or streaming software. See
[WiVRn](https://github.com/WiVRn/WiVRn),
[SteamVR for Linux](https://github.com/ValveSoftware/SteamVR-for-Linux), and
[Monado](https://monado.freedesktop.org/).

Automatic selection follows the active runtime registration. With multiple
installed runtimes and no active registration, select one explicitly. A broken
active registration or explicit override is reported instead of silently
switching to another provider. Runtime manifests must refer to libraries
accessible from the host: a manifest referencing a Flatpak-only `/app` library
cannot be loaded by the native game process. Use the runtime's supported host
integration or native package in that case.

## AMD and NVIDIA

Both vendors use the same OpenXR/Vulkan bridge. Flightdeck queries the VR
compositor's graphics device and keeps the game's DXGI selection on that GPU.
This matters on laptops and systems with several GPUs. Remove manual GPU name
or index filters before enabling VR; an incompatible explicit UUID is rejected.

On NVIDIA, begin with Flightdeck's **Automatic** graphics setting. Its existing
compatibility mode disables NVAPI, DLSS, Reflex and NVIDIA Frame Generation.
The experimental NVIDIA features setting is not a prerequisite for VR.
[Normal NVIDIA rendering](graphics.md) is confirmed by user testing; VR needs
separate validation. Runtime, driver, headset and encoder support still determine
which combinations work; OpenXR alone does not guarantee wireless performance.

## Checks and troubleshooting

- **Runtime not found:** start the intended VR application, select its OpenXR
  runtime there, then check again. Choosing SteamVR in Flightdeck does not
  download or start SteamVR.
- **No headset available:** connect or wake the headset and finish its connection
  in the VR application before checking or starting MSFS.
- **Loader missing:** install the 64-bit OpenXR loader and Vulkan drivers for
  your distribution. A successful desktop game launch alone does not prove the
  OpenXR loader is installed.
- **Check timed out:** restart the VR application and retry. Driver probing runs
  in a separate process with a deadline; it cannot hold the launcher indefinitely.
- **Runner components missing:** use the runner installed by Flightdeck.
- **Different graphics devices:** use the same GPU for the VR compositor and
  game. Remove custom `DXVK_FILTER_DEVICE_NAME`, `VKD3D_FILTER_DEVICE_NAME` and
  `VKD3D_VULKAN_DEVICE` overrides; align any `DXVK_FILTER_DEVICE_UUID` override.
- **Check succeeds but no VR picture:** confirm the simulator's VR binding and
  settings, then test a native OpenXR application with the same runtime. The
  launcher check establishes headset and GPU discovery, not a successful flight.

Saving settings and checking the headset require an idle installation. They are
blocked during play, cloud synchronization and setup. An enabled VR launch stops
with an actionable error if preparation fails; it does not silently fall back
to monitor mode. Select **Off** to return to the ordinary launch path.

Diagnostics include the selected mode and the last check outcome for the current
launcher session. Raw driver output, runtime paths and device UUIDs are excluded.

## Verified scope

The integration has been tested with a simulated Monado headset on an AMD
Radeon RX 6900 XT: native OpenXR discovery, Windows manifest handoff, D3D11 and
D3D12 session creation, imported stereo swapchains, tracked views and submitted
frames through Flightdeck's Wine runner. Automated checks also cover provider
selection, NVIDIA-compatible preparation, failure handling, service locking and
the German/English UI. [Reproduction and test limits](vr-validation.md).

Physical headset operation, NVIDIA hardware, WiVRn streaming, SteamVR headsets,
motion controllers, comfort, latency and an actual MSFS VR flight have **not**
been validated for this release. The integration is available for those setups;
they are not presented as a verified compatibility matrix.

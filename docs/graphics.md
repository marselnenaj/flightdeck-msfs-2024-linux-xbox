# NVIDIA graphics

[Deutsch](graphics.de.md)

**Flightdeck 0.1.11** uses compatibility settings automatically on NVIDIA.
It disables NVIDIA Low Latency in both DirectX 11 and 12 and corrects DXVK's
previously ignored opt-out. The existing VKD3D multiwindow corrections are
included. This corrects the incomplete compatibility setup; resolution of the
black MSFS main view still needs confirmation on NVIDIA hardware.
[Build and validation](nvidia-renderer.md).

Update through **Updates → Flightdeck**, finish the update and restart the
simulator. Existing **Automatic** settings adopt the new profile. Reinstalling
MSFS, resetting the Wine environment or adding launch parameters is unnecessary.

Flightdeck uses the graphics components supplied with its runner and the NVIDIA
driver installed on Linux. A working Vulkan driver is required. Use your
distribution's recommended driver and restart Linux after changing it.
Flightdeck does not install system drivers. On systems with one discrete NVIDIA
card, it selects the same physical card for DXGI and DirectX 12 using a stable
device ID, including when the NVIDIA vendor is hidden.

## Choose a mode

1. Select the simulator and close any running game or setup.
2. Open **Setup → NVIDIA graphics**.
3. Select a mode, choose **Save mode**, then start the simulator.

| Mode | Behavior |
| :--- | :--- |
| **Automatic (prefer compatibility)** | Uses the compatibility profile described below. This is the default, including after upgrading an existing automatic preference. |
| **Compatibility (without NVIDIA features)** | Disables NVAPI, optical flow, NGX and NVIDIA Low Latency in DXVK/VKD3D. Hides NVIDIA vendor identification from Wine/DXGI. DLSS, Reflex and NVIDIA Frame Generation are unavailable. |
| **NVIDIA features (experimental)** | Enables the runner's NVIDIA components and available NGX from the installed driver. This is the previous automatic behavior. DLSS requires matching driver components; inherited disable settings still apply. |

The choice is saved per installation and applies on the next game start without
a launcher service restart. Switching to **NVIDIA features (experimental)**
restores feature availability on the next start. Compatibility does not remove
DLLs, change drivers or select integrated graphics. AMD/Intel-only systems do
not display these controls and keep their existing graphics setup.

The full installer replaces only recognized graphics DLLs with the matched
bundle. Custom renderer files remain untouched. The source-only package does
not contain the rebuilt libraries; use the full installer for this correction.

## Remaining limitation

Working menus with a black globe, free-flight map or primary cockpit view remain
a reported NVIDIA issue. Earlier releases' modes did not resolve every case.
A working second render window does not establish that the primary view is fixed.
See [known issues](known-issues.md) and the
[Steam compatibility reference](nvidia-steam-parity.md).

[Flightdeck 0.1.17](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.17)
is a prerelease for testing a targeted 3D-texture layout correction. Its effect
on this issue remains unverified. Install its full package manually to test;
it is not offered through **Updates → Flightdeck**. Follow the
[test and restoration instructions](nvidia-renderer.md#3d-texture-layout-correction-0117-prerelease).

The local rendering regression covers mixed DirectX 11/12 devices, primary and
secondary swapchains, resize and destruction. It passed on AMD hardware and does
not establish NVIDIA driver behavior or MSFS flight stability.

Custom configuration is documented in the
[runtime reference](runtime.md#nvidia-graphics-in-launcher-managed-starts).

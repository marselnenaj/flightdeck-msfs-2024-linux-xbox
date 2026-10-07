# NVIDIA graphics

[Deutsch](graphics.de.md)

**NVIDIA operation is confirmed by user testing.** MSFS 2024's main view now
works in reported setups. Smaller issues can remain, including a missing or
black video during startup. This confirmation concerns normal monitor rendering;
it does not qualify every GPU/driver combination, VR, DLSS or Frame Generation.

Use the default **Automatic** mode and keep Flightdeck updated through
**Updates → Flightdeck**. Close and restart the simulator after changing modes.
[Current limitations](known-issues.md) · [Technical renderer history](nvidia-renderer.md)

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

Flightdeck also reconciles MSFS 2024's saved graphics options
before launch. Saved DLSS selects TAA while NVIDIA features are disabled;
saved Reflex and NVIDIA frame generation are switched off, including their VR
settings. Their original values are retained in the Wine profile. Switching
back to Features restores each value only if it still matches Flightdeck's
change. FSR, other frame generators, display/quality settings and later manual
changes are retained. Unsupported, ambiguous or externally linked configuration
files are left intact.

Both simulator editions start with `-FastLaunch` to skip the intro path.
Video playback and the main 3D scene are separate checks: a missing startup
video does not by itself mean that NVIDIA rendering has failed.

The full installer replaces only recognized graphics DLLs with the matched
bundle. Custom renderer files remain untouched. The source-only package does
not contain the rebuilt libraries; use the full installer for this correction.

## Remaining issues and reporting

The startup video may be missing or black even when the menu, map and cockpit
render normally. Driver-specific problems can still occur. If the main 3D view
also stays black, report that separately with the Flightdeck version, GPU,
driver, selected graphics mode and a [diagnostic report](problem-reports.md).

Earlier releases had black-main-view reports. Their detailed investigations and
isolated renderer tests remain in the [technical history](nvidia-renderer.md);
they are not the current general compatibility status. Local renderer tests on
AMD do not substitute for NVIDIA user testing.

For custom settings, see the
[runtime reference](runtime.md#nvidia-graphics-in-launcher-managed-starts).

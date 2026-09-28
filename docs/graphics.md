# NVIDIA graphics

[Deutsch](graphics.de.md)

Flightdeck prepares NVIDIA graphics for both MSFS 2024 and MSFS 2020. It uses
the graphics components supplied with its runner and the NVIDIA driver installed
on Linux. On systems with one discrete NVIDIA card and integrated graphics, it
selects the NVIDIA card consistently for DXGI and DirectX 12.

A working Vulkan driver is required. Install the recommended NVIDIA driver
through your distribution's software manager and restart Linux after changing
drivers. DLSS also requires the driver's NGX components. Flightdeck does not
install system drivers, and a detected GPU alone does not confirm that the
simulator can render correctly.

NVIDIA launch integration is implemented. Rendering and flight stability have
not yet been verified across NVIDIA hardware and driver versions. Successful
Steam/Proton reports are useful compatibility evidence, but do not validate
Flightdeck's separate Xbox PC launch path.

**Flightdeck 0.1.7** corrects an adapter-selection problem: Wine can report
a different GPU name from Linux, causing the earlier automatic name filter to
exclude the selected card. Selection now uses the card's stable device ID,
which also works when Compatibility mode hides NVIDIA-specific identification.
The public 0.1.6 release does not contain this correction.

## Choose a mode

The mode selector is available in **Flightdeck 0.1.6 or later**. Update through
**Updates → Flightdeck** if it is missing. See the [release history](changelog.md).

1. Select the simulator in Flightdeck and finish any running game or setup.
2. Open **Setup → NVIDIA graphics**.
3. Select a mode and choose **Save mode**, then start the simulator.

| Mode | Behavior |
| :--- | :--- |
| **Automatic (enable NVIDIA features)** | Prepares NVAPI and optical flow from the runner and NGX from the installed driver, when available. Explicit environment overrides remain effective. |
| **Compatibility (without NVIDIA features)** | Disables NVAPI, optical flow and NGX for the game process and hides NVIDIA-specific vendor identification from Wine and DXGI. Try this for black scenery or NVIDIA-related crashes. DLSS and NVIDIA Frame Generation are unavailable in this mode. |

The choice is saved separately for each installation and applies on the next
game start. You do not need to restart Flightdeck or recreate the Wine
environment. Switching back to Automatic restores the normal setup on the next
start. The compatibility setting does not change system drivers, remove graphics
libraries or switch rendering to integrated graphics.

This mode is based on an
[upstream MSFS 2024 compatibility report](https://github.com/ValveSoftware/Proton/issues/9641)
and is a troubleshooting option, not a guaranteed remedy for every black screen.
AMD/Intel-only systems do not display the NVIDIA controls.

## If rendering still fails

A black 3D scene with working menus has been reported on Linux Mint 22.3 with
an RTX 5060 Ti and NVIDIA 580.178.04 in both graphics modes. Creating a second
render window briefly displayed the scene but subsequently caused crashes.
The 0.1.7 adapter-selection correction needs to be tested on this system;
successful in-game rendering has not yet been confirmed.

Run the same simulator scene once in each mode. In **Diagnostics**, load and
download a report after each attempt. Include the Flightdeck version, simulator
edition, GPU, driver version and a short description of when rendering fails.
The report includes available Vulkan adapters and the settings requested for the
last launcher-managed start; it does not certify successful rendering.
In 0.1.7, it also includes detected renderer versions, filtered-out adapters and
counts of presents without rendering from the available log excerpt. A
present without rendering is an observation, not a diagnosis of a faulty driver.

Checking game files repairs the base game. Resetting the Wine environment creates
a new profile. Neither replaces graphics drivers or establishes that a rendering
problem has been fixed. Try the graphics settings before resetting an otherwise
working installation.

For custom runtime configuration and environment-variable behavior, see the
[runtime reference](runtime.md#nvidia-graphics-in-launcher-managed-starts).

# MobiFlight and WinWing

[Deutsche Anleitung](mobiflight.de.md)

The experimental **Mods → MobiFlight / WinWing** integration uses the selected
Xbox or Steam MSFS 2020/2024 profile and its Proton runner. It downloads the
official portable MobiFlight Connector **11.3.1**, verifies its size and SHA-256,
and includes the application's Windows .NET runtime. No extra permanent Wine
profile or Linux .NET installation is needed.

The automatic download uses MobiFlight's public release ZIP; you do not need to
select a separate installer file. If the app download finishes but WASM/event
setup fails, choose **Prepare / repair MobiFlight**. Flightdeck resumes the
existing installation and retains your settings instead of downloading the app again.
If Fenix or a Proton switch has replaced the Windows profile since the interrupted
attempt, Flightdeck archives the old setup journal and retains its temporary files;
repair starts a new attempt in the selected profile.

1. Select the correct simulator installation and close MSFS and its companion apps.
2. Choose **Install MobiFlight**, or **Prepare / repair MobiFlight** for an existing app.
3. Open MobiFlight and select your hardware and configuration. Verify its simulator
   connection during a flight. Optional **Start MobiFlight with MSFS** manages only
   this profile's startup entry.

Preparation installs the official WASM module into the actual Community folder
and obtains missing event definitions from MobiFlight's official HubHop service.
The current HubHop export address is used directly, without following redirects.
Existing modules, event definitions and custom `events.user.txt` files are retained.
An unknown module remains under your control. Flightdeck never activates
input-remapper presets. A preset with exclusive access can hide a device from Wine;
check your active presets when a device is missing.

The HID short-write compatibility fix is built from the exact official HidSharp
2.6.4 assembly. Flightdeck retains the original DLL and atomically replaces only
the supported hash. Unknown DLL versions are reported and preserved. The patch
builder, original package pins, license and changes are documented in
[compat/mobiflight](../compat/mobiflight/README.md).

For the exact verified Xodus/Fenix `winebus.sys`, preparation merges four WinWing
device IDs into the existing `EnableHidraw` list after saving a registry backup.
Other allowed devices and explicit disable settings are preserved. A different
Winebus build is reported as unverified and is not changed automatically. This
does not qualify SimAppPro, firmware flashing, every WinWing model or full-flight
hardware behavior; physical-device testing remains necessary.

## Linux device access

The status checks connected WinWing hidraw devices for read/write permission.
If permission is missing, add this narrow rule using your distribution's normal
administrator editor as `/etc/udev/rules.d/70-flightdeck-winwing.rules`:

```udev
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="4098", TAG+="uaccess"
```

Then reload the rules with `sudo udevadm control --reload-rules`, unplug and
reconnect the device, and refresh Flightdeck's status. The rule grants access to
the active local seat through logind; it does not make all input devices globally
writable. Flightdeck currently diagnoses permissions without making privileged
system changes. Avoid broad `MODE="0666"` rules or adding your account to the
`input` group just for this integration.

Closing the selected simulator ends only its enabled MobiFlight companion.
Closing Flightdeck leaves an active flight running. Reopen the add-on status
after switching Proton so actions use the current profile generation.

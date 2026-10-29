# MobiFlight HID compatibility payload

Flightdeck downloads the original, hash-pinned **MobiFlight Connector 11.3.1**
portable archive directly from its official GitHub release. It contains the
Windows .NET 10.0.12 Desktop runtime; users do not need a Linux .NET SDK or a
new Wine profile. MobiFlight itself is MIT licensed:
https://github.com/MobiFlight/MobiFlight-Connector/tree/11.3.1

The bundled HidSharp 2.6.4 DLL is the `netstandard2.0` assembly from the official
NuGet package. Its license and copyright notice are reproduced in
`HidSharp-LICENSE.txt`. Flightdeck's separately built DLL is a **modified** copy:
after a successful Windows HID write it counts the requested output report size
(clamped to the device's maximum) rather than Wine's incorrect short write count.
This addresses the same Wine behavior corrected by upstream commit
135ae829199108e7168662c740fc076b0d841bce (Wine bug 59891). It does not change reads,
feature reports, device enumeration, or the selected Wine runner's libraries.

`Program.cs` is Flightdeck's own MIT-licensed maintainer-only patch builder. It
uses Mono.Cecil 0.11.6 from its official NuGet package. It never loads or runs the
subject DLL. The original SHA-256, exact method body, existing `Math.Min(int,int)`
MemberRef, local types and instruction count must match. The builder reuses the
original `netstandard` reference; it does **not** import its host SDK's CoreLib.
It verifies that all other method instructions, locals and exception handlers
are unchanged and that the assembly references are identical. Cecil rewrites
metadata and the deterministic module identity; raw DLL offsets therefore differ.

The `flightdeck-tools mobiflight-build` maintainer command takes `--sources`, `--work`,
`--output` and `--dotnet`. `--download` obtains only the two pinned NuGet packages.
It builds this reviewed source in a new disposable work folder, patches the
official original twice, requires byte-identical results and checks the final
SHA-256 against `../mobiflight.lock.json`. Use `--output build/mobiflight` when
preparing launcher resources. No NuGet restore feed or system installation is
used. A .NET 10 SDK is needed only on the maintainer/CI build machine.

The release resource is `mobiflight/HidSharp.dll`, with this notice and the
HidSharp license. No DLL supplied in the community patch is redistributed.
Runtime application accepts only the exact original or the exact already-fixed
DLL and saves the original before atomic replacement. Unknown/newer DLLs are
left untouched; no guessed patch is applied to future versions.

MobiFlight's bundled WASM module is installed only into the selected simulator's
actual Community folder when no module exists. The official 11.3.1 updater
(`SimConnectMSFS/WasmModuleUpdater.cs`, lines 101–117 and 151–166) uses this same
module for MSFS 2020 and 2024. Existing modules and event/user configuration are
preserved. The portable archive lacks the generated `modules/events.txt`.
Flightdeck fetches this text data from the same official HubHop HTTPS endpoint
used by MobiFlight (updater lines 320–328), with no redirects, a 4 MiB bound and
text/record validation. These event definitions change over time and are not
treated as a hash-pinned executable. Only missing events in the exact known
module may be added; existing events and `events.user.txt` are never replaced.
The new module's layout size is updated before atomic publication. Unknown or
symlinked existing modules remain under the user's control.

WinWing's four tested VID:PID pairs are merged into `EnableHidraw` only when
the selected runner has the exact supported `winebus.sys` hash in the lock.
That file was independently extracted from the SHA-256-verified original Xodus
runner archive pinned in Flightdeck's bootstrap lock. Its unstripped PE function
`is_hidraw_enabled` (VA 0x180005a60) confirms the disable guard, exact device list
lookup via `NtQueryValueKey`, and `_wcsnicmp` matching; its behavior also agrees
with the pinned Wine fork sources and upstream Wine. A Windows registry export
is preserved before a change. Unrelated allowed devices and explicit global or
per-device disable preferences are preserved. Unknown runner DLLs are reported,
never guessed from a version label. Only idle, selected profiles may be changed;
Wine services are then stopped to reload the device list on next launch.

Hardware and in-simulator operation still need verification on actual devices.
No input-remapper presets are activated. Linux device permissions are diagnosed
read-only; Flightdeck provides the canonical uaccess rule and instructions for
an administrator to install it. It does not install a root helper or udev rule.

# Native launcher status

The **[0.2.1 release](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.2.1)** runs the Flightdeck backend, installer,
updater and runtime helpers in Rust. It retains the local HTML/CSS/JavaScript
interface and the established C/C++ Wine/Store ABI components. Version 0.1.22
is the Python transition release and offers 0.2.1 through its normal updater.
Older launchers receive 0.1.22 first, then 0.2.1 after restarting and checking
again. Direct installation of the full native package also works. The earlier
0.2.0-dev.1 prerelease remains available as a historical manual preview.

## Implemented contracts

| Area | Native implementation |
| --- | --- |
| Desktop and API | Loopback HTTP service, origin/session checks, embedded UI, language selection, service reuse and update handoff |
| Setup | Prerequisite checks, pinned component downloads, official Microsoft sign-in through Xodus, installation/import, progress, pause and resume |
| Runtime | Exclusive leases, owned process supervision, component refresh, portable licensed game loader, graphics and VR setup |
| Add-ons | Community discovery, Fenix setup/restore/repair, display helpers, GSX/FSDT preparation and Proton switching with retained profiles |
| .NET | Both registry views and CLR architectures, managed compiler probes during repair, staged automatic repair before Fenix/MSFS startup, transaction recovery |
| Saves | Exact XDLOCAL1 interchange, bounded connected-storage protocol, account-bound cloud operations, backups, conflict decisions and uncertain-upload recovery |
| Maintenance | Game update/repair/rollback, launcher update/rollback/uninstall, inventory-bound previews and recoverable filesystem transactions |
| Diagnostics | Bounded log analysis, Store checks, filtered exports and local problem-report drafts |

Installed native packages contain no Python application code and do not call a
Python interpreter. Thin shell entry points invoke the Rust executable. A native
runtime helper is installed before the corresponding managed wrappers change.
An explicit rollback to an older Python release still needs that release's
Python runtime; normal native operation does not.
The Python **0.1.22 transition update** accepts native packages and
hands the existing browser session to the native service. The in-app path is
**0.1.21 → 0.1.22 → 0.2.1**. Older updaters only accept the
Python source layout, so the [staged release procedure](rust-transition.md)
keeps their discovery endpoint on the bridge. Direct installation with the new
full package also works. Native-to-native updates use the Rust updater. Update
and rollback preserve the installed release selection and browser origin.

The Python source remains in the repository as a compatibility reference and
test oracle, and supplies this transitional release. Python also runs build,
packaging and integration-test tools. The
proprietary Fenix application remains a Windows .NET application: changing the
launcher's implementation language does not remove that prerequisite.

## Compatibility and validation

The port preserves the HTTP/UI contract, private runtime layout, game identities,
save format and existing transaction journals. Known managed files can be
updated while idle; custom files are retained. Restoring a launcher version does
not roll back game files, account state or saves.

Validation includes native filesystem/concurrency/transaction tests, differential
Python/Rust save tests, HTTP contract checks and the Chromium UI suite running
against the native service. The installer is exercised with Python absent from
PATH. Separate account-free Wine checks cover:

- Fresh .NET installation, repair after removing the x86 CLR, restored file
  hashes and an idempotent restart, using the pinned Flightdeck runner.
- Experimental 11.0 (20260924) and CachyOS Proton 10.0 sunset: relative DLL loads
  from the licensed temporary game view, Store bridge loading, D3D11/D3D12
  rendering, retained profile markers and return to the default runner.
- Default Flightdeck runner through Python and Rust: automatic `-FastLaunch`
  reaches the Windows process, mapped DLLs load from the working and module
  directories, and mixed D3D11/D3D12 rendering succeeds. The default DLL probe
  fails with error 193 before the loader correction.

Later CachyOS multiwindow-renderer repetitions timed out twice, then succeeded
in a Wine virtual desktop and again after returning to the host compositor.
This remains an intermittent synthetic-test issue; it does not establish a
virtual-desktop requirement or reproduce the NVIDIA MSFS failure. See the
[NVIDIA investigation](nvidia-renderer.md#default-runner-dll-mapping-correction).

These checks reproduce the loader defect behind a plausible MSFS menu failure
and verify its correction. They do not establish that the reported menu failure
is resolved in the affected user's game. Full Fenix/GSX flights, real cloud
account operations with this native build and performance inside MSFS still
need live validation. The exact remote .NET failure has not been reproduced
from that user's profile.

See [BUILDING.md](../BUILDING.md) for reproducible checks and packaging, and
[launcher performance](performance.md) for the Python/Rust comparison.

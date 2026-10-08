# Native launcher status

The stable **[0.2.7 release](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.2.7)** runs the Flightdeck desktop, backend,
installer, updater and runtime helpers in Rust. It retains the established
C/C++ Wine/Store ABI components. Version 0.1.22 is the Python transition release
and can discover 0.2.7 through its normal updater.
Users of 0.1.21 or earlier need the full native installer or an explicit 0.1.22
bridge installation; the current Latest endpoint no longer offers that bridge.
The superseded
0.2.0-dev.1 preview and withdrawn 0.2.0 release entries have been removed;
Native versions 0.2.1–0.2.6 update through their existing updater.
The previous stable release remains available for reproducibility and rollback.

Version **0.2.3** introduced the desktop migration with a native Rust
interface for all six views. It uses the same artwork and font, real local API
commands, existing installation records and update handoff. The old web UI is retained in Git history, not in the source or installed package. This release also
persists Fenix .NET compatibility settings in the Wine profile, waits for detached
installer children and checks install-hook failures. See [native desktop and
Fenix verification](native-ui.md).

## Implemented contracts

| Area | Native implementation |
| --- | --- |
| Desktop and API | Loopback HTTP service, origin/session checks, native iced UI, persisted language selection, service reuse and update handoff |
| Setup | Prerequisite checks, pinned component downloads, official Microsoft sign-in through Xodus, installation/import, progress, pause and resume |
| Runtime | Exclusive leases, owned process supervision, component refresh, portable licensed game loader, graphics and VR setup |
| Add-ons | Community discovery and reviewed per-package uninstallation, Fenix setup/restore/repair, display helpers, GSX/FSDT preparation and Proton switching with retained profiles |
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
hands the existing local service endpoint to the native launcher. The explicit bridge path is
**0.1.21 → install 0.1.22 → in-app update to 0.2.7**. Older updaters only accept the
Python source layout; see the [transition instructions](rust-transition.md).
Direct installation with the new
full package also works. Native-to-native updates use the Rust updater. Update
and rollback preserve the installed release selection and local service endpoint.
The native UI reconnects only after verifying a replacement service record;
update restart hands control to the new executable and closes the old window.

The current branch has removed the Python application, its installer, pip
metadata and duplicated tests. Rust tests consume frozen synthetic Python
results in `tests/fixtures/legacy-python/`, including the producing commit and
source hashes. Migration and benchmark harnesses take explicit historical
packages or checkouts; they cannot import an application from the current tree.
Python remains only in maintainer build, packaging and test tools. The
proprietary Fenix application remains a Windows .NET application: changing the
launcher's implementation language does not remove that prerequisite.

## Compatibility and validation

The port preserves the HTTP/UI contract, private runtime layout, game identities,
save format and existing transaction journals. Known managed files can be
updated while idle; custom files are retained. Restoring a launcher version does
not roll back game files, account state or saves.

Validation includes native filesystem/concurrency/transaction tests, frozen
legacy save/.NET/settings/diagnostic contracts, HTTP contract checks, native UI
event/rendering tests and the Rust client running against the real local service.
The historical web API fixture is retained for native comparison captures. The installer is exercised with Python absent from
PATH. Separate account-free Wine checks cover:

- Fresh .NET installation, repair after removing the x86 CLR, restored file
  hashes and an idempotent restart, using the pinned Flightdeck runner.
- Experimental 11.0 (20260924) and CachyOS Proton 10.0 sunset: relative DLL loads
  from the licensed temporary game view, Store bridge loading, D3D11/D3D12
  rendering, retained profile markers and return to the default runner.
- Default Flightdeck runner through the historical Python and current Rust builds: automatic `-FastLaunch`
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

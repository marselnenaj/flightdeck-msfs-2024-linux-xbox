# Follow-up to the 0.1.8 tester reports

This is a historical investigation note. The current support status is in
[known issues](known-issues.md). The exporter instructions below remain a tool
reference, not a request for affected users to repeat diagnostic submissions.

The remaining Marketplace and NVIDIA/audio symptoms are **not confirmed fixed**.
The first tester's screenshot says **“Marketplace-Sitzung abgelaufen”**
(Marketplace session expired). This is the simulator's message; it does not
establish a timeout in Flightdeck's purchase window.

Both reports contain the expected 0.1.8 native component hashes. The AMD run's
inventory request completed successfully after 5.169 seconds, followed by
successful game-license, license-token and package-update queries. The earlier
five-second IPC failure is absent from these observed calls. Successful Store
calls do not establish successful sign-in to the game's Marketplace services.
The AMD cloud snapshot was taken about six seconds after exit, while sync was
still running. It establishes neither a final sync failure nor completion.

The RTX 4060 run requested the NVIDIA UUID and reported four UUID-filter skips.
Those skips alone do not identify the selected adapter or demonstrate a filter
failure. Its earlier Store window check was cancelled, and its cloud mode was
local. No audio evidence was included. Neither Vulkan enumeration nor a zero
exit status establishes successful rendering or audible playback.

## Diagnostic correction included in 0.1.9

Schema 5 scans through the game and service logs using bounded chunks, up to
64 MiB or three seconds per scan, plus the final 512 KiB when capped. This
replaces the previous first-1-MiB/last-512-KiB analysis in the normal report.
`log_coverage` and the Store timeline's `coverage` disclose bytes read, omitted
bytes, oversized lines and files that changed during reading. More than 256
distinct outcomes per summary category set `summary_limited`; the Store
timeline retains its existing 256-event limit and `partial` indicator.

The report adds existing XUser token/signature HRESULTs, endpoint-policy cache
stages, signing-policy decisions and networking-policy results. Hostnames are
reduced to fixed service categories. Audio/media errors and available warnings
are counted by fixed Wine channels; numeric renderer failures also contribute
to renderer error counts even if no Vulkan symbolic error is printed. Native
signal exits are retained. No raw messages, URLs, tokens, account identifiers,
request bodies or device identifiers are exported.

New starts enable warnings for the selected Wine audio channels. Reports made
from old logs can only include warnings that were actually logged. Empty audio
or graphics error lists are not a successful playback/rendering test.

## Re-export an existing tester run

The read-only tool works with existing 0.1.8 logs, without installing a new
Flightdeck release, starting the game or connecting to Microsoft:

```sh
flightdeck diagnose-run --output flightdeck-session.json
```

It reads the selected runtime from the normal Flightdeck state directory. With
a custom state location or to select an older run explicitly:

```sh
flightdeck diagnose-run --runtime /path/to/runtime --output flightdeck-session.json
flightdeck diagnose-run --run /path/to/private/run-YYYYMMDD-HHMMSS-XXXXXX --output flightdeck-session.json
```

Use a new output filename. Send the JSON, the exact visible symptom and whether
it occurred before entering Marketplace, on opening it, or during a purchase.
For NVIDIA, distinguish a black 3D scene with working menus from a stalled
loading screen, and record whether audio is also absent in the menu. The JSON
does not collect live cloud state; use the normal launcher report after sync
finishes for that part of the investigation.

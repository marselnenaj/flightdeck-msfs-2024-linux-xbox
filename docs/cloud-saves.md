# Xbox cloud saves

[Deutsch](cloud-saves.de.md)

Flightdeck uses Xbox cloud saves by default when you start MSFS from the launcher.
It loads the cloud state before starting the simulator, keeps local backups,
and uploads changes after the game exits. Cloud support remains experimental.
The Xbox profile signed into the game determines which saves are used.

With Flightdeck 0.1.1's edition selection, the selected runtime determines the
game's cloud identity, helper profile and local backups. MSFS 2020 and MSFS 2024
saves are not interchangeable. The new 2020 path still needs end-to-end gameplay
validation; see [release status](changelog.md).

## Just start the simulator

1. Install the current full Flightdeck package and select **Start simulator**.
2. Flightdeck backs up the local saves and checks the cloud. On first use, an
   existing cloud state is used; an empty cloud preserves existing local saves.
3. The simulator starts automatically when the comparison is complete.
4. After you exit, Flightdeck backs up and uploads the local state, then reads
   it back before showing **Your saves are synchronized with Xbox cloud**.

You do not need to select a direction or download a copy for each session.
The local save provider is enabled automatically. Opening the launcher or its
Saves page does not itself transfer data; the normal start action begins the
workflow. Closing the launcher window leaves its local background service
running so an owned game session can finish syncing.

Later starts check the current cloud index and download only changed save
containers. Unchanged containers use a locally hash-verified copy tied to the
same Xbox profile and exact cloud revision. A missing or damaged copy is
downloaded again. Every written container is downloaded in full to verify the
upload, even when its reported revision is unchanged. Other containers still
require matching live revisions, with the complete index checked before and
after each read.

Flightdeck also retains its separate cloud helper's Wine environment after a
clean shutdown. This avoids repeating its initialization for each sync. The
Xbox account and game identity are checked anew for every connection.

## When Flightdeck needs a decision

A previous verified transfer provides a common comparison state. Changes on
only one side are applied automatically. Independent changes in different
containers can merge. If both sides changed the same container differently,
Flightdeck asks whether to use the cloud or local version. It does not pick
one based on timestamps. The preview and choice are tied to the exact compared
versions; changed or expired comparisons must be checked again.

If sign-in or the connection is unavailable, choose **Try again** or explicitly
start with local saves. This also lets you open MSFS for its first Xbox sign-in.
Local sessions are backed up; their pending changes are checked at the next
normal start. An authentication failure is never treated as an empty cloud.
Do not run the same game profile on another device during a session: changes
there can require a decision before the final upload.

## Backups and interruptions

Flightdeck keeps backups before play, before local replacements and before
uploads. The runtime is reserved throughout comparison, the game session and
upload, excluding another launch or maintenance operation. The native save
writer is also locked during transfers.

Uploads use the normal Xbox storage lock, never a forced takeover. A single
container update is atomic; the whole collection is not. A connection loss can
leave some containers updated. The durable session record and saved target
remain available; the next attempt reads the server again. It never blindly
replays a request or silently replaces pending progress with the cloud.

If the launcher or its game supervisor is forcibly killed, Flightdeck may
require a Linux restart before another automatic session. This prevents save
changes while an orphaned game process might still be running. A normal game
exit and the launcher's Stop button do not require a restart.

## Advanced tools

**Saves → Advanced options** contains manual comparison, import, upload and
separate cloud-copy downloads. These are optional recovery tools. A manual
transfer replaces the complete selected profile's save collection, including
confirmed removals. Empty-source warnings are shown before that choice.
**Undo last import** restores the prior local state during the current launcher
session, provided the game has not changed it since; the cloud is unaffected.

## Private data

All local copies remain in the selected runtime:

| Location | Contents |
| --- | --- |
| `private/cloud-saves/` | Separate cloud snapshots, including the cloud state before an upload |
| `private/cloud-cache/` | Profile-bound references to verified snapshots for faster starts |
| `private/cloud-helper-prefixes/` | Separate version-bound Wine environment for the cloud helper |
| `private/cloud-import-backups/` | Local saves before imports and undo operations |
| `private/cloud-import-receipts/` | Records used to validate previous completed transfers |
| `private/local-saves/` | Active local saves and their comparison baseline |
| `private/save-backups/` | Local backups before and after game sessions |
| `private/cloud-sessions/` | Account-bound pending transfers and recovery references |
| `private/cloud-offline.pending` | Protects unconfirmed progress, including account changes during play |
| `private/connected-storage-device.seed` | Random local identifier for normal cloud-lock ownership |

These files contain private data and must not be included in issues, source
exports or release archives. Xbox tokens, signatures and temporary upload URLs
stay inside the native helper and are not exposed to the launcher page.
Do not manually copy downloaded blobs over `state.bin`; the formats differ.

When relocating an existing runtime, keep its complete `private/` directory
together and finish the game and cloud transfer first. A copied
`local-saves/` directory alone can contain baseline references whose receipts
and backups still live in the original runtime. Flightdeck cannot validate that
comparison state until those matching records are present. Preserve their exact
bytes and private permissions; downloaded cloud snapshots cannot replace them.

## Verification

Synthetic tests cover import/undo, account binding, filesystem races, lease
loss, partial uploads, cancellation, readback, restart-safe comparison records,
automatic start/exit, interrupted sessions, and the German/English browser workflow. A real MSFS profile was also used to
verify a complete cloud download, normal lock ownership, and upload, readback
and deletion of a separate 32-byte test container. The test container was
removed; all existing cloud and local saves remained unchanged. A Windows/Xbox
to Linux and back gameplay test remains necessary before treating the feature
as stable.

The implementation is original. Wire-contract observations and source references
are documented in [Connected Storage protocol](connected-storage-protocol.md).
Universal Title Storage is a different service and is not used as a fallback.

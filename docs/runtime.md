# Local runtime contract

For a first installation, use the full Flightdeck installer and choose **Install
MSFS** in the launcher. It downloads the owned game and prepares Wine/Proton
automatically after the Linux prerequisite checks. Follow the
[installation guide](install.md); no existing game, runner or Wine prefix is
required for that workflow.

This page covers the advanced import and developer staging of existing files.
The launcher accepts a prepared runtime directory. Its required entry point is
`tools/play-msfs.sh`; the game is at `games/MSFS2024/FlightSimulator2024.exe`, the
isolated prefix at `local/msfs-prefix`, and optional local saves at
`private/local-saves`. The encrypted package's `.xodus-streaming.msixvc` and
MicrosoftGame.Config must remain alongside the game. Xodus obtains and checks
the user's actual entitlement; the compatibility code does not grant licenses.

## Advanced: prepare a runtime from existing files

The installed launcher provides the same preparation logic under **Einrichtung
→ Neue Runtime vorbereiten**. Select the input folders, review the checks and
start setup. Folder picker buttons appear when zenity or kdialog is available;
paths can always be entered directly. A new runtime is built in a temporary
directory and published only after successful verification. Cancelling removes
that operation's temporary copy. No source prefix, game or runner is overwritten.
The existing-runtime option checks an already prepared folder and connects it.

Both the guided interface and the command below use `flightdeck.setup`, so the
artifact checks, runner fingerprint and no-overwrite rules stay consistent.
Close any application using the source Wine prefix before copying it.

Supply your own Xodus-compatible Proton runner, game downloaded using your
purchased account, and initialized x64 Wine prefix with working graphics.
Runner revisions are pinned in `compat/upstreams.lock.json`. The tested
original `xgameruntime.dll` is a free Wine builtin from that runner; the source
repository deliberately does not distribute it. A different original DLL is
rejected because the proxy's private interface layout was checked against the
pinned version. A normal upstream Wine build lacks the required
`WINE_DLL_FILE_MAP` loader extension.

```sh
python3 scripts/stage-runtime.py \
  --artifacts build/compat/artifacts \
  --game "$OWNED_GAME_DIRECTORY" --runner "$RUNNER" \
  --prefix "$PREPARED_WINE_PREFIX" --destination "$NEW_RUNTIME_DIRECTORY" \
  --market "$STORE_COUNTRY_CODE" --local-saves
```

The command verifies artifact hashes and the runner ABI fingerprint, copies
the prefix using reflinks where available, links the supplied game and runner,
and installs the newly built components into the copy. It refuses an existing
destination. The original prefix, runner and game content are not rewritten.
The result is local user data, including the copied prefix; never publish it.
No login, game start, download or package installation occurs during staging.
The configured market is also used for public Store catalog prices. Product
text follows the Wine user locale; this setting does not select or change the
authenticated Microsoft Store account.

The staging command's `--local-saves` option creates
`private/local-saves.enabled` with mode0600 and its storage directory with
mode0700. The startup wrapper uses this gate to set the native provider option.
Normal launcher starts enable this provider automatically for cloud-backed
sessions. The launcher loads cloud saves before play and uploads local changes
after the game exits, keeping local backups. It does not merge live game writes
in the middle of a session; see [cloud saves](cloud-saves.md).
The implementation rejects `syncOnDemand=true`; the observed game uses false.
Keep the entire local-saves directory when backing up or moving this runtime.

The game requires native Vulkan/DXVK/VKD3D setup in the supplied prefix and
compatible GStreamer codecs for startup video. `--media-plugins DIRECTORY`
adds an already prepared plugin directory without modifying the system. Codec
ABI and driver availability are host-specific. The source builder does not
download or install graphics, multimedia or system packages.

## Authentication and launch

Run the prepared `tools/xodus.sh login` for a normal interactive Microsoft
login if needed. The broker and CLI use Linux Secret Service on the current
desktop D-Bus session. No password/token file belongs in the source repository.
Run `tools/play-msfs.sh` from your graphical Linux session, or select the
prepared directory in Flightdeck. The wrapper creates an isolated per-runtime
IPC socket beneath XDG_RUNTIME_DIR, starts its broker, forwards termination
signals, and returns the actual game process exit status. Private logs remain
under the runtime's `private/` directory.

The loader wrapper preserves Xodus's inherited memory-file descriptors and
creates only a temporary sparse executable containing PE headers. It does not
write a decrypted executable or obtain a license itself. Temporary launch
headers are removed when the wrapper exits. The game and prefix paths must
match this runtime configuration.

## Current limits

Native play has reached a cockpit in development. This is an experimental
compatibility layer with a bounded read-only Store integration: explicit queries
can return simple consumable products, public desktop prices and actual
Store-account collection data. The catalog must establish the game's association
and the supported product shape. Unverified ownership and unsupported product
shapes produce an error. See [the Collections contract](marketplace-collections.md)
for the scope and validation level.

Whole-title DLC inventory, purchase dialogs and consumable fulfillment remain
unverified. The launcher supports full-package updates, integrity checks and
repairs; see [game updates](game-updates.md). Cloud saves synchronize automatically
before a managed game starts and after it exits, with local backups and explicit
conflict choices. The local save provider does not synchronize during gameplay.
The actual runner, driver, codecs, game version and service contracts can change
independently of this repository.

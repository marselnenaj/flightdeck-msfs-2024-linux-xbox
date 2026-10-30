# Wine media-cache and alternate-Proton validation

Issue [#16](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/16)
reports a black screen with Flightdeck 0.3.3 after the reporter had confirmed
working video with 0.2.25 and CachyOS 20261015-SLR. The media-selection source
and system-memory patch are identical in those two releases. This is a return
of a symptom; a particular intervening source change is not established as its cause.

## Wine registry mismatch reproduced on 30 October 2026

The new [reporter reply](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/16#issuecomment-6507238288)
confirms that 0.3.5 still black-screens after a clean installation. Its complete
safe scan reports all nine host elements ready, followed by Wine being unable to
create `videoconvert`. Sign-in and download completed; this does not establish an
authentication failure.

The [pinned Wine `wg_init_gstreamer`](https://github.com/xodus-gaming/wine/blob/b1dd32734a34472a28eb5be9922df06e07ac0834/dlls/winegstreamer/unixlib.c)
replaces `GST_REGISTRY_1_0` with
`WINE_GST_REGISTRY_DIR/registry.x86_64.bin` and disables registry forking. The
launcher had instead validated `registry.bin` with the ordinary scanner. These
were separate caches even when both runs selected the host media path.

An account-free reproduction used the installed pinned Xodus Wine family with
its existing Fenix ntdll pair and the verified system-memory module. The media
configuration of the retained 0.3.4-final driver is unchanged in 0.3.5. A temporary
incompatible libgstvideo in the test scanner generated a real GStreamer blacklist;
the public GStreamer API repaired unrelated entries, retaining only the stale
`videoconvertscale` entry. Neither the system plugins nor the installed runner
were modified.

| State | Native check | Actual Wine Media Foundation result |
| --- | --- | --- |
| Healthy, isolated caches | All nine elements ready | Two 120-frame 3840×2160 NV12 passes, rewind, EOS and shutdown |
| Stale Wine registry; separate Linux registry healthy | All nine elements ready | `videoconvert` missing, decode fails with exit 1 |
| Corrected driver, same damaged profile | Detects `videoconvert` missing; rebuilds the owned registry once | Same video passes both reads, rewind, EOS and shutdown |

The correction shares Wine's registry filename and initialization flags. It
rechecks after a bounded cache rebuild instead of resetting a prefix or changing
plugins. Missing software H.264 alone does not trigger cache deletion; explicit
plugin choices and links to unrelated files are retained. Tests cover persistent
failure and refuse symbolic/hard-linked registry files. Safe diagnostics identify
the shared registry scope, recovery state and 32-/64-bit missing-element evidence.

This proves the cache defect and its repair under Wine, including the exact
missing element named in the report. It does not establish that the reporter's
cache is its only cause or confirm a complete simulator flight. The additional
reset-preview failure was not reproduced: a real isolated Wine reset completed
with its backup. Maintenance diagnostics now retain a fixed failure phase and
numeric errno so a subsequent report can identify that separate failure without
exposing paths.

## Isolated comparison on 29 October 2026

The official `proton-cachyos-11.0-20261015-slr-x86_64.tar.xz` was checked against
its published SHA-256:
`35e16eab636724e3ed7aa8e2dbc33eb8e102e0c63bbc28b5a39af544c75d5bd3`.
Separate drivers were built from the unchanged 0.2.25 and 0.3.3 source commits.
Each test used a new Wine prefix and private caches. No game or account was started.

| Test | Selected codecs | Result |
| --- | --- | --- |
| First 0.2.25 run | Host | Two 120-frame NV12 passes, rewind, EOS and shutdown passed |
| First 0.3.3 run | Runner | H.264 decoder missing; Media Foundation failed with `c00d36bb` |
| Repeated selection with both drivers and warmed caches | Host | Both selected the verified host fallback |
| Corrected decoder validation, fresh profile | Host | Both passes, rewind, EOS and shutdown passed |
| First final-version run without selection trace | Runner | Decode failed; the reason for retaining the runner was not captured |
| Twenty subsequent fresh-cache selections, including eight after evicting the test driver's file cache | Host | All selected the verified host fallback |
| Two full runs with retained selection trace | Host | Both passes, rewind, EOS and shutdown passed in both new profiles |
| Final bounded-retry implementation, eight fresh-cache selections | Host | All selected the verified host fallback |
| Final bounded-retry implementation, full video test in another new profile | Host | Both passes, rewind, EOS and shutdown passed |

The runner's codec plugins report unavailable `libvpx.so.9` and other Steam
runtime dependencies outside their intended container. The fallback must verify
that the alternative stack actually loads. A GStreamer factory lookup can return
cached metadata without loading the plugin. Flightdeck now instantiates the
fixed MP4/H.264 and Wine post-processing elements in its disposable native child.
Tests reproduce a cached factory existing while element creation fails.

These results establish a real media failure and a validation defect. The
cache-sensitive comparison does not establish a deterministic difference between
the two source versions, nor prove that it explains every symptom in #16.
The reporter's export names the host path but omits the missing element and the
applied overlay. Updated diagnostics retain those fixed-name facts from existing
logs. A fresh reporter test is still needed for the full simulator outcome.
The untraced failed run is retained as a limitation; it is not counted as a
successful regression test. The developer probe now saves `media-selection.log`,
and launch diagnostics include the host-loader check so future failures retain
the complete selection decision.
An unsuccessful host fallback is checked once more before retaining an incomplete
runner stack. Synthetic tests cover a transient codec failure, a transient loader
failure and a persistent loader failure; a failing loader never becomes accepted.

To repeat the account-free test, build the driver as described in
[BUILDING.md](../BUILDING.md), then run `flightdeck-tools probe media` with
`--runner-media`, the exact runner, a local `PressStartVideo-4K.mp4`, dimensions
3840×2160 and a new output folder. A successful test reads actual NV12 frames;
it rejects placeholder video and requires completed shutdown.

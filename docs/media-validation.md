# Alternate-Proton video validation

Issue [#16](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/16)
reports a black screen with Flightdeck 0.3.3 after the reporter had confirmed
working video with 0.2.25 and CachyOS 20261015-SLR. The media-selection source
and system-memory patch are identical in those two releases. This is a return
of a symptom; a particular intervening source change is not established as its cause.

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

To repeat the account-free test, build the driver as described in
[BUILDING.md](../BUILDING.md), then run `flightdeck-tools probe media` with
`--runner-media`, the exact runner, a local `PressStartVideo-4K.mp4`, dimensions
3840×2160 and a new output folder. A successful test reads actual NV12 frames;
it rejects placeholder video and requires completed shutdown.

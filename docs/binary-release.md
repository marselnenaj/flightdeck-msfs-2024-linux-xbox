# Native package provenance

Flightdeck's full Linux installer contains six free compatibility components,
their exact artifact manifest and an aggregate `THIRD-PARTY-NOTICES.txt`. It does
not contain Microsoft game files, a Wine prefix, account data, Linux system
libraries or the upstream Proton runner. The Git repository and source-only
archive still exclude compiled runtime binaries.

The current package is Flightdeck **0.1.2**, using the unchanged **0.1.1** native
components and their matching source archive. See
[release changes](changelog.md) before choosing an archive.

The release pins in `compat/bootstrap.lock.json` bind the component archive,
every executable/library and the notice file to SHA256 checksums. The installer
accepts only these eight native files. Bootstrap rechecks executable hashes
before use. Extracted archives use Python's data filter and are bounded by member
count and expanded size; downloads are HTTPS with checksums checked before use.

## Build and sources

Native code is built from the exact WineGDK/Xodus revisions and reviewed patches
in `compat/upstreams.lock.json`. The Xodus patch includes file-level download
resume with package binding and integrity checks, plus real reauthentication
when a paused installation outlives its sign-in.
The distributable build removes Wine DLL debug paths and remaps Rust source
locations so personal checkout/Cargo-cache paths are not embedded in the package.
The checked source manifest connects each patched source to its build artifacts.

`scripts/binary-release.py` creates both the native archive and its matching
`flightdeck-native-sources-0.1.1.tar.gz`. The source archive contains full patched
WineGDK and Xodus sources, proxy and cloud-helper sources, all 686 locked Cargo vendor packages,
the source manifest, original notices and an offline Cargo configuration. The
reviewed Linux normal/build dependency graph contains 505 packages. This is a
conservative graph including build tools, not a claim that all 505 are linked
into both executables. Other-target dependency sources are also retained.

From the extracted source archive's root, with the toolchain in `BUILDING.md`:

```sh
CARGO_NET_OFFLINE=true BUILD_JOBS=8 bash ./build-compat.sh .
```

Host development packages and compilers are external prerequisites. Modified
native components can be rebuilt and used through Flightdeck's advanced runtime
preparation, which accepts the new build manifest. The pinned beginner installer
does not prevent using modified LGPL/GPL components through that route.

## Notices

Xodus remains GPL-3.0-only. Wine-derived components, the proxy and the cloud helper retain
LGPL-2.1-or-later terms. Original MIT, Apache, BSD, ISC, MPL, Unicode and other
dependency notices are retained according to their actual source declarations.
Wine's bundled XML/C++ libraries, MinGW runtime, GCC runtime exception and Rust
standard-library notices are included. See `dependency-inventory.json` and the
aggregate notices shipped with the binary and corresponding sources.

Twenty-two crates omitted separate notice files from their published crate
archive. Original texts were retrieved from the immutable source revisions where
available. For `cms 0.2.3`, the original README explicitly grants Apache-2.0 or MIT;
the package includes that declaration and a clearly identified Apache reference
text. `xmlserde` and `xmlserde_derives 0.7.2` explicitly declare MIT in their
original Cargo manifests, but the upstream repository omits a standalone license
file. Those declarations, author metadata, original README and a labeled MIT
reference text are retained without inventing copyright ownership or dates.
These upstream attribution omissions are recorded in
`license-supplements/manifest.json` in the corresponding-source archive.

## Separate Fenix payload

Flightdeck's Fenix panel includes the MIT installer engine and fixed manifests.
The optional Wine overlay is downloaded from
[Fenix patch releases](https://github.com/marselnenaj/fenix-a320-linux-patch/releases)
only when requested, with its ZIP hash checked against `compat/fenix/release.json`
and its payload files checked against `compat/fenix/bundle.json`.

Patch **0.1.0-preview.1** contains nine replacement Wine modules, an MIT window
helper and launch integration, plus complete Wine sources, patches and build
instructions. The derived Wine components retain LGPL-2.1-or-later. That ZIP and
its corresponding-source archive are separate from Flightdeck's six native
components and source package. Fenix aircraft, proprietary executables, fonts,
accounts and copied profiles are excluded. Microsoft prerequisites are fetched
from Microsoft with pinned checksums; the user obtains the official Fenix
installer through their account.

[User workflow](addons.md#fenix-a320) ·
[Maintainer import and release checks](contributing.md#fenix-patch-releases)

## Runner and platform limits

The automatic setup downloads the unmodified pinned upstream Proton artifact
directly from the upstream distribution. Its ZIP digest was independently
checked against GitHub's artifact metadata, and the inner tar archive and
original runtime have separate pinned hashes. It is not repackaged by Flightdeck.
The current upstream artifact expires on **2026-11-05**. Maintainers must replace
that pin with a newly validated available upstream build before expiry; the
installer fails clearly if the download is no longer available. It never silently
switches to an untested latest runner. Already installed runners keep working.

The native binary currently requires glibc 2.39+ and compatible GTK 3,
WebKitGTK 4.1 and OpenSSL 3 libraries. Vulkan drivers and a Secret Service provider
must be present in the graphical session. GStreamer must supply `qtdemux`,
`h264parse` and `avdec_h264`; the setup probes these without reading game data.
Distribution-wide portability is not
established by the successful Arch Linux test. The first-install flow has a
100 GiB free-space floor. Download pause/resume retains verified complete files
within the current background installation session; up to four incomplete files
restart. Recovery after a service or system restart is not supported.

Xodus stores credentials in the desktop's Linux Secret Service under its upstream
service name. Per-installation XDG directories isolate local config/cache data;
they do not create separate account keyrings. Authentication and signed license
checks remain with Microsoft/Xodus, and download output containing tokens or
signed URLs is not exposed in Flightdeck's API or diagnostics.

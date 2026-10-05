# Rust and Python launcher performance

## Native page switching 0.2.5

The release-mode native interface was compared with 0.2.4 at
`b9918d977b78e414bf93eeffa599fb23dc328415` on the same workstation. Synthetic
pages are switched through the real controller and laid out with the same
persisted widget cache and software renderer. Each frame includes complete CPU
rasterization and pixel readback; these are not compositor presentation times.

| Measurement, median | 0.2.4 | 0.2.5 |
| --- | ---: | ---: |
| Return to Overview, layout at 1280 × 900 | 12.340 ms | 1.238 ms |
| Return to Overview, complete frame at 1× | 25.839 ms | 14.594 ms |
| Return to Overview, complete frame at 2× | 67.922 ms | 54.076 ms |
| Setup, complete frame at 2× | 22.745 ms | 19.626 ms |
| Mods, complete frame at 2× | 17.953 ms | 15.500 ms |

Decoded panorama pixels survive renderer cache eviction, eliminating repeated
PNG decompression on returning to Overview. Fully visible text also avoids a
separate clipping layer, while partially visible text retains explicit clipping.
The full Overview frame takes about 44% less time at 1× and 20% less at 2× in
this run. This does not establish 60 FPS at HiDPI or change simulator performance.

Three alternating runs per version contain seven measured cycles after one
discarded warm-up, giving 21 samples per page and scale. Both use Rust 1.98.0,
release optimization, the same fixtures, 1280 × 900 logical pixels and scales
1×/2×. No HTTP, game, account or cloud operations are part of the measurement.
[Raw samples, ranges and binary hashes](native-ui-latency-results.json) are
included. The baseline assertion verifies 0.2.4's original disconnected state
on navigation; both builds must be rebuilt from their actual source trees when
sharing a Cargo target directory.

Reproduce the individual measurements with the same
`native/ui/tests/performance.rs` harness in each checkout:

```sh
FLIGHTDECK_UI_BENCHMARK=/tmp/flightdeck-ui-latency.json \
  cargo test --locked --release -p flightdeck-ui --test performance -- --ignored
```

Separately, launch acknowledgement is now a synchronous controller state
change before HTTP is awaited. Page navigation keeps cached content and online
status while fetching new data, and cancels superseded read requests. Neither
change skips launch checks or cloud synchronization.

## Native desktop candidate 0.2.3 — 5 October 2026

The final glibc-2.39-compatible **0.2.3 candidate** was compared with the
unchanged Python **0.1.21** source at
`005194e2d071447637f48e74c406f410c4fe5284`. All 49 exported source files and
48 installed Python modules were checked against that revision. These results
measure the **background service**, with the graphical interface closed.

| Installed launcher measurement, median | Python 0.1.21 | Rust 0.2.3 |
| --- | ---: | ---: |
| Process start to first valid status response | 148.48 ms | 87.40 ms |
| Resident service memory after requests | 35.18 MiB | 14.38 MiB |
| Status API | 1.660 ms | 0.335 ms |
| Setup status API | 28.668 ms | 0.278 ms |
| Cloud-save status API | 0.488 ms | 0.199 ms |
| Launcher-update status API | 0.633 ms | 0.209 ms |

In this run the installed Rust service takes **41% less startup time** and
**59% less resident memory**. Direct invocation, without installed-wrapper
verification, takes 71.67 ms in Python and 19.38 ms in Rust; resident service
memory is 30.70 MiB and 10.81 MiB respectively. The setup endpoint difference
also includes avoiding repeated component hashing during status polling; it
is not a language-only comparison. Component installation still verifies files.

The same deterministic save workloads produce identical canonical/content
hashes and container/blob/byte counts:

| Workload | Python process time | Rust process time | Python/Rust ratio |
| --- | ---: | ---: | ---: |
| 128 KiB, 32 blobs | 25.98 ms | 2.54 ms | 10.2× |
| 1 MiB, 4,096 small blobs | 40.07 ms | 8.42 ms | 4.8× |
| 32 MiB, 32 blobs | 143.76 ms | 115.40 ms | 1.2× |

Each implementation runs seven fresh processes per mode/workload, alternating
order after one discarded warm-up. API figures include 350 requests per endpoint
and implementation, after five warm-up requests in each process. The filesystem
cache stays warm. This run used the same i9-13900K, 32 logical CPUs, Linux
`7.2.5-3-omarchy`, Python 3.14.7 and Rust 1.98.0 as the historical comparison
below. No other Flightdeck builds or test suites ran during this measurement.
HOME, XDG and launcher state were isolated; no game, account, Wine or cloud
operations ran. Save process times include startup and I/O, not just the codec.

[The complete 0.2.3 measurements](performance-native-ui-results.json) include
the measured binary SHA-256, ranges and sample counts. Use the reproduction
command below with the baseline-built 0.2.3 binary and installed package. These
are warm-cache observations on one workstation, not a cross-machine guarantee.
They do not measure GUI startup, GUI memory, frame rate, flight loading or MSFS
performance. Separate native rendering tests and real X11/Wayland window checks
are described in [native desktop verification](native-ui.md#verification).

## Historical service comparison: 0.2.0-dev.1

The native **0.2.0-dev.1** release build was compared with Python **0.1.21**
at commit `005194e2d071447637f48e74c406f410c4fe5284`.
On this workstation the installed Rust launcher starts in **72 ms instead of
134 ms** and uses **70% less resident service memory**. These are launcher
measurements; no simulator FPS, flight loading time or cloud-transfer speed was
measured.

### Installed packages

Both full packages were installed into separate test directories and started
through their installed commands. Times below are medians; lower is better.

| Measurement | Python 0.1.21 | Rust 0.2.0-dev.1 |
| --- | ---: | ---: |
| Process start to first valid status response | 134.09 ms | 72.28 ms |
| Resident service memory after requests | 35.21 MiB | 10.48 MiB |
| Status API | 1.580 ms | 0.310 ms |
| Setup status API | 26.814 ms | 0.219 ms |
| Cloud-save status API | 0.467 ms | 0.102 ms |
| Launcher-update status API | 0.585 ms | 0.098 ms |

The first benchmark exposed repeated hashing of bundled compatibility files
while polling setup status. Rust now checks the pinned download description
first for this capability query. Component acquisition and installation still
verify the actual files. The setup result therefore includes a change in work
performed, not just a language/compiler effect.

Direct invocation without the installed wrapper took **69.34 ms in Python** and
**11.90 ms in Rust**, with **30.70 MiB** and **8.71 MiB** resident service memory.
The installed measurements include verification and selection of the managed
release, making them the more relevant startup comparison for users.

The full compressed tar package grows from **31.51 MiB to 34.78 MiB**. Native
code and dependency/standard-library notices increase download size despite the
lower service memory requirement. This size comparison excludes companion
source archives, which are supplied separately in both releases.

### Identical save workloads

Each fresh process reads an XDLOCAL1 file, validates/decodes it, re-encodes it,
and computes canonical and logical content hashes. Both implementations must
produce identical hashes, container/blob counts and content byte counts.

| Workload | Python process time | Rust process time | Python/Rust ratio |
| --- | ---: | ---: | ---: |
| 128 KiB, 32 blobs | 21.80 ms | 1.82 ms | 12.0× |
| 1 MiB, 4,096 small blobs | 35.88 ms | 7.95 ms | 4.5× |
| 32 MiB, 32 blobs | 160.68 ms | 122.35 ms | 1.3× |

These timings include process/interpreter startup and file I/O. They do not
measure the codec in isolation. Startup accounts for much of the small-file
difference; large-buffer work improves less. The fixtures contain deterministic
synthetic bytes, never a player's saves.

## Method and reproduction

- Intel Core i9-13900K, 32 logical CPUs; Linux `7.2.5-3-omarchy`.
- Python 3.14.7 and Rust 1.98.0, with the Cargo release profile and locked dependencies.
- Seven measured fresh processes per implementation and mode, alternating
  execution order after one discarded warm-up. Filesystem cache remains warm.
- Fifty requests per endpoint per process after five warm-up requests: 350
  measured requests per endpoint and implementation, using new loopback HTTP
  connections.
- Separate state/config/data/cache directories, no configured runtime, no
  account, Wine, game or remote cloud operations. No other Flightdeck builds or
  test suites ran during the final measurement.
- RSS/HWM comes from the service process's `/proc` status. Browser memory,
  simulator memory and helper processes are outside this measurement.

[Machine-readable results](performance-results.json) include medians, ranges,
p95 observations and the exact measured native binary's SHA-256. Seven startup
samples describe this workstation run; they are not a cross-machine guarantee
or a cold-boot measurement.

Build Rust as described in [BUILDING.md](../BUILDING.md#native-packages), export
the referenced Python revision to a separate directory, and install both full
packages into isolated directories with `--no-desktop --no-launch`. Then run:

```sh
python3 scripts/benchmark-launchers.py \
  --binary target/release/flightdeck-rust \
  --python-source build/python-baseline \
  --python-commit 005194e2d071447637f48e74c406f410c4fe5284 \
  --rust-launcher build/rust-bin/flightdeck \
  --python-launcher build/python-bin/flightdeck \
  --repeats 7 --requests 50 \
  --output build/performance-results.json
```

Omit both installed-launcher arguments to measure direct invocation only. The
output path must be new. The script cleans up only its own temporary services
and data. It does not flush the workstation's filesystem cache or change CPU
settings.

## Historical web UI idle work (before the native desktop)

A separate Chromium comparison uses the JavaScript UI from commit
`4ef1fc7105a68118221c7846b01f460281aede22` and the optimized native-preview UI.
Both receive identical synthetic API responses. Each view stays visible and
unchanged for 30 seconds after settling; the implementations run sequentially.

| Work in 30 seconds | Previous UI | Optimized UI |
| --- | ---: | ---: |
| Overview: HTTP status reads | 10 | 3 |
| Installation: HTTP status reads across four endpoints | 50 | 12 |
| Overview: check-list DOM mutation records | 40 | 0 |
| Installation: check-list DOM mutation records | 80 | 0 |

That is **70% fewer idle requests on Overview and 76% fewer on Installation**
in this observation. Unchanged check lists keep their existing nodes. This
measures actual HTTP requests and DOM mutation records, not CPU usage, browser
memory or simulator performance. It is one observation per view, not a latency
or throughput benchmark.

In that historical web implementation, one shared timer polls idle views every ten seconds. Active jobs keep
1.5–3 second intervals, reads do not overlap, and scheduled polling stops while
the document is hidden. Focus/visibility return and explicit refresh revalidate
immediately. Service-side jobs continue independently of the browser. Job
reservations now share a single registry; unchanged reservation values do not
trigger another whole-app render. Locale formatters are reused until the language
changes. The Rust server also sends its embedded UI bytes without first copying
each entire asset into a new buffer; no separate speedup is claimed for that change.

[Measurement data and UI source hashes](ui-performance-results.json) record
both runs. Reproduce using the Chromium fixture (no real backend/game calls):

```sh
mkdir -p build/web-reference
git archive v0.2.2 ui | tar -x -C build/web-reference
mkdir -p build/ui-before
git archive 4ef1fc7105a68118221c7846b01f460281aede22 ui | tar -xf - -C build/ui-before
FLIGHTDECK_UI_SOURCE=build/ui-before/ui FLIGHTDECK_UI_BENCHMARK=1 \
  FLIGHTDECK_UI_ARTIFACTS=build/ui-measure-before node build/web-reference/ui/tests/browser-test.mjs
FLIGHTDECK_UI_BENCHMARK=1 FLIGHTDECK_UI_ARTIFACTS=build/ui-measure-after \
  node build/web-reference/ui/tests/browser-test.mjs
```

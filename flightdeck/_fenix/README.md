Vendored from https://github.com/marselnenaj/fenix-a320-linux-patch,
commit 87ab6a6b55116af77f62f1240cf9747c58811814 (0.1.0-preview.2 payload).
MIT; see compat/fenix/LICENSE. Update with scripts/sync-fenix.py.

Local changes: .NET preparation verifies both CLR architectures, completes
pending Wine restart work and attempts a bounded repair using the verified
Microsoft installer. Uncommitted, unchanged staging transactions can be retried
without restoring the user's active profile. Preserve these changes on import;
tests/test_framework.py covers the detection, repair and recovery contracts.

Flightdeck deploys its current hash-pinned launch scripts when installing an overlay; older overlay archives must not downgrade the portable loader. The overlay archive and its original integrity checks remain unchanged.

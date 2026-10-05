// SPDX-License-Identifier: MIT
// Explicit application assets; never include runtime files or build directories.
pub static ASSETS: &[(&str, &[u8])] = &[
    ("compat/bootstrap.lock.json", include_bytes!("../compat/bootstrap.lock.json")),
    ("compat/fenix/bundle.json", include_bytes!("../compat/fenix/bundle.json")),
    ("compat/fenix/release.json", include_bytes!("../compat/fenix/release.json")),
    ("compat/graphics.lock.json", include_bytes!("../compat/graphics.lock.json")),
    ("compat/upstreams.lock.json", include_bytes!("../compat/upstreams.lock.json")),
    ("scripts/runtime/launch-msfs.sh", include_bytes!("../scripts/runtime/launch-msfs.sh")),
    ("scripts/runtime/play-msfs.sh", include_bytes!("../scripts/runtime/play-msfs.sh")),
    ("scripts/runtime/runtime-env.sh", include_bytes!("../scripts/runtime/runtime-env.sh")),
    ("scripts/runtime/xodus-service.sh", include_bytes!("../scripts/runtime/xodus-service.sh")),
    ("scripts/runtime/xodus-wine-launch", include_bytes!("../scripts/runtime/xodus-wine-launch")),
    ("scripts/runtime/xodus.sh", include_bytes!("../scripts/runtime/xodus.sh")),
];

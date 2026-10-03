// SPDX-License-Identifier: MIT
//! Allowlisted Store events and fingerprints of the files actually used at launch.
use crate::{
    Result, files,
    log_reader::{self, regex},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
pub const COMPONENTS: [(&str, &str); 7] = [
    ("cli", "bin/xodus-cli"),
    ("broker", "bin/xodus-service"),
    ("storage", "bin/flightdeck-connected-storage.exe"),
    (
        "proxy",
        "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll",
    ),
    (
        "builtin",
        "local/store-runtime/x86_64-windows/xodus_store_test.dll",
    ),
    (
        "prefix_builtin",
        "local/msfs-prefix/drive_c/windows/system32/xodus_store_test.dll",
    ),
    (
        "unix",
        "local/store-runtime/x86_64-unix/xodus_store_test.so",
    ),
];
pub const METHODS: [&str; 11] = [
    "XStoreQueryGameLicenseAsync",
    "XStoreQueryEntitledProductsAsync",
    "XStoreProductsQueryNextPageAsync",
    "XStoreQueryLicenseTokenAsync",
    "XStoreQueryProductsAsync",
    "XStoreQueryConsumableBalanceRemainingAsync",
    "XStoreAcquireLicenseForDurablesAsync",
    "XStoreQueryGameAndDlcPackageUpdatesAsync",
    "XStoreShowPurchaseUIAsync",
    "XStoreQueryProductForCurrentGameAsync",
    "XStoreCanAcquireLicenseForStoreIdAsync",
];
const CATALOG: [&str; 9] = [
    "inventory",
    "inventory-catalog",
    "inventory-mapping",
    "inventory-page",
    "catalog",
    "mapping",
    "collections",
    "page",
    "result",
];
const REASONS: [&str; 18] = [
    "action-filters",
    "product-kind",
    "title-association",
    "sku-selection",
    "trial-sku",
    "package-payload",
    "bundle-sku",
    "subscription-sku",
    "product-videos",
    "sku-videos",
    "product-language",
    "sku-language",
    "offer-association",
    "offer-price",
    "offer-conditions",
    "price-precision",
    "no-offer",
    "ambiguous-price",
];
const ASYNC: [&str; 6] = [
    "schedule",
    "work_enter",
    "cancel",
    "cleanup",
    "begin_return",
    "context_retain",
];
const PHASES: [&str; 15] = [
    "prepare",
    "catalog",
    "authentication",
    "window_open",
    "bootstrap_started",
    "bootstrap_ready",
    "bootstrap_error",
    "window_ready",
    "checkout_ready",
    "load_timeout",
    "session_timeout",
    "expired",
    "error",
    "complete",
    "cancel",
];
const OUTCOMES: [&str; 9] = [
    "started",
    "passed",
    "failed",
    "cancelled",
    "expired",
    "timeout",
    "unsupported",
    "busy",
    "succeeded",
];
pub fn catalog_rows(text: &str) -> Vec<Value> {
    let pattern = regex(
        r"\[xodus-store-catalog\] stage=([a-z-]{1,32})(?: products=\d{1,10} skus=\d{1,10})? hr=([a-fA-F0-9]{8})(?:\s|$)",
    );
    let reason = regex(r"\breason=([a-z-]{1,32})(?:\s|$)");
    let mut result = Vec::new();
    for line in text.lines() {
        if let Some(c) = pattern.captures(line)
            && CATALOG.contains(&&c[1])
        {
            let mut value = json!({"stage":&c[1],"hresult":c[2].to_ascii_lowercase()});
            if matches!(&c[1], "mapping" | "inventory-mapping")
                && let Some(r) = reason.captures(line)
                && REASONS.contains(&&r[1])
            {
                value["reason"] = json!(&r[1]);
            }
            result.push(value);
        }
    }
    result
}
pub fn launch_record(root: &Path) -> Option<Value> {
    use std::os::unix::fs::MetadataExt;
    let capture = || -> Result<Value> {
        let root = files::directory(root, false)?;
        let mut hashes = BTreeMap::new();
        for (name, path) in COMPONENTS {
            let file = files::beneath(&root, path)?;
            let before = file.metadata()?;
            crate::error::require(
                before.len() > 0 && before.len() <= 128 * 1024 * 1024,
                "Invalid diagnostic component.",
            )?;
            let check = file.try_clone()?;
            let hash = files::digest(file)?;
            let after = check.metadata()?;
            crate::error::require(
                (
                    before.len(),
                    before.mtime(),
                    before.mtime_nsec(),
                    before.ctime(),
                    before.ctime_nsec(),
                ) == (
                    after.len(),
                    after.mtime(),
                    after.mtime_nsec(),
                    after.ctime(),
                    after.ctime_nsec(),
                ),
                "Changed diagnostic component.",
            )?;
            hashes.insert(name, hash);
        }
        Ok(json!({"launcher":crate::VERSION,"files":hashes}))
    };
    capture().ok()
}
fn build(value: Value) -> Option<Value> {
    let object = value.as_object()?;
    if object.len() != 2 || !regex(r"^[A-Za-z0-9.+-]{1,39}$").is_match(value["launcher"].as_str()?)
    {
        return None;
    }
    let hashes = value["files"].as_object()?;
    if hashes.keys().map(String::as_str).collect::<BTreeSet<_>>()
        != COMPONENTS.iter().map(|(name, _)| *name).collect()
        || hashes
            .values()
            .any(|v| !v.as_str().is_some_and(files::hex_digest))
    {
        return None;
    }
    Some(value)
}
pub fn session(run: &Path) -> Value {
    let mut result =
        json!({"components_at_launch":null,"events":[],"partial":false,"sources":{},"coverage":{}});
    let mut events = Vec::new();
    let mut partial = false;
    let timestamp = regex(r"\btime_ms=(\d{13})(?:\s|$)");
    let query = regex(r"\[xodus-store-query\] kind=(\d{1,2}) hr=([a-fA-F0-9]{8})\b");
    let asynchronous =
        regex(r"\[xodus-store-async\] kind=(\d{1,2}) stage=([a-z_]{1,20}) hr=([a-fA-F0-9]{8})\b");
    let event = regex(
        r"\[flightdeck-store-event\] time_ms=\d{13} seq=\d{1,6} phase=([a-z_]{1,24}) outcome=([a-z]{1,16})(?:\s|$)",
    );
    for source in ["game", "service"] {
        let scan = log_reader::scan(&run.join(format!("{source}.log")), |text| {
            partial |= text.contains("[flightdeck-store-events-truncated]");
            for line in text.lines() {
                if source == "service"
                    && line.len() < 4096
                    && let Some(raw) = line.strip_prefix("[flightdeck-store-build] ")
                    && let Ok(value) = crate::cloud::json(raw.as_bytes())
                    && let Some(value) = build(value)
                {
                    result["components_at_launch"] = value;
                }
                let Some(time) = timestamp
                    .captures(line)
                    .and_then(|c| c[1].parse::<u64>().ok())
                    .filter(|t| (946684800000..7258118400000).contains(t))
                else {
                    continue;
                };
                let mut row = json!({"time_ms":time,"source":source});
                if let Some(c) = query.captures(line)
                    && let Some(method) = c[1].parse::<usize>().ok().and_then(|n| METHODS.get(n))
                {
                    row["method"] = json!(method);
                    row["phase"] = json!("result");
                    row["hresult"] = json!(c[2].to_ascii_lowercase());
                } else if let Some(c) = asynchronous.captures(line)
                    && let Some(method) = c[1].parse::<usize>().ok().and_then(|n| METHODS.get(n))
                    && ASYNC.contains(&&c[2])
                {
                    row["method"] = json!(method);
                    row["phase"] = json!(&c[2]);
                    row["hresult"] = json!(c[3].to_ascii_lowercase());
                } else if let Some(mut catalog) = catalog_rows(line).pop() {
                    row["phase"] = catalog["stage"].take();
                    if let (Some(a), Some(b)) = (row.as_object_mut(), catalog.as_object_mut()) {
                        b.remove("stage");
                        a.extend(b.clone());
                    }
                } else if let Some(c) = event.captures(line)
                    && PHASES.contains(&&c[1])
                    && OUTCOMES.contains(&&c[2])
                {
                    row["phase"] = json!(&c[1]);
                    row["outcome"] = json!(&c[2]);
                } else {
                    continue;
                }
                events.push(row);
            }
            if events.len() > 256 {
                events.sort_by_key(|v| v["time_ms"].as_u64().unwrap_or(0));
                events.drain(..events.len() - 256);
                partial = true;
            }
        });
        match scan {
            Ok((coverage, _)) => {
                partial |= coverage["complete"] != true;
                result["sources"][source] = json!(if coverage["complete"] == true {
                    "complete"
                } else {
                    "partial"
                });
                result["coverage"][source] = coverage;
            }
            Err(_) => {
                result["sources"][source] = json!("unavailable");
                partial = true;
            }
        }
    }
    events.sort_by_key(|v| v["time_ms"].as_u64().unwrap_or(0));
    if events.len() > 256 {
        events.drain(..events.len() - 256);
        partial = true;
    }
    result["events"] = json!(events);
    result["partial"] = json!(partial);
    result
}

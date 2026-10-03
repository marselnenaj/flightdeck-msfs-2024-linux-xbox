// SPDX-License-Identifier: MIT
//! Numeric, allowlisted evidence from bounded game-log scans.
use crate::{
    Result, graphics_diagnostics,
    log_reader::{self, regex},
    store_diagnostics,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::Metadata,
    path::Path,
};
const USERS: [&str; 9] = [
    "XUserGetTokenAndSignatureAsync.complete",
    "signature.policy",
    "signature.pack",
    "x_user_token_and_signature_begin",
    "x_user_XUserGetTokenAndSignatureUtf16Async",
    "x_user_XUserGetTokenAndSignatureResultSize",
    "x_user_XUserGetTokenAndSignatureResult",
    "x_user_XUserGetTokenAndSignatureUtf16ResultSize",
    "x_user_XUserGetTokenAndSignatureUtf16Result",
];
const POLICY: [&str; 6] = [
    "default_fetch",
    "default_parse",
    "title_fetch",
    "title_parse",
    "merge",
    "title_publish",
];
const NETWORK: [&str; 10] = [
    "invalid",
    "unmatched",
    "ambiguous",
    "fetch-failed",
    "pins-unsupported",
    "tls-unsupported",
    "matched-unpinned",
    "matched-title-unpinned",
    "matched-https-fallback",
    "matched-title-https-fallback",
];
fn host_category(host: &str) -> &'static str {
    let host = host.to_ascii_lowercase();
    for (suffix, category) in [
        ("xboxlive.com", "xboxlive"),
        ("playfabapi.com", "playfab"),
        ("playfab.com", "playfab"),
        ("flightsimulator.com", "flightsimulator"),
    ] {
        if host == suffix || host.strip_suffix(suffix).is_some_and(|s| s.ends_with('.')) {
            return category;
        }
    }
    "other"
}
pub struct GameLog {
    rows: BTreeMap<&'static str, Vec<Value>>,
    limited: bool,
    auth: BTreeSet<u16>,
    exit: Value,
    graphics: Value,
    graphics_errors: BTreeMap<String, u64>,
    graphics_warnings: BTreeMap<String, u64>,
    audio_errors: BTreeMap<String, u64>,
    audio_warnings: BTreeMap<String, u64>,
    audio_observations: BTreeMap<String, u64>,
}
impl Default for GameLog {
    fn default() -> Self {
        Self {
            rows: BTreeMap::new(),
            limited: false,
            auth: BTreeSet::new(),
            exit: Value::Null,
            graphics: graphics_diagnostics::log_summary(""),
            graphics_errors: BTreeMap::new(),
            graphics_warnings: BTreeMap::new(),
            audio_errors: BTreeMap::new(),
            audio_warnings: BTreeMap::new(),
            audio_observations: BTreeMap::new(),
        }
    }
}
impl GameLog {
    fn add(&mut self, key: &'static str, row: Value) {
        let rows = self.rows.entry(key).or_default();
        if !rows.contains(&row) {
            if rows.len() < 256 {
                rows.push(row);
            } else {
                self.limited = true;
            }
        }
    }
    pub fn consume(&mut self, text: &str) {
        for c in regex(r"xodus-title-auth: host=(?:user|device|title|xsts)\.auth\.xboxlive\.com status=([1-5]\d{2})\b").captures_iter(text){if let Ok(n)=c[1].parse(){self.auth.insert(n);}}
        for c in regex(r"\[xodus-gamesave\] local_init enabled=([01]) sync_on_demand=([01]) hr=([0-9a-fA-F]{8})\b").captures_iter(text){self.add("local_save_init",json!({"enabled":u8::from(&c[1]=="1"),"sync_on_demand":u8::from(&c[2]=="1"),"hresult":c[3].to_ascii_lowercase()}));}
        for c in regex(r"\[xodus-store\] (XStore[A-Za-z0-9_]{1,80})(?: [^\r\n]{0,100})? hr=([0-9a-fA-F]{8})(?:\s|$)").captures_iter(text){if store_diagnostics::METHODS.contains(&&c[1])||["XStoreCreateContext","XStoreAcquireLicenseForPackageAsync","XStoreQueryAddOnLicensesAsync"].contains(&&c[1]){self.add("store_calls",json!({"method":&c[1],"hresult":c[2].to_ascii_lowercase()}));}}
        for c in regex(r"\[xodus-store-query\] kind=(10|[0-9]) hr=([0-9a-fA-F]{8})(?:\s|$)")
            .captures_iter(text)
        {
            if let Some(method) = c[1]
                .parse::<usize>()
                .ok()
                .and_then(|n| store_diagnostics::METHODS.get(n))
            {
                self.add(
                    "store_calls",
                    json!({"method":method,"hresult":c[2].to_ascii_lowercase()}),
                );
            }
        }
        for row in store_diagnostics::catalog_rows(text) {
            self.add("store_catalog", row);
        }
        for c in regex(
            r"xodus-user-api: ([A-Za-z0-9_.]{1,90}) call=\d{1,10} hr=([0-9a-fA-F]{8})(?:\s|$)",
        )
        .captures_iter(text)
        {
            if USERS.contains(&&c[1]) {
                self.add(
                    "user_calls",
                    json!({"method":&c[1],"hresult":c[2].to_ascii_lowercase()}),
                );
            }
        }
        for c in regex(r"xodus-user-policy-cache: stage=([a-z_]{1,32}) hr=([0-9a-fA-F]{8})(?:\s|$)")
            .captures_iter(text)
        {
            if POLICY.contains(&&c[1]) {
                self.add(
                    "policy_cache",
                    json!({"stage":&c[1],"hresult":c[2].to_ascii_lowercase()}),
                );
            }
        }
        for c in regex(r"xodus-signature-policy: call=\d{1,10} host=([A-Za-z0-9.-]{1,253}) matched=([01]) has_policy=([01]) index=\d{1,10} version=(\d{1,10}) supported=([01]) token_only=([01]) hr=([0-9a-fA-F]{8})(?:\s|$)").captures_iter(text){if let Ok(version)=c[4].parse::<u64>(){self.add("signature_policy",json!({"host_category":host_category(&c[1]),"matched":&c[2]=="1","has_policy":&c[3]=="1","version":version,"supported":&c[5]=="1","token_only":&c[6]=="1","hresult":c[7].to_ascii_lowercase()}));}}
        for c in regex(r"\[xodus-network\] security scheme=https host=([A-Za-z0-9.-]{1,253}) policy=([a-z-]{1,32}) result=([0-9a-fA-F]{8})(?:\s|$)").captures_iter(text){if NETWORK.contains(&&c[2]){self.add("network_security",json!({"host_category":host_category(&c[1]),"policy":&c[2],"hresult":c[3].to_ascii_lowercase()}));}}
        for c in regex(r"xodus-wine-launch: wine_pid=\d+ exit_code=(\d{1,10}) elapsed_seconds=(\d{1,10}(?:\.\d{1,6})?)(?:\s|$)").captures_iter(text){if let(Ok(code),Ok(seconds))=(c[1].parse::<u64>(),c[2].parse::<f64>()){self.exit=json!({"code":code,"seconds":seconds});}}
        for c in regex(r"xodus-wine-launch: wine_pid=\d+ signal=(\d{1,3}) shell_exit_code=(\d{1,3}) elapsed_seconds=(\d{1,10}(?:\.\d{1,6})?)(?:\s|$)").captures_iter(text){if let(Ok(signal),Ok(code),Ok(seconds))=(c[1].parse::<u64>(),c[2].parse::<u64>(),c[3].parse::<f64>()){self.exit=json!({"signal":signal,"code":code,"seconds":seconds});}}
        let evidence = graphics_diagnostics::log_summary(text);
        for key in ["observed_components", "error_symbols"] {
            let merged: BTreeSet<_> = self.graphics[key]
                .as_array()
                .into_iter()
                .flatten()
                .chain(evidence[key].as_array().into_iter().flatten())
                .filter_map(Value::as_str)
                .collect();
            self.graphics[key] = json!(merged);
        }
        if let Some(versions) = evidence["observed_versions"].as_object() {
            for (key, values) in versions {
                let merged: BTreeSet<_> = self.graphics["observed_versions"][key]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .chain(values.as_array().into_iter().flatten())
                    .filter_map(Value::as_str)
                    .collect();
                self.graphics["observed_versions"][key] =
                    json!(merged.into_iter().take(8).collect::<Vec<_>>());
            }
        }
        if let Some(observations) = evidence["observations"].as_object() {
            for (key, count) in observations {
                self.graphics["observations"][key] = json!(
                    self.graphics["observations"][key].as_u64().unwrap_or(0)
                        + count.as_u64().unwrap_or(0)
                );
            }
        }
        for c in regex(r"\b(err|warn):vkd3d-proton:").captures_iter(text) {
            *(if &c[1] == "err" {
                &mut self.graphics_errors
            } else {
                &mut self.graphics_warnings
            })
            .entry("vkd3d-proton".into())
            .or_default() += 1;
        }
        for c in
            regex(r"\b(err|warn):(mmdevapi|pulse|alsa|xaudio2|dsound|winegstreamer|mfplat|mf):")
                .captures_iter(text)
        {
            *(if &c[1] == "err" {
                &mut self.audio_errors
            } else {
                &mut self.audio_warnings
            })
            .entry(c[2].into())
            .or_default() += 1;
        }
        for (key, pattern) in [
            (
                "no_audio_driver",
                r"\berr:mmdevapi:init_driver:No driver from [^\r\n]{1,256} could be initialized",
            ),
            (
                "pulse_context_failed",
                r"\bwarn:pulse:pulse_contextcallback:Context failed:",
            ),
            (
                "audio_device_unavailable",
                r"\bwarn:mmdevapi:get_mmdevice_by_activatepath:Failed to get requested device",
            ),
        ] {
            let count = regex(pattern).find_iter(text).count() as u64;
            if count > 0 {
                *self.audio_observations.entry(key.into()).or_default() += count;
            }
        }
    }
    pub fn result(mut self, coverage: Value) -> Value {
        let mut result = json!({"auth_http":self.auth,"exit":self.exit,"log_coverage":coverage,"summary_limited":self.limited});
        for name in [
            "local_save_init",
            "store_calls",
            "store_catalog",
            "user_calls",
            "policy_cache",
            "signature_policy",
            "network_security",
        ] {
            let mut rows = self.rows.remove(name).unwrap_or_default();
            if ["store_calls", "store_catalog", "user_calls", "policy_cache"].contains(&name) {
                let field = if name == "store_catalog" || name == "policy_cache" {
                    "stage"
                } else {
                    "method"
                };
                rows.sort_by_key(|r| {
                    (
                        r[field].as_str().unwrap_or("").to_owned(),
                        r["hresult"].as_str().unwrap_or("").to_owned(),
                        r["reason"].as_str().unwrap_or("").to_owned(),
                    )
                });
            }
            result[name] = json!(rows);
        }
        self.graphics["scope"] = json!("bounded_scan");
        self.graphics["coverage"] = coverage.clone();
        self.graphics["error_counts"] = json!(self.graphics_errors);
        self.graphics["warning_counts"] = json!(self.graphics_warnings);
        result["graphics_log"] = self.graphics;
        result["audio"] = json!({"scope":"game_log","coverage":coverage,"error_counts":self.audio_errors,"warning_counts":self.audio_warnings,"observations":self.audio_observations});
        result
    }
}
pub fn read(path: &Path) -> Result<(Value, Metadata)> {
    let mut log = GameLog::default();
    let (coverage, info) = log_reader::scan(path, |text| log.consume(text))?;
    Ok((log.result(coverage), info))
}

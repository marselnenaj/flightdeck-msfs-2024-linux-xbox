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
pub const SCHEMA: u32 = 8;
const USERS: &[&str] = &[
    "XUserAddAsync.service",
    "XUserAddAsync.complete",
    "x_user_XUserAddAsync",
    "x_user_XUserAddResult",
    "x_user_XUserGetLocalId",
    "x_user_XUserGetId",
    "x_user_XUserGetState",
    "x_user_XUserFindUserById",
    "x_user_XUserFindUserByLocalId",
    "x_user_XUserDuplicateHandle",
    "x_user_XUserIsStoreUser",
    "x_user_XUserGetIsGuest",
    "x_user_XUserGetAgeGroup",
    "x_user_XUserCheckPrivilege",
    "x_user_XUserResolvePrivilegeWithUiAsync",
    "x_user_XUserResolvePrivilegeWithUiResult",
    "x_user_XUserResolveIssueWithUiAsync",
    "x_user_XUserResolveIssueWithUiResult",
    "x_user_XUserResolveIssueWithUiUtf16Async",
    "x_user_XUserResolveIssueWithUiUtf16Result",
    "x_user_XUserGetMsaTokenSilentlyAsync",
    "x_user_XUserGetMsaTokenSilentlyResultSize",
    "x_user_XUserGetMsaTokenSilentlyResult",
    "x_user_XUserGetGamerPictureAsync",
    "x_user_XUserGetGamerPictureResultSize",
    "x_user_XUserGetGamerPictureResult",
    "x_user_XUserGetTokenAndSignatureAsync",
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
const USER_LOAD_STAGES: [&str; 8] = [
    "default_cache",
    "msa_begin",
    "msa_await",
    "msa_result",
    "msa_token_property",
    "msa_device_property",
    "xbox_user_load",
    "default_cache_store",
];
const USER_AUTH_STAGES: [&str; 9] = [
    "proof_key",
    "user_token",
    "device_title_tokens",
    "xsts",
    "identity_validation",
    "endpoint_cache",
    "profile_http",
    "profile_json",
    "profile_fields",
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
    media_observations: BTreeMap<&'static str, u64>,
    media_configuration: Option<Value>,
    media_probes: BTreeMap<String, Value>,
    media_selection: Option<String>,
    media_missing: BTreeSet<&'static str>,
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
            media_observations: BTreeMap::new(),
            media_configuration: None,
            media_probes: BTreeMap::new(),
            media_selection: None,
            media_missing: BTreeSet::new(),
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
        for c in regex(r"xodus-runtime-lifetime: initialize argc=([234]) clients=\d{1,20} result=([0-9a-fA-F]{8})(?:\s|$)").captures_iter(text) {
            self.add("runtime_calls", json!({"method":"InitializeApiImpl","arguments":c[1].parse::<u8>().unwrap_or_default(),"hresult":c[2].to_ascii_lowercase()}));
        }
        for c in
            regex(r"xodus-user-bridge: (initialize|query|shutdown) result=([0-9a-fA-F]{8})(?:\s|$)")
                .captures_iter(text)
        {
            self.add(
                "runtime_calls",
                json!({"method":"UserRuntime","stage":&c[1],"hresult":c[2].to_ascii_lowercase()}),
            );
        }
        for c in regex(r"xodus-user-load: stage=([a-z_]{1,32}) hr=([0-9a-fA-F]{8})(?:\s|$)")
            .captures_iter(text)
        {
            if USER_LOAD_STAGES.contains(&&c[1]) {
                self.add("runtime_calls", json!({"method":"XUserAddAsync","stage":&c[1],"hresult":c[2].to_ascii_lowercase()}));
            }
        }
        for c in regex(r"xodus-user-auth: stage=([a-z_]{1,32}) hr=([0-9a-fA-F]{8})(?:\s|$)")
            .captures_iter(text)
        {
            if USER_AUTH_STAGES.contains(&&c[1]) {
                self.add("runtime_calls", json!({"method":"XUserAuthenticate","stage":&c[1],"hresult":c[2].to_ascii_lowercase()}));
            }
        }
        for c in regex(r"xodus-runtime: System\.(GetConsoleId|GetXboxLiveSandboxId|GetAppSpecificDeviceId|HandleTrack) result=([0-9a-fA-F]{8})(?:\s|$)").captures_iter(text) {
            self.add("runtime_calls", json!({"method":format!("XSystem{}",&c[1]),"hresult":c[2].to_ascii_lowercase()}));
        }
        for captures in regex(r"\bflightdeck-media: source=(host|runner) h264=(default|explicit|software_preferred)\b(?: overlay=(system_memory|unsupported|not_bundled|disabled|cache_changed)(?:\s|$))?").captures_iter(text) {
            self.media_configuration = Some(json!({"source":&captures[1],"h264":&captures[2]}));
            if let Some(overlay) = captures.get(3)
                && let Some(configuration) = self.media_configuration.as_mut()
            {
                configuration["overlay"] = json!(overlay.as_str());
            }
        }
        for captures in regex(r"\bflightdeck-media-selection: reason=(initial_host|runner|explicit|verified_host_fallback|runner_unverified)(?:\s|$)").captures_iter(text) {
            self.media_selection = Some(captures[1].into());
        }
        for captures in regex(r"\bflightdeck-media-probe: source=(host|runner) state=(ready|incomplete|initialization_failed|unavailable|probe_failed|timeout)(?:\s|$)").captures_iter(text) {
            self.media_probes.entry(captures[1].into()).or_insert_with(|| json!({"elements":{}}))["state"] = json!(&captures[2]);
        }
        for captures in regex(
            r"\bflightdeck-media-loader: source=(host) state=(ready|probe_failed|timeout)(?:\s|$)",
        )
        .captures_iter(text)
        {
            self.media_probes
                .entry(captures[1].into())
                .or_insert_with(|| json!({"elements":{}}))["loader"] = json!(&captures[2]);
        }
        for captures in regex(r"\bflightdeck-media-element: source=(host|runner) name=(avdec_h264|h264parse|qtdemux|decodebin|videoconvert|capsfilter|capssetter|deinterlace|videoflip) ready=([01])(?:\s|$)").captures_iter(text) {
            self.media_probes.entry(captures[1].into()).or_insert_with(|| json!({"elements":{}}))["elements"][&captures[2]] = json!(&captures[3] == "1");
        }
        for captures in regex(
            r"\bwinegstreamer: failed to create ([a-z0-9_]{1,64}), are (?:32|64)-bit GStreamer\b",
        )
        .captures_iter(text)
        {
            if let Some(name) = crate::native_probe::MEDIA_ELEMENTS
                .into_iter()
                .find(|name| *name == &captures[1])
            {
                self.media_missing.insert(name);
            }
        }
        for (name, pattern) in [
            (
                "placeholder_video",
                r"\bCreating tag file placeholder-video-used\b",
            ),
            ("h264_decode_failed", r"\bCannot decode caps video/x-h264\b"),
            ("h264_decoder_missing", r"\bMissing decoder: H\.264(?:\s|$)"),
            (
                "missing_plugin",
                r"\bYour GStreamer installation is missing a (?:required )?plug-in\b|\bwinegstreamer: failed to create [A-Za-z0-9_-]{1,64}, are (?:32|64)-bit GStreamer\b",
            ),
            (
                "transcoded_video_unavailable",
                r"\bEnv MEDIACONV_VIDEO_TRANSCODED_FILE is not set\b",
            ),
            (
                "plugin_library_unavailable",
                r"\bmodule_open failed: [A-Za-z0-9_.+-]{1,128}: cannot open shared object file\b",
            ),
        ] {
            let count = regex(pattern).find_iter(text).count() as u64;
            if count != 0 {
                *self.media_observations.entry(name).or_default() += count;
            }
        }
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
        if let Some(rows) = self.rows.remove("runtime_calls") {
            result["runtime_calls"] = json!(rows);
        }
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
        if self.media_configuration.is_some()
            || !self.media_observations.is_empty()
            || !self.media_probes.is_empty()
            || self.media_selection.is_some()
        {
            result["media"] = json!({"scope":"game_log","coverage":coverage.clone(),
                "configuration":self.media_configuration,"observations":self.media_observations,
                "selection_reason":self.media_selection,"probes":self.media_probes,"missing_elements":self.media_missing});
        }
        result["audio"] = json!({"scope":"game_log","coverage":coverage,"error_counts":self.audio_errors,"warning_counts":self.audio_warnings,"observations":self.audio_observations});
        result
    }
}
pub fn read(path: &Path) -> Result<(Value, Metadata)> {
    let mut log = GameLog::default();
    let (coverage, info) = log_reader::scan(path, |text| log.consume(text))?;
    Ok((log.result(coverage), info))
}

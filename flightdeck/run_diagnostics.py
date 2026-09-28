# SPDX-License-Identifier: MIT
"""Allowlisted game-log evidence, including authentication after Store startup."""
from collections import Counter
import re

from . import graphics_diagnostics, log_reader, store_diagnostics

USER_METHODS = {
    "XUserGetTokenAndSignatureAsync.complete", "signature.policy", "signature.pack",
    "x_user_token_and_signature_begin", "x_user_XUserGetTokenAndSignatureUtf16Async",
    "x_user_XUserGetTokenAndSignatureResultSize", "x_user_XUserGetTokenAndSignatureResult",
    "x_user_XUserGetTokenAndSignatureUtf16ResultSize", "x_user_XUserGetTokenAndSignatureUtf16Result",
}
POLICY_STAGES = {"default_fetch", "default_parse", "title_fetch", "title_parse", "merge", "title_publish"}
NETWORK_POLICIES = {"invalid", "unmatched", "ambiguous", "fetch-failed", "pins-unsupported", "tls-unsupported",
                    "matched-unpinned", "matched-title-unpinned", "matched-https-fallback", "matched-title-https-fallback"}
AUDIO_CHANNELS = "mmdevapi|pulse|alsa|xaudio2|dsound|winegstreamer|mfplat|mf"
STORE_METHODS = set(store_diagnostics.METHODS) | {"XStoreCreateContext", "XStoreAcquireLicenseForPackageAsync", "XStoreQueryAddOnLicensesAsync"}


def _host_category(host):
    # Never export arbitrary hostnames, title IDs, paths, queries or accounts.
    host = host.lower()
    for suffix, category in (("xboxlive.com", "xboxlive"), ("playfabapi.com", "playfab"),
                             ("playfab.com", "playfab"), ("flightsimulator.com", "flightsimulator")):
        if host == suffix or host.endswith("." + suffix):
            return category
    return "other"


class GameLog:
    def __init__(self):
        self.rows = {}
        self.limited = False
        self.auth_http = set()
        self.exit = None
        self.graphics = graphics_diagnostics.log_summary("")
        self.graphics_errors = Counter()
        self.graphics_warnings = Counter()
        self.audio_errors = Counter()
        self.audio_warnings = Counter()
        self.audio_observations = Counter()

    def add(self, key, rows):
        table = self.rows.setdefault(key, {})
        for row in rows:
            identity = tuple(row.items())
            if identity in table or len(table) < 256:
                table[identity] = row
            else:
                self.limited = True

    def consume(self, text):
        self.auth_http.update(int(x) for x in re.findall(r"xodus-title-auth: host=(?:user|device|title|xsts)\.auth\.xboxlive\.com status=([1-5]\d{2})\b", text))
        self.add("local_save_init", ({"enabled": int(e), "sync_on_demand": int(s), "hresult": h.lower()}
            for e, s, h in re.findall(r"\[xodus-gamesave\] local_init enabled=([01]) sync_on_demand=([01]) hr=([0-9a-fA-F]{8})\b", text)))
        calls = re.findall(r"\[xodus-store\] (XStore[A-Za-z0-9_]{1,80})(?: [^\r\n]{0,100})? hr=([0-9a-fA-F]{8})(?=\s|$)", text)
        calls += [(store_diagnostics.METHODS[int(kind)], hr) for kind, hr in
                  re.findall(r"\[xodus-store-query\] kind=(10|[0-9]) hr=([0-9a-fA-F]{8})(?=\s|$)", text)]
        self.add("store_calls", ({"method": method, "hresult": hr.lower()} for method, hr in calls if method in STORE_METHODS))
        self.add("store_catalog", ({"stage": stage, "hresult": hr.lower()} for stage, hr in
            re.findall(r"\[xodus-store-catalog\] stage=([a-z-]{1,32})(?: products=\d{1,10} skus=\d{1,10})? hr=([0-9a-fA-F]{8})(?=\s|$)", text)
            if stage in store_diagnostics.CATALOG))
        self.add("user_calls", ({"method": method, "hresult": hr.lower()} for method, hr in
            re.findall(r"xodus-user-api: ([A-Za-z0-9_.]{1,90}) call=\d{1,10} hr=([0-9a-fA-F]{8})(?=\s|$)", text) if method in USER_METHODS))
        self.add("policy_cache", ({"stage": stage, "hresult": hr.lower()} for stage, hr in
            re.findall(r"xodus-user-policy-cache: stage=([a-z_]{1,32}) hr=([0-9a-fA-F]{8})(?=\s|$)", text) if stage in POLICY_STAGES))
        policies = re.findall(r"xodus-signature-policy: call=\d{1,10} host=([A-Za-z0-9.-]{1,253}) matched=([01]) has_policy=([01]) index=\d{1,10} version=(\d{1,10}) supported=([01]) token_only=([01]) hr=([0-9a-fA-F]{8})(?=\s|$)", text)
        self.add("signature_policy", ({"host_category": _host_category(host), "matched": matched == "1",
            "has_policy": policy == "1", "version": int(version), "supported": supported == "1",
            "token_only": token == "1", "hresult": hr.lower()}
            for host, matched, policy, version, supported, token, hr in policies))
        self.add("network_security", ({"host_category": _host_category(host), "policy": policy, "hresult": hr.lower()}
            for host, policy, hr in re.findall(r"\[xodus-network\] security scheme=https host=([A-Za-z0-9.-]{1,253}) policy=([a-z-]{1,32}) result=([0-9a-fA-F]{8})(?=\s|$)", text)
            if policy in NETWORK_POLICIES))
        exits = re.findall(r"xodus-wine-launch: wine_pid=\d+ exit_code=(\d{1,10}) elapsed_seconds=(\d{1,10}(?:\.\d{1,6})?)(?=\s|$)", text)
        if exits:
            self.exit = {"code": int(exits[-1][0]), "seconds": float(exits[-1][1])}
        signals = re.findall(r"xodus-wine-launch: wine_pid=\d+ signal=(\d{1,3}) shell_exit_code=(\d{1,3}) elapsed_seconds=(\d{1,10}(?:\.\d{1,6})?)(?=\s|$)", text)
        if signals:
            self.exit = {"signal": int(signals[-1][0]), "code": int(signals[-1][1]), "seconds": float(signals[-1][2])}
        evidence = graphics_diagnostics.log_summary(text)
        for key in ("observed_components", "error_symbols"):
            self.graphics[key] = sorted(set(self.graphics[key]) | set(evidence[key]))
        for key, values in evidence["observed_versions"].items():
            self.graphics["observed_versions"][key] = sorted(set(self.graphics["observed_versions"].get(key, [])) | set(values))[:8]
        for key, count in evidence["observations"].items():
            self.graphics["observations"][key] = self.graphics["observations"].get(key, 0) + count
        for level in re.findall(r"\b(err|warn):vkd3d-proton:", text):
            (self.graphics_errors if level == "err" else self.graphics_warnings)["vkd3d-proton"] += 1
        for level, channel in re.findall(r"\b(err|warn):(" + AUDIO_CHANNELS + r"):", text):
            (self.audio_errors if level == "err" else self.audio_warnings)[channel] += 1
        for key, pattern in (
            ("no_audio_driver", r"\berr:mmdevapi:init_driver:No driver from [^\r\n]{1,256} could be initialized"),
            ("pulse_context_failed", r"\bwarn:pulse:pulse_contextcallback:Context failed:"),
            ("audio_device_unavailable", r"\bwarn:mmdevapi:get_mmdevice_by_activatepath:Failed to get requested device"),
        ):
            self.audio_observations[key] += len(re.findall(pattern, text))

    def result(self, coverage):
        data = {name: list(self.rows.get(name, {}).values()) for name in
                ("local_save_init", "store_calls", "store_catalog", "user_calls", "policy_cache", "signature_policy", "network_security")}
        for name in ("store_calls", "store_catalog", "user_calls", "policy_cache"):
            data[name].sort(key=lambda row: tuple(row.values()))
        self.graphics.update(scope="bounded_scan", coverage=coverage,
                             error_counts=dict(self.graphics_errors), warning_counts=dict(self.graphics_warnings))
        data.update(auth_http=sorted(self.auth_http), exit=self.exit, log_coverage=coverage,
                    summary_limited=self.limited, graphics_log=self.graphics,
                    audio={"scope": "game_log", "coverage": coverage,
                           "error_counts": dict(self.audio_errors), "warning_counts": dict(self.audio_warnings),
                           "observations": {key: count for key, count in self.audio_observations.items() if count}})
        return data


def read(path):
    log = GameLog()
    coverage, info = log_reader.scan(path, log.consume)
    return log.result(coverage), info

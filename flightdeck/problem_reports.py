# SPDX-License-Identifier: MIT
"""Explicit, local support drafts. No network requests or mail credentials."""
from __future__ import annotations

import hashlib
import json
import os
import platform
import re
import stat
import threading
import uuid

from .backend import LauncherError, atomic_json, utc_now

# Public contact address for official builds; no credentials belong here.
SUPPORT_EMAIL = "contact@flightdeck-app.com"
CATEGORIES = {"graphics", "cloud", "marketplace", "installation", "other"}
OBSERVATIONS = {"menus_visible", "main_view_black", "second_window_works", "second_window_crashes"}
SUMMARY_FIELDS = {"context", "run_found", "auth_http", "local_save_init", "store_calls",
                  "store_catalog", "store_session", "store_check", "exit", "cloud_sync",
                  "graphics", "audio", "user_calls", "policy_cache", "signature_policy",
                  "network_security", "log_coverage", "summary_limited"}
MAX_BYTES = 512 * 1024


def email_address(value):
    return (isinstance(value, str) and len(value) <= 254 and
            re.fullmatch(r"[A-Za-z0-9.!#$%'+/=_`{|}~-]{1,64}@[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?\.[A-Za-z]{2,63}", value)
            is not None and ".." not in value)


def encoded(report):
    return (json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False) + "\n").encode("utf-8")


def system_info():
    try:
        release = platform.freedesktop_os_release()
    except OSError:
        release = {}
    values = {"distribution": release.get("ID"), "release": release.get("VERSION_ID"),
              "architecture": platform.machine(), "kernel": platform.release()}
    return {key: value if isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9._+-]{1,100}", value)
            else None for key, value in values.items()}


class ProblemReports:
    def __init__(self, launcher):
        self.launcher = launcher
        self.path = launcher.state_dir / "problem-report.json"
        self.lock = threading.RLock()

    @staticmethod
    def recipient():
        address = os.environ.get("FLIGHTDECK_SUPPORT_EMAIL", SUPPORT_EMAIL)
        return address if email_address(address) else None

    def _read(self):
        try:
            fd = os.open(self.path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        except FileNotFoundError:
            return None
        with os.fdopen(fd, "rb") as stream:
            info = os.fstat(stream.fileno())
            if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                    or info.st_mode & 0o077 or info.st_size > MAX_BYTES):
                raise ValueError()
            value = json.loads(stream.read(MAX_BYTES + 1))
        if (not isinstance(value, dict) or set(value) != {"report", "sha256"}
                or not isinstance(value["report"], dict)
                or value["report"].get("schema") != 1
                or not re.fullmatch(r"[a-f0-9]{32}", str(value["report"].get("id", "")))
                or value["report"].get("category") not in CATEGORIES
                or not isinstance(value["report"].get("description"), str)
                or not isinstance(value["report"].get("observations"), list)
                or not isinstance(value["report"].get("system"), dict)
                or not isinstance(value["report"].get("diagnostics"), dict)
                or hashlib.sha256(encoded(value["report"])).hexdigest() != value["sha256"]):
            raise ValueError()
        return value

    def snapshot(self):
        with self.lock:
            try:
                draft = self._read()
                unreadable = False
            except (OSError, ValueError, TypeError, UnicodeError, RecursionError):
                draft, unreadable = None, True
            return {"recipient": self.recipient(), "draft": draft, "unreadable": unreadable}

    def prepare(self, data):
        if not isinstance(data, dict) or set(data) - {"category", "description", "observations", "runtime_path"}:
            raise LauncherError("Der Fehlerbericht enthält unbekannte Felder.")
        category, description = data.get("category"), data.get("description")
        observations = data.get("observations", [])
        if not isinstance(category, str) or category not in CATEGORIES:
            raise LauncherError("Bitte eine Fehlerkategorie auswählen.")
        if (not isinstance(description, str) or not 10 <= len(description.strip()) <= 4000
                or any(ord(c) < 32 and c not in "\n\t" for c in description)):
            raise LauncherError("Bitte den Fehler mit 10 bis 4000 Zeichen beschreiben.")
        try:
            description.encode("utf-8")
        except UnicodeError as error:
            raise LauncherError("Bitte den Fehler mit 10 bis 4000 Zeichen beschreiben.") from error
        if (not isinstance(observations, list) or len(observations) > len(OBSERVATIONS)
                or any(not isinstance(item, str) or item not in OBSERVATIONS for item in observations)
                or (category != "graphics" and observations)):
            raise LauncherError("Die Angaben zum Grafikfehler sind ungültig.")
        # A game switch or launch cannot change the context halfway through the
        # capture. No runtime paths are copied into the exported support draft.
        with self.launcher.lock:
            self.launcher.require_open()
            current = str(self.launcher.runtime) if self.launcher.runtime else None
            if data.get("runtime_path") != current:
                raise LauncherError("Die ausgewählte Installation hat sich geändert. Bitte den Status neu laden.")
            diagnostic = self.launcher.diagnostics()
            summary = {key: value for key, value in diagnostic["summary"].items() if key in SUMMARY_FIELDS}
            # Free-text check details can contain workstation paths. Statuses
            # suffice here; the existing local diagnostics view retains details.
            checks = [{"id": row["id"], "ok": row["ok"]} for row in diagnostic["checks"]
                      if isinstance(row, dict) and isinstance(row.get("id"), str)
                      and re.fullmatch(r"[a-z][a-z0-9_-]{0,39}", row["id"])
                      and "ok" in row and (row["ok"] is None or type(row["ok"]) is bool)]
            report = {"schema": 1, "id": uuid.uuid4().hex, "created_at": utc_now(),
                      "category": category, "description": description.strip(),
                      "observations": sorted(set(observations)), "system": system_info(),
                      "diagnostics": {"generated_at": diagnostic["generated_at"], "summary": summary, "checks": checks}}
        raw = encoded(report)
        if len(raw) > MAX_BYTES - 1024:
            raise LauncherError("Der Fehlerbericht ist zu groß. Bitte den lokalen Diagnoseexport verwenden.")
        draft = {"report": report, "sha256": hashlib.sha256(raw).hexdigest()}
        if len(encoded(draft)) > MAX_BYTES:
            raise LauncherError("Der Fehlerbericht ist zu groß. Bitte den lokalen Diagnoseexport verwenden.")
        with self.lock:
            atomic_json(self.path, draft)
        return {"ok": True, "recipient": self.recipient(), "draft": draft, "unreadable": False}

    def discard(self, report_id):
        with self.lock:
            try:
                draft = self._read()
            except (OSError, ValueError, TypeError, UnicodeError, RecursionError) as error:
                raise LauncherError("Der gespeicherte Bericht konnte nicht gelesen werden. Bitte einen neuen Bericht vorbereiten.") from error
            if not draft or draft["report"]["id"] != report_id:
                raise LauncherError("Dieser Bericht ist nicht mehr aktuell. Bitte die Ansicht neu laden.")
            self.path.unlink()
        return {"ok": True, "recipient": self.recipient(), "draft": None, "unreadable": False}

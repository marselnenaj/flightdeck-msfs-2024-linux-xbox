# SPDX-License-Identifier: MIT
"""OpenXR selection, Wine handoff and service lifecycle without hardware access."""
import json
import os
from pathlib import Path
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from flightdeck import vr
from flightdeck.backend import Launcher, LauncherError


READY = {"state": "ready", "vendor_id": 0x1002, "device_id": 0x73bf,
         "device_uuid": "1234567890abcdef1234567890abcdef",
         "instance_extensions": ["VK_KHR_external_memory_capabilities"],
         "device_extensions": ["VK_KHR_external_memory_fd", "VK_KHR_external_semaphore_fd"]}


class VRTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.runtime = self.root / "runtime"
        (self.runtime / "private").mkdir(parents=True)
        (self.runtime / "tools").mkdir()
        (self.runtime / "tools/play-msfs.sh").touch()
        self.environment = {"HOME": str(self.root), "XDG_CONFIG_HOME": str(self.root / "config"),
                            "XDG_DATA_HOME": str(self.root / "data"), "XDG_CONFIG_DIRS": str(self.root / "system-config")}

    def manifest(self, name="openxr_wivrn.json", active=False):
        path = self.root / "data/openxr/1" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        (path.parent / "libwivrn.so").touch()
        path.write_text(json.dumps({"file_format_version": "1.0.0", "runtime": {"library_path": "./libwivrn.so"}}))
        if active:
            link = self.root / "config/openxr/1/active_runtime.json"
            link.parent.mkdir(parents=True, exist_ok=True)
            link.symlink_to(path)
        return path

    def bridge(self):
        for name in ("bin/wine", "lib/wine/x86_64-windows/wineopenxr.dll", "lib/wine/x86_64-unix/wineopenxr.so"):
            path = self.runtime / "runner/files" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.touch()

    def mode(self, mode):
        (self.runtime / "private" / vr.SETTINGS).write_text(json.dumps({"schema": 1, "mode": mode}))

    def test_default_off_does_not_probe_or_modify_environment(self):
        original = {**self.environment, "WINE_DLL_FILE_MAP": "7:store.dll"}
        with patch.object(vr, "probe") as probe:
            self.assertEqual(vr.prepare(self.runtime, original), original)
            probe.assert_not_called()
        self.assertFalse((self.runtime / "private/vr").exists())

    def test_active_symlink_is_pinned_deduplicated_and_driver_is_not_loaded(self):
        path = self.manifest(active=True)
        with patch.object(vr, "probe") as probe:
            selected = vr.choose("auto", self.environment)
            self.assertEqual(selected, {"provider": "wivrn", "path": str(path), "active": True, "valid": True})
            self.assertEqual(len([p for p in vr.discover(self.environment) if p["path"] == str(path)]), 1)
            probe.assert_not_called()

    def test_invalid_explicit_runtime_never_falls_back_to_installed_runtime(self):
        self.manifest(active=True)
        for value in ("relative.json", str(self.root / "missing.json")):
            chosen = vr.choose("auto", {**self.environment, "XR_RUNTIME_JSON": value})
            self.assertFalse(chosen["valid"])
            self.assertTrue(chosen["active"])
        active = self.root / "config/openxr/1/active_runtime.json"
        active.unlink()
        active.symlink_to(self.root / "nonexistent")
        self.assertFalse(vr.choose("auto", self.environment)["valid"])

    def test_windows_and_flatpak_manifest_are_not_usable_host_runtimes(self):
        path = self.manifest(active=True)
        for library in (r"C:\windows\system32\wineopenxr.dll", "/app/lib/libopenxr_wivrn.so", None):
            path.write_text(json.dumps({"runtime": {"library_path": library}}))
            self.assertFalse(vr.choose("auto", self.environment)["valid"])

    def test_steamvr_discovery_and_host_registration_survive_xdg_isolation(self):
        registration = self.root / "config/openvr/openvrpaths.vrpath"
        registration.parent.mkdir(parents=True)
        steam = self.root / "SteamVR"
        steam.mkdir()
        registration.write_text(json.dumps({"runtime": [str(steam)]}))
        (steam / "vrclient.so").touch()
        (steam / "steamxr_linux64.json").write_text(json.dumps({"runtime": {"library_path": "./vrclient.so"}}))
        self.assertEqual(vr.choose("steamvr", self.environment)["path"], str(steam / "steamxr_linux64.json"))
        env = vr.host_environment(self.environment)
        self.assertEqual(env["VR_PATHREG_OVERRIDE"], str(registration))
        custom = {**env, "VR_PATHREG_OVERRIDE": "/custom/registration"}
        self.assertEqual(vr.host_environment(custom)["VR_PATHREG_OVERRIDE"], "/custom/registration")

    def test_settings_reject_invalid_schema_mode_and_symlink(self):
        path = self.runtime / "private" / vr.SETTINGS
        for value in ({"schema": 2, "mode": "auto"}, {"schema": 1, "mode": []}, [], None):
            path.write_text(json.dumps(value))
            with self.assertRaises(LauncherError):
                vr.settings(self.runtime)
        path.unlink()
        external = self.root / "outside.json"
        external.write_text('{"schema":1,"mode":"auto"}')
        path.symlink_to(external)
        with self.assertRaises(LauncherError):
            vr.settings(self.runtime)

    def test_prepare_separates_host_and_windows_manifests_and_preserves_store_bridge(self):
        path = self.manifest(active=True)
        self.bridge()
        self.mode("auto")
        captured = {}
        def apply(command, env, **kwargs):
            captured.update(environment=env, registry=Path(command[-1]).read_text(encoding="utf-16"))
            return {"returncode": 0, "output": b""}
        env = {**self.environment, "WINE_DLL_FILE_MAP": "7:store.dll", "WINEDLLOVERRIDES": "xgameruntime=n"}
        with patch.object(vr, "probe", return_value=READY), patch.object(vr, "run_child", side_effect=apply):
            prepared = vr.prepare(self.runtime, env)
        self.assertEqual(prepared["WINE_DLL_FILE_MAP"], env["WINE_DLL_FILE_MAP"])
        self.assertEqual(prepared["WINEDLLOVERRIDES"], "xgameruntime=n")
        self.assertEqual(prepared["XR_RUNTIME_JSON"], str(path))
        self.assertTrue(prepared["WINEXR_RUNTIME_JSON"].startswith("Z:\\"))
        self.assertEqual(prepared["DXVK_FILTER_DEVICE_UUID"], READY["device_uuid"])
        self.assertNotIn("WINE_DLL_FILE_MAP", captured["environment"])
        self.assertIn('"state"=dword:00000001', captured["registry"])
        self.assertIn("VK_KHR_win32_surface", captured["registry"])
        self.assertIn("VK_KHR_external_memory_fd", captured["registry"])
        manifest = self.runtime / "private/vr/wineopenxr64.json"
        self.assertEqual(json.loads(manifest.read_text())["runtime"]["library_path"], r"C:\windows\system32\wineopenxr.dll")
        self.assertEqual(list(manifest.parent.glob("*.reg")), [])
        self.assertNotIn("XR_RUNTIME_JSON", env)

    def test_failure_or_conflicting_gpu_prevents_registry_changes(self):
        self.manifest(active=True)
        self.bridge()
        self.mode("auto")
        for result, extra in (({"state": "headset_missing"}, {}), (READY, {"DXVK_FILTER_DEVICE_UUID": "0" * 32})):
            with patch.object(vr, "probe", return_value=result), patch.object(vr, "run_child") as apply:
                with self.assertRaises(LauncherError):
                    vr.prepare(self.runtime, {**self.environment, **extra})
                apply.assert_not_called()

    def test_nvidia_compatibility_preserves_hidden_vendor_and_uses_native_ids(self):
        self.manifest(active=True)
        self.bridge()
        self.mode("auto")
        native = {**READY, "vendor_id": 0x10de, "device_id": 0x2704}
        captured = []
        def apply(command, env, **kwargs):
            captured.append(Path(command[-1]).read_text(encoding="utf-16"))
            return {"returncode": 0}
        environment = {**self.environment, "WINE_HIDE_NVIDIA_GPU": "1", "DXVK_ENABLE_NVAPI": "0"}
        with patch.object(vr, "probe", return_value=native), patch.object(vr, "run_child", side_effect=apply):
            result = vr.prepare(self.runtime, environment)
        self.assertEqual(result["WINE_HIDE_NVIDIA_GPU"], "1")
        self.assertEqual(result["DXVK_ENABLE_NVAPI"], "0")
        self.assertEqual(result["DXVK_FILTER_DEVICE_UUID"], native["device_uuid"])
        self.assertIn('"openxr_vulkan_device_vid"=dword:000010de', captured[0])

    def test_manual_name_and_index_filters_are_rejected_before_probing(self):
        self.manifest(active=True)
        self.bridge()
        self.mode("auto")
        for name in ("DXVK_FILTER_DEVICE_NAME", "VKD3D_FILTER_DEVICE_NAME", "VKD3D_VULKAN_DEVICE"):
            with patch.object(vr, "probe") as probe:
                with self.assertRaises(LauncherError):
                    vr.prepare(self.runtime, {**self.environment, name: "0"})
                probe.assert_not_called()

    def test_failed_registry_setup_removes_temporary_import_file(self):
        self.manifest(active=True)
        self.bridge()
        self.mode("auto")
        with patch.object(vr, "probe", return_value=READY), patch.object(vr, "run_child", return_value={"state": "timeout"}):
            with self.assertRaises(LauncherError):
                vr.prepare(self.runtime, self.environment)
        self.assertEqual(list((self.runtime / "private/vr").glob("*.reg")), [])

    def test_probe_rejects_untrusted_driver_output_and_has_timeout_and_cancellation(self):
        selected = {"path": "/synthetic/runtime.json"}
        for output in (b"private driver output", b'FLIGHTDECK_XR_RESULT=[]', b'FLIGHTDECK_XR_RESULT={"state":"ready","device_uuid":"bad"}',
                       b'FLIGHTDECK_XR_RESULT={"state":"unexpected"}'):
            with patch.object(vr, "run_child", return_value={"returncode": 0, "output": output}):
                self.assertEqual(vr.probe(selected, {}), {"state": "failed"})
        with patch.object(vr, "run_child", return_value={"returncode": 0, "output": b'FLIGHTDECK_XR_RESULT=' + json.dumps(READY).encode()}):
            self.assertEqual(vr.probe(selected, {}), READY)
        child = [sys.executable, "-c", "import time; time.sleep(10)"]
        self.assertEqual(vr.run_child(child, {}, timeout=.05), {"state": "timeout"})
        cancel = threading.Event()
        cancel.set()
        self.assertEqual(vr.run_child(child, {}, cancelled=cancel), {"state": "cancelled"})
        self.assertEqual(set(vr.public_result(READY)), {"state", "message", "checked_at"})

    def test_explicit_check_reserves_runtime_and_releases_after_completion(self):
        self.bridge()
        self.mode("auto")
        launcher = Launcher(self.root / "state", str(self.runtime))
        manager = launcher.vr
        entered, finish = threading.Event(), threading.Event()
        self.addCleanup(manager.close)
        def wait_probe(*args, **kwargs):
            entered.set()
            finish.wait(5)
            return READY
        with patch.object(vr, "choose", return_value={"path": "/synthetic", "valid": True}), patch.object(vr, "probe", side_effect=wait_probe):
            try:
                manager.check(str(self.runtime))
                self.assertTrue(entered.wait(2))
                self.assertTrue(launcher.setup_busy)
                self.assertEqual(manager.snapshot()["check"]["state"], "checking")
                with self.assertRaises(LauncherError):
                    manager.configure(str(self.runtime), "off")
                with self.assertRaises(LauncherError):
                    launcher.reserve_setup()
            finally:
                finish.set()
                manager.thread.join(3)
        self.assertFalse(launcher.setup_busy)
        self.assertEqual(manager.snapshot()["check"]["state"], "ready")
        manager.configure(str(self.runtime), "off")
        self.assertEqual(vr.settings(self.runtime)["mode"], "off")
        self.assertIsNone(manager.snapshot().get("check"))
        with self.assertRaises(LauncherError):
            manager.configure("/different/runtime", "auto")
        with self.assertRaises(LauncherError):
            manager.check(str(self.runtime))

    def test_worker_start_failure_releases_reservation(self):
        self.mode("auto")
        launcher = Launcher(self.root / "state", str(self.runtime))
        with patch.object(threading.Thread, "start", side_effect=RuntimeError):
            with self.assertRaises(LauncherError):
                launcher.vr.check(str(self.runtime))
        self.assertFalse(launcher.setup_busy)
        launcher.vr.close()

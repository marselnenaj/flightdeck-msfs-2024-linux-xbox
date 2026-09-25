"""NVIDIA startup integration in isolated prefixes; no real GPU or driver I/O."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from flightdeck import graphics


NVIDIA = {"name": "NVIDIA GeForce RTX 4060", "vendor_id": 0x10de, "type": 2,
          "api_version": "1.3.280", "driver_version": "580.126.9.0"}
IGPU = {"name": "AMD Radeon Graphics", "vendor_id": 0x1002, "type": 1,
        "api_version": "1.3.280", "driver_version": "25.0.0"}


class GraphicsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.runtime = self.root / "runtime"
        self.windows = self.runtime / "local/msfs-prefix/drive_c/windows"
        for name in ("system32", "syswow64"):
            (self.windows / name).mkdir(parents=True)
        (self.runtime / "private").mkdir()
        self.runner = self.runtime / "runner/files"
        (self.runner / "bin").mkdir(parents=True)
        (self.runner / "bin/wine").write_bytes(b"synthetic runner")
        for target, name in graphics._NVAPI.items():
            source = self.runner / "lib/wine/nvapi" / name
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_bytes(b"synthetic runner component: " + target.encode())
        self.driver = self.root / "driver/nvidia/wine"
        self.driver.mkdir(parents=True)
        for name in ("nvngx.dll", "_nvngx.dll"):
            (self.driver / name).write_bytes(b"synthetic host driver: " + name.encode())
        for function, value in (("nvidia_present", True), ("probe", {"status": "ready", "devices": [IGPU, NVIDIA]}),
                                ("nvidia_directory", self.driver)):
            mocker = patch.object(graphics, function, return_value=value)
            setattr(self, function, mocker.start())
            self.addCleanup(mocker.stop)

    def test_nvidia_installs_matched_components_and_selects_one_adapter_for_both_apis(self):
        environment, report = graphics.prepare(self.runtime, {"WINEDLLOVERRIDES": "xgameruntime=n"})
        for target, name in graphics._NVAPI.items():
            self.assertEqual((self.windows / target).read_bytes(), (self.runner / "lib/wine/nvapi" / name).read_bytes())
        self.assertEqual((self.windows / "system32/nvngx.dll").read_bytes(), (self.driver / "nvngx.dll").read_bytes())
        self.assertEqual(environment["DXVK_ENABLE_NVAPI"], "1")
        self.assertEqual(environment["DXVK_FILTER_DEVICE_NAME"], NVIDIA["name"])
        self.assertEqual(environment["VKD3D_FILTER_DEVICE_NAME"], NVIDIA["name"])
        self.assertEqual(environment["NVIDIA_WINE_DLL_DIR"], str(self.driver))
        self.assertEqual(environment["WINEDLLOVERRIDES"], "xgameruntime=n;nvapi=n;nvapi64=n;nvofapi64=n;nvcuda=b")
        self.assertEqual(report["nvidia"], "ready")
        self.assertTrue(report["ngx_available"])
        self.assertNotIn(str(self.driver), json.dumps(report))

    def test_managed_libraries_follow_runner_and_host_driver_updates(self):
        graphics.prepare(self.runtime, {})
        source = self.runner / "lib/wine/nvapi/x86_64-windows/nvapi64.dll"
        source.write_bytes(b"updated runner component")
        (self.driver / "nvngx.dll").write_bytes(b"updated host driver")
        graphics.prepare(self.runtime, {})
        self.assertEqual((self.windows / "system32/nvapi64.dll").read_bytes(), source.read_bytes())
        self.assertEqual((self.windows / "system32/nvngx.dll").read_bytes(), b"updated host driver")

    def test_custom_dll_and_explicit_gpu_overrides_are_preserved(self):
        custom = self.windows / "system32/nvapi64.dll"
        custom.write_bytes(b"user supplied component")
        env = {"DXVK_FILTER_DEVICE_NAME": "selected by user", "VKD3D_VULKAN_DEVICE": "2",
               "WINEDLLOVERRIDES": "NvApi,NvApi64=b;other=n"}
        environment, report = graphics.prepare(self.runtime, env)
        self.assertEqual(custom.read_bytes(), b"user supplied component")
        self.assertEqual(environment["DXVK_FILTER_DEVICE_NAME"], env["DXVK_FILTER_DEVICE_NAME"])
        self.assertNotIn("VKD3D_FILTER_DEVICE_NAME", environment)
        self.assertIn("NvApi,NvApi64=b", environment["WINEDLLOVERRIDES"])
        self.assertEqual(report["custom_dlls"], ["nvapi64.dll"])
        self.assertNotIn("DXVK_ENABLE_NVAPI", env)

    def test_amd_and_intel_start_environment_is_unchanged(self):
        self.nvidia_present.return_value = False
        env = {"WINEDLLOVERRIDES": "custom=n", "VKD3D_CONFIG": "existing"}
        result, report = graphics.prepare(self.runtime, env)
        self.assertEqual(result, env)
        self.probe.assert_not_called()
        self.assertEqual(report, {"nvidia": "not_present"})
        self.assertFalse((self.runtime / "private/nvidia-runtime.json").exists())

    def test_multiple_discrete_gpus_are_not_arbitrarily_forced_to_nvidia(self):
        self.probe.return_value = {"status": "ready", "devices": [NVIDIA, {**IGPU, "type": 2}]}
        env, _ = graphics.prepare(self.runtime, {})
        self.assertNotIn("DXVK_FILTER_DEVICE_NAME", env)
        self.assertNotIn("VKD3D_FILTER_DEVICE_NAME", env)

    def test_failed_or_missing_nvidia_vulkan_does_not_modify_prefix(self):
        for report in ({"status": "failed", "devices": []}, {"status": "ready", "devices": [IGPU]}):
            with self.subTest(report=report):
                self.probe.return_value = report
                with self.assertRaises(graphics.GraphicsError):
                    graphics.prepare(self.runtime, {})
                self.assertFalse((self.runtime / "private/nvidia-runtime.json").exists())
                self.assertEqual(list((self.windows / "system32").iterdir()), [])

    def test_incomplete_runner_is_rejected_before_any_dll_is_installed(self):
        (self.runner / "lib/wine/nvapi/i386-windows/nvapi.dll").unlink()
        with self.assertRaises(graphics.GraphicsError):
            graphics.prepare(self.runtime, {})
        self.assertEqual(list((self.windows / "system32").iterdir()), [])

    def test_explicit_disable_skips_changes(self):
        for env in ({"PROTON_DISABLE_NVAPI": "1"}, {"DXVK_ENABLE_NVAPI": "0"}):
            result, report = graphics.prepare(self.runtime, env)
            self.assertEqual(result, env)
            self.assertEqual(report["nvidia"], "disabled")
        self.probe.assert_not_called()

    def test_missing_ngx_does_not_reuse_a_stale_managed_driver_copy(self):
        graphics.prepare(self.runtime, {})
        self.nvidia_directory.return_value = None
        env, report = graphics.prepare(self.runtime, {})
        self.assertFalse(report["ngx_available"])
        self.assertNotIn("NVIDIA_WINE_DLL_DIR", env)
        self.assertFalse((self.windows / "system32/nvngx.dll").exists())
        self.assertTrue((self.windows / "system32/nvapi64.dll").exists())

    def test_symlinked_system_directory_and_manifest_are_rejected(self):
        outside = self.root / "outside"
        outside.mkdir()
        target = self.windows / "syswow64"
        target.rmdir()
        target.symlink_to(outside, target_is_directory=True)
        with self.assertRaises(graphics.GraphicsError):
            graphics.prepare(self.runtime, {})
        self.assertEqual(list(outside.iterdir()), [])
        target.unlink()
        target.mkdir()
        secret = self.root / "unrelated"
        secret.write_text("unchanged")
        (self.runtime / "private/nvidia-runtime.json").symlink_to(secret)
        with self.assertRaises(graphics.GraphicsError):
            graphics.prepare(self.runtime, {})
        self.assertEqual(secret.read_text(), "unchanged")


class GraphicsProbeTests(unittest.TestCase):
    def test_probe_exports_only_bounded_hardware_fields(self):
        value = {"status": "ready", "devices": [{**NVIDIA, "device_uuid": "private", "token": "private"}]}
        with patch.object(graphics, "_child", return_value=value):
            result = graphics.probe()
        self.assertEqual(result["devices"], [NVIDIA])
        self.assertNotIn("private", json.dumps(result))

    def test_software_only_is_not_reported_as_hardware_ready(self):
        with patch.object(graphics, "_child", return_value={"status": "ready", "devices": [{**IGPU, "type": 4}]}):
            self.assertEqual(graphics.probe()["status"], "software_only")

    def test_probe_failure_and_malformed_driver_output_are_bounded(self):
        for value in (None, [], {"status": "ready", "devices": [None, {**NVIDIA, "name": "bad\nvalue"}]}):
            with self.subTest(value=value), patch.object(graphics, "_child", return_value=value):
                self.assertEqual(graphics.probe()["status"], "failed")

    def test_hardware_discovery_only_considers_display_devices(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            device = root / "pci-device"
            device.mkdir()
            (device / "vendor").write_text("0x10de\n")
            (device / "class").write_text("0x040300\n")
            self.assertFalse(graphics.nvidia_present(root))
            (device / "class").write_text("0x030000\n")
            self.assertTrue(graphics.nvidia_present(root))

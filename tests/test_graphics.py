"""NVIDIA startup integration in isolated prefixes; no real GPU or driver I/O."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from flightdeck import graphics


NVIDIA = {"name": "NVIDIA GeForce RTX 4060", "vendor_id": 0x10de, "type": 2,
          "api_version": "1.3.280", "driver_version": "580.126.9.0"}
IGPU = {"name": "AMD Radeon Graphics", "vendor_id": 0x1002, "type": 1,
        "api_version": "1.3.280", "driver_version": "25.0.0"}
DEVICE_UUID = "00112233445566778899aabbccddeeff"


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
        self.set_mode("features")
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
        for function, value in (("nvidia_present", True),
                                ("probe", {"status": "ready", "devices": [IGPU, {**NVIDIA, "device_uuid": DEVICE_UUID}]}),
                                ("nvidia_directory", self.driver)):
            mocker = patch.object(graphics, function, return_value=value)
            setattr(self, function, mocker.start())
            self.addCleanup(mocker.stop)

    def set_mode(self, mode):
        (self.runtime / "private" / graphics.SETTINGS_FILE).write_text(json.dumps({"schema": 1, "nvidia_mode": mode}))

    def test_new_and_existing_automatic_settings_select_complete_compatibility(self):
        settings = self.runtime / "private" / graphics.SETTINGS_FILE
        for saved in (None, "auto", "compatibility"):
            with self.subTest(saved=saved):
                settings.unlink(missing_ok=True)
                if saved is not None:
                    self.set_mode(saved)
                original = {"DXVK_ENABLE_NVAPI": "1", "VKD3D_DISABLE_EXTENSIONS": "VK_EXT_present_timing",
                            "DXVK_CONFIG": "dxvk.disableNvLowLatency2 = False; dxvk.latencySleep = True; dxgi.maxFrameRate = 60"}
                env, report = graphics.prepare(self.runtime, original)
                self.assertEqual(env["DXVK_ENABLE_NVAPI"], "0")
                self.assertEqual(env["WINE_HIDE_NVIDIA_GPU"], "1")
                self.assertEqual(env["VKD3D_DISABLE_EXTENSIONS"], "VK_EXT_present_timing;VK_NV_low_latency2")
                # DXVK's last assignment wins. Keep unrelated options intact.
                config = dict(item.strip().split(" = ", 1) for item in env["DXVK_CONFIG"].split(";") if item.strip())
                self.assertEqual(config["dxvk.disableNvLowLatency2"], "True")
                self.assertEqual(config["dxvk.latencySleep"], "False")
                self.assertEqual(config["dxgi.maxFrameRate"], "60")
                self.assertEqual(report["nvidia_mode"], saved or "auto")
                self.assertEqual(report["nvidia"], "disabled")
                self.assertEqual(env["DXVK_FILTER_DEVICE_UUID"], DEVICE_UUID)
                self.assertEqual(original["DXVK_ENABLE_NVAPI"], "1")
                self.nvidia_directory.assert_not_called()

    def test_nvidia_installs_matched_components_and_selects_one_adapter_for_both_apis(self):
        environment, report = graphics.prepare(self.runtime, {"WINEDLLOVERRIDES": "xgameruntime=n"})
        for target, name in graphics._NVAPI.items():
            self.assertEqual((self.windows / target).read_bytes(), (self.runner / "lib/wine/nvapi" / name).read_bytes())
        self.assertEqual((self.windows / "system32/nvngx.dll").read_bytes(), (self.driver / "nvngx.dll").read_bytes())
        self.assertEqual(environment["DXVK_ENABLE_NVAPI"], "1")
        self.assertEqual(environment["DXVK_FILTER_DEVICE_UUID"], DEVICE_UUID)
        self.assertNotIn("DXVK_FILTER_DEVICE_NAME", environment)
        self.assertNotIn("VKD3D_FILTER_DEVICE_NAME", environment)
        self.assertEqual(environment["NVIDIA_WINE_DLL_DIR"], str(self.driver))
        self.assertEqual(environment["WINEDLLOVERRIDES"], "xgameruntime=n;nvapi=n;nvapi64=n;nvofapi64=n;nvcuda=b")
        self.assertEqual(report["nvidia"], "ready")
        self.assertTrue(report["ngx_available"])
        self.assertNotIn(str(self.driver), json.dumps(report))
        self.assertEqual(report["adapter_selection"], "nvidia_uuid")
        self.assertNotIn(DEVICE_UUID, json.dumps(report))
        self.probe.assert_called_once_with(include_device_ids=True)

    def test_managed_libraries_follow_runner_and_host_driver_updates(self):
        graphics.prepare(self.runtime, {})
        source = self.runner / "lib/wine/nvapi/x86_64-windows/nvapi64.dll"
        source.write_bytes(b"updated runner component")
        (self.driver / "nvngx.dll").write_bytes(b"updated host driver")
        graphics.prepare(self.runtime, {})
        self.assertEqual((self.windows / "system32/nvapi64.dll").read_bytes(), source.read_bytes())
        self.assertEqual((self.windows / "system32/nvngx.dll").read_bytes(), b"updated host driver")

    def test_nvidia_startup_logging_preserves_explicit_preferences(self):
        env, _ = graphics.prepare(self.runtime, {})
        self.assertEqual(env["DXVK_LOG_LEVEL"], "info")
        self.assertEqual(env["VKD3D_DEBUG"], "warn")
        env, _ = graphics.prepare(self.runtime, {"DXVK_LOG_LEVEL": "none", "VKD3D_DEBUG": "warn"})
        self.assertEqual(env["DXVK_LOG_LEVEL"], "none")
        self.assertEqual(env["VKD3D_DEBUG"], "warn")

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

    def test_explicit_disable_blocks_nvapi_without_preparing_the_prefix(self):
        for env in ({"PROTON_DISABLE_NVAPI": "1"}, {"DXVK_ENABLE_NVAPI": "0"}):
            result, report = graphics.prepare(self.runtime, env)
            self.assertEqual(result["DXVK_ENABLE_NVAPI"], "0")
            self.assertEqual(result["WINEDLLOVERRIDES"], "nvapi,nvapi64,nvofapi64,*nvapi,*nvapi64,*nvofapi64=")
            self.assertEqual(report["nvidia"], "disabled")
        self.nvidia_directory.assert_not_called()
        self.assertFalse((self.runtime / "private/nvidia-runtime.json").exists())

    def test_nvapi_opt_out_keeps_the_same_physical_gpu_for_both_graphics_apis(self):
        # Hybrid systems can have one NVIDIA card and an AMD integrated GPU.
        # Turning off optional NVIDIA features must not undo adapter selection.
        enabled, _ = graphics.prepare(self.runtime, {})
        for flag in ({"PROTON_DISABLE_NVAPI": "1"}, {"DXVK_ENABLE_NVAPI": "0"}):
            with self.subTest(flag=flag):
                disabled, report = graphics.prepare(self.runtime, flag)
                for key in ("DXVK_FILTER_DEVICE_UUID", "__GLVND_DISALLOW_PATCHING"):
                    self.assertEqual(disabled.get(key), enabled[key])
                self.assertNotIn("VKD3D_FILTER_DEVICE_NAME", disabled)
                self.assertEqual(report["devices"], [IGPU, NVIDIA])

    def test_nvapi_opt_out_preserves_explicit_gpu_choices_and_does_not_choose_among_discrete_cards(self):
        for key in graphics._SELECTORS:
            with self.subTest(key=key):
                env = {"DXVK_ENABLE_NVAPI": "0", key: "user choice"}
                result, _ = graphics.prepare(self.runtime, env)
                self.assertEqual({k: v for k, v in result.items() if k in graphics._SELECTORS}, {key: "user choice"})
        self.probe.return_value = {"status": "ready", "devices": [NVIDIA, {**IGPU, "type": 2}]}
        result, _ = graphics.prepare(self.runtime, {"PROTON_DISABLE_NVAPI": "1"})
        self.assertFalse(set(result) & set(graphics._SELECTORS))

    def test_nvapi_opt_out_still_checks_nvidia_vulkan_before_starting(self):
        self.probe.return_value = {"status": "failed", "devices": []}
        for flag in ({"PROTON_DISABLE_NVAPI": "1"}, {"DXVK_ENABLE_NVAPI": "0"}):
            with self.subTest(flag=flag), self.assertRaises(graphics.GraphicsError):
                graphics.prepare(self.runtime, flag)
        self.assertFalse((self.runtime / "private/nvidia-runtime.json").exists())

    def test_disable_after_enabled_launch_retains_files_and_can_be_reenabled(self):
        graphics.prepare(self.runtime, {})
        marker = self.runtime / "private/nvidia-runtime.json"
        original = {name: (self.windows / name).read_bytes() for name in graphics._FILES}
        manifest = marker.read_bytes()
        for flag in ({"PROTON_DISABLE_NVAPI": "1"}, {"DXVK_ENABLE_NVAPI": "0"}):
            with self.subTest(flag=flag):
                env = {"WINEDLLOVERRIDES": "xgameruntime=n;NvApi64=n;*nvapi64=n",
                       "DXVK_FILTER_DEVICE_NAME": NVIDIA["name"], "DXVK_ENABLE_NVAPI": "1", **flag}
                result, report = graphics.prepare(self.runtime, env)
                self.assertEqual(result["DXVK_ENABLE_NVAPI"], "0")
                self.assertEqual(result["WINEDLLOVERRIDES"], env["WINEDLLOVERRIDES"] +
                                 ";nvapi,nvapi64,nvofapi64,*nvapi,*nvapi64,*nvofapi64=")
                self.assertEqual(result["DXVK_FILTER_DEVICE_NAME"], NVIDIA["name"])
                self.assertEqual(report["nvidia"], "disabled")
                self.assertEqual(marker.read_bytes(), manifest)
                for name, contents in original.items():
                    self.assertEqual((self.windows / name).read_bytes(), contents)
                self.assertNotIn("nvofapi64=", env["WINEDLLOVERRIDES"])
        result, report = graphics.prepare(self.runtime, {})
        self.assertEqual(result["DXVK_ENABLE_NVAPI"], "1")
        self.assertIn("nvapi64=n", result["WINEDLLOVERRIDES"])
        self.assertEqual(report["nvidia"], "ready")

    def test_disable_preserves_custom_dlls(self):
        target = self.windows / "system32/nvapi64.dll"
        target.write_bytes(b"user supplied component")
        _, report = graphics.prepare(self.runtime, {"PROTON_DISABLE_NVAPI": "1"})
        self.assertEqual(report["nvidia"], "disabled")
        self.assertEqual(target.read_bytes(), b"user supplied component")

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

    def test_compatibility_survives_restart_and_returns_to_driver_features(self):
        # Real files installed by a prior normal start must not make the
        # process accidentally opt back into NVIDIA features.
        graphics.prepare(self.runtime, {})
        before = {name: (self.windows / name).read_bytes() for name in graphics._FILES}
        settings = self.runtime / "private" / graphics.SETTINGS_FILE
        settings.write_text(json.dumps({"schema": 1, "nvidia_mode": "compatibility"}))
        original = {"DXVK_ENABLE_NVAPI": "1", "PROTON_HIDE_NVIDIA_GPU": "0",
                    "PROTON_FORCE_NVAPI": "1", "WINE_HIDE_NVIDIA_GPU": "0",
                    "DXVK_CONFIG": "dxgi.maxFrameRate = 60; dxgi.hideNvidiaGpu = False",
                    "WINEDLLOVERRIDES": "xgameruntime=n;nvapi64=n;nvngx=n"}
        env, report = graphics.prepare(self.runtime, original)
        self.assertEqual(env["DXVK_ENABLE_NVAPI"], "0")
        self.assertEqual(env["WINE_HIDE_NVIDIA_GPU"], "1")
        self.assertEqual(env["VKD3D_DISABLE_EXTENSIONS"], "VK_NV_low_latency2")
        self.assertTrue(env["DXVK_CONFIG"].endswith("; dxgi.hideNvidiaGpu = True"))
        self.assertIn("dxgi.maxFrameRate = 60", env["DXVK_CONFIG"])
        from flightdeck.graphics_diagnostics import environment_overrides
        overrides = environment_overrides(env)
        for name in ("nvapi64", "nvapi", "nvofapi64", "nvngx", "_nvngx", "*nvapi64", "*nvngx"):
            self.assertEqual(overrides[name], "disabled")
        self.assertEqual(report["nvidia_mode"], "compatibility")
        self.assertTrue(report["hide_nvidia"])
        self.assertEqual(env["DXVK_FILTER_DEVICE_UUID"], DEVICE_UUID)
        # Wine reports a different name when hiding the NVIDIA vendor. No
        # automatically supplied host-name filter may exclude that adapter.
        self.assertNotIn("DXVK_FILTER_DEVICE_NAME", env)
        self.assertNotIn("VKD3D_FILTER_DEVICE_NAME", env)
        self.assertEqual({name: (self.windows / name).read_bytes() for name in graphics._FILES}, before)
        self.assertEqual(original["DXVK_ENABLE_NVAPI"], "1")
        settings.write_text(json.dumps({"schema": 1, "nvidia_mode": "features"}))
        restored, report = graphics.prepare(self.runtime, {})
        self.assertEqual(restored["DXVK_ENABLE_NVAPI"], "1")
        self.assertNotIn("WINE_HIDE_NVIDIA_GPU", restored)
        self.assertNotIn("DXVK_CONFIG", restored)
        self.assertNotIn("VKD3D_DISABLE_EXTENSIONS", restored)
        self.assertFalse(report["hide_nvidia"])

    def test_compatibility_disables_renderer_reflex_without_losing_extension_preferences(self):
        settings = self.runtime / "private" / graphics.SETTINGS_FILE
        settings.write_text(json.dumps({"schema": 1, "nvidia_mode": "compatibility"}))
        original = {"VKD3D_DISABLE_EXTENSIONS": "VK_EXT_present_timing"}
        env, _ = graphics.prepare(self.runtime, original)
        self.assertEqual(env["VKD3D_DISABLE_EXTENSIONS"], "VK_EXT_present_timing;VK_NV_low_latency2")
        repeated, _ = graphics.prepare(self.runtime, env)
        self.assertEqual(repeated["VKD3D_DISABLE_EXTENSIONS"], env["VKD3D_DISABLE_EXTENSIONS"])
        self.assertEqual(original, {"VKD3D_DISABLE_EXTENSIONS": "VK_EXT_present_timing"})

    def test_compatibility_keeps_a_valid_extension_exclusion_with_inherited_spaces(self):
        # VKD3D recognizes only comma/semicolon boundaries; a space before the
        # extension must not make Flightdeck think it is already disabled.
        inherited = "VK_EXT_present_timing; VK_NV_low_latency2"
        environment = {"VKD3D_DISABLE_EXTENSIONS": inherited}
        graphics._nvidia_mode(environment, "compatibility")
        self.assertEqual(environment["VKD3D_DISABLE_EXTENSIONS"], inherited + ";VK_NV_low_latency2")

    def test_proton_hide_option_is_translated_for_wine_and_dxgi(self):
        for flag in ("PROTON_HIDE_NVIDIA_GPU", "WINE_HIDE_NVIDIA_GPU"):
            with self.subTest(flag=flag):
                env, report = graphics.prepare(self.runtime, {flag: "1"})
                self.assertEqual(env["WINE_HIDE_NVIDIA_GPU"], "1")
                self.assertEqual(env["DXVK_CONFIG"], "dxgi.hideNvidiaGpu = True")
                self.assertTrue(report["hide_nvidia"])
        env, report = graphics.prepare(self.runtime, {"PROTON_HIDE_NVIDIA_GPU": "0"})
        self.assertNotIn("WINE_HIDE_NVIDIA_GPU", env)
        self.assertNotIn("DXVK_CONFIG", env)

    def test_unique_partial_gpu_selection_is_completed_for_the_other_api(self):
        for selected, missing in (("DXVK_FILTER_DEVICE_NAME", "VKD3D_FILTER_DEVICE_NAME"),
                                  ("VKD3D_FILTER_DEVICE_NAME", "DXVK_FILTER_DEVICE_NAME")):
            with self.subTest(selected=selected):
                env, _ = graphics.prepare(self.runtime, {selected: "RTX 4060"})
                self.assertEqual(env[selected], "RTX 4060")
                self.assertEqual(env[missing], "RTX 4060")
                self.probe.return_value = {"status": "ready", "devices": [NVIDIA, {**NVIDIA, "name": "NVIDIA GeForce RTX 4060 Ti"}]}
                env, _ = graphics.prepare(self.runtime, {selected: "RTX 4060"})
                self.assertNotIn(missing, env)
                self.probe.return_value = {"status": "ready", "devices": [NVIDIA, IGPU]}

    def test_gpu_selection_is_not_tied_to_a_specific_nvidia_model(self):
        for name in ("NVIDIA GeForce GTX 1660", "NVIDIA GeForce RTX 3060", "NVIDIA GeForce RTX 4060",
                     "NVIDIA GeForce RTX 4060 Laptop GPU", "NVIDIA GeForce RTX 5060 Ti",
                     "NVIDIA GeForce RTX 5090", "NVIDIA RTX A4000"):
            with self.subTest(name=name):
                self.probe.return_value = {"status": "ready", "devices": [IGPU, {**NVIDIA, "name": name, "device_uuid": DEVICE_UUID}]}
                env, _ = graphics.prepare(self.runtime, {})
                self.assertEqual(env["DXVK_FILTER_DEVICE_UUID"], DEVICE_UUID)
                self.assertNotIn("DXVK_FILTER_DEVICE_NAME", env)
                self.assertNotIn("VKD3D_FILTER_DEVICE_NAME", env)

    def test_missing_invalid_or_ambiguous_ids_never_fall_back_to_host_names_or_indices(self):
        for identity in (None, "", "0" * 32, "invalid", "1" * 33, [], "private-id"):
            with self.subTest(identity=identity):
                self.probe.return_value = {"status": "ready", "devices": [IGPU, {**NVIDIA, "device_uuid": identity}]}
                env, report = graphics.prepare(self.runtime, {})
                self.assertFalse(set(env) & set(graphics._SELECTORS))
                self.assertEqual(report["adapter_selection"], "default")
                self.assertNotIn("device_uuid", json.dumps(report))
        self.probe.return_value = {"status": "ready", "devices": [
            {**IGPU, "device_uuid": DEVICE_UUID}, {**NVIDIA, "device_uuid": DEVICE_UUID}]}
        env, report = graphics.prepare(self.runtime, {})
        self.assertFalse(set(env) & set(graphics._SELECTORS))
        self.assertEqual(report["adapter_selection"], "default")

    def test_settings_are_bounded_validated_and_never_follow_symlinks(self):
        (self.runtime / "private" / graphics.SETTINGS_FILE).unlink()
        target = self.runtime / "private" / graphics.SETTINGS_FILE
        self.assertEqual(graphics.settings(self.runtime), {"nvidia_mode": "auto"})
        for value in ([], {"schema": 1, "nvidia_mode": {}}, {"schema": 1, "nvidia_mode": "untrusted"},
                      {"schema": 2, "nvidia_mode": "auto"}):
            target.write_text(json.dumps(value))
            with self.subTest(value=value), self.assertRaises(graphics.GraphicsError):
                graphics.prepare(self.runtime, {})
        target.write_text("x" * 4097)
        with self.assertRaises(graphics.GraphicsError):
            graphics.settings(self.runtime)
        target.unlink()
        outside = self.root / "outside.json"
        outside.write_text('{"schema":1,"nvidia_mode":"compatibility"}')
        target.symlink_to(outside)
        with self.assertRaises(graphics.GraphicsError):
            graphics.settings(self.runtime)
        self.assertTrue(graphics.snapshot(self.runtime)["error"])


class GraphicsProbeTests(unittest.TestCase):
    def test_probe_exports_only_bounded_hardware_fields(self):
        value = {"status": "ready", "devices": [{**NVIDIA, "device_uuid": "private", "token": "private"}]}
        with patch.object(graphics, "_child", return_value=value):
            result = graphics.probe()
        self.assertEqual(result["devices"], [NVIDIA])
        self.assertNotIn("private", json.dumps(result))

    def test_device_ids_are_validated_and_only_available_to_internal_selection(self):
        device = {**NVIDIA, "device_uuid": DEVICE_UUID, "driver_uuid": "private", "token": "private"}
        with patch.object(graphics, "_child", return_value={"status": "ready", "devices": [device]}):
            self.assertEqual(graphics.probe()["devices"], [NVIDIA])
            internal = graphics.probe(include_device_ids=True)
            self.assertEqual(internal["devices"], [{**NVIDIA, "device_uuid": DEVICE_UUID}])
            self.assertNotIn("private", json.dumps(internal))
        for identity in (None, [], "0" * 32, "1234", "g" * 32, "0" * 31 + "\n"):
            with self.subTest(identity=identity), patch.object(graphics, "_child", return_value={
                    "status": "ready", "devices": [{**NVIDIA, "device_uuid": identity}]}):
                self.assertEqual(graphics.probe(include_device_ids=True)["devices"], [NVIDIA])

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


class GraphicsLaunchScriptTests(unittest.TestCase):
    def test_real_shell_launcher_retains_selected_adapter_and_logging_preferences(self):
        with tempfile.TemporaryDirectory() as directory:
            runtime = Path(directory)
            scripts = Path(__file__).resolve().parents[1] / "scripts/runtime"
            (runtime / "tools").mkdir()
            (runtime / "private").mkdir()
            (runtime / "desktop").mkdir()
            game = runtime / "games/MSFS2024"
            game.mkdir(parents=True)
            for name in (".xodus-streaming.msixvc", "FlightSimulator2024.exe"):
                (game / name).write_bytes(b"synthetic placeholder")
            (runtime / "private/runtime.json").write_text('{"game_id":"msfs2024","market":"AT"}')
            for name in ("launch-msfs.sh", "runtime-env.sh"):
                shutil.copy2(scripts / name, runtime / "tools" / name)
            capture = runtime / "tools/xodus.sh"
            capture.write_text("#!/usr/bin/env python3\nimport json,os\n"
                               "print(json.dumps({k:os.environ.get(k) for k in "
                               "('DXVK_LOG_LEVEL','VKD3D_DEBUG','DXVK_FILTER_DEVICE_UUID','VKD3D_FILTER_DEVICE_NAME')}))\n")
            capture.chmod(0o700)
            base = {"PATH": os.defpath, "XDG_RUNTIME_DIR": str(runtime / "desktop")}
            for preferences in ({}, {"DXVK_LOG_LEVEL": "info", "VKD3D_DEBUG": "info"},
                                {"DXVK_LOG_LEVEL": "none", "VKD3D_DEBUG": "warn"}):
                with self.subTest(preferences=preferences):
                    env = base | preferences | {"DXVK_FILTER_DEVICE_UUID": DEVICE_UUID}
                    result = subprocess.run(["bash", str(runtime / "tools/launch-msfs.sh")],
                        env=env, capture_output=True, text=True, timeout=5, check=True)
                    actual = json.loads(result.stdout)
                    self.assertEqual(actual, {"DXVK_LOG_LEVEL": preferences.get("DXVK_LOG_LEVEL", "warn"),
                        "VKD3D_DEBUG": preferences.get("VKD3D_DEBUG", "warn"),
                        "DXVK_FILTER_DEVICE_UUID": DEVICE_UUID, "VKD3D_FILTER_DEVICE_NAME": None})

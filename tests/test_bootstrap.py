"""Component bootstrap fixtures; no network, accounts or real Wine execution."""
# SPDX-License-Identifier: MIT
import hashlib
import io
import json
from email.message import Message
from pathlib import Path
import tarfile
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import urllib.request

from flightdeck import bootstrap
from flightdeck.setup import (ARTIFACTS, CONNECTED_STORAGE_FEATURE, CONNECTED_STORAGE_HELPER,
                              SetupError, SetupCancelled)


class BootstrapTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.source = self.root / "source"
        self.native = self.source / "build/compat/artifacts"
        files = {}
        for name in ARTIFACTS:
            path = self.native / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(("synthetic " + name).encode())
            if name.startswith("bin/"):
                path.chmod(0o700)
            files[name] = hashlib.sha256(path.read_bytes()).hexdigest()
        self.lock = {"native": {"files": files, "archive_sha256": None, "minimum_glibc": "2.39"}}
        lock = self.source / "compat/bootstrap.lock.json"
        lock.parent.mkdir()
        lock.write_text(json.dumps(self.lock))

    def tearDown(self):
        self.temp.cleanup()

    def test_native_checksum_and_execute_requirements(self):
        bootstrap.verify_native(self.native, self.lock)
        (self.native / "bin/xodus-cli").chmod(0o600)
        with self.assertRaisesRegex(SetupError, "nicht ausführbar"):
            bootstrap.verify_native(self.native, self.lock)
        (self.native / "bin/xodus-cli").chmod(0o700)
        (self.native / "runtime/xgameruntime.dll").write_bytes(b"changed")
        with self.assertRaisesRegex(SetupError, "Prüfsumme"):
            bootstrap.verify_native(self.native, self.lock)

    def test_native_selection_skips_stale_build_for_verified_build(self):
        import shutil
        other = self.source / "build/compat-marketplace-run23/artifacts"
        shutil.copytree(self.native, other)
        (self.native / "runtime/xgameruntime.dll").write_bytes(b"stale")
        # Isolate the module-level packaged path as well as source candidates.
        with patch.object(bootstrap, "__file__", str(self.source / "flightdeck/bootstrap.py")):
            self.assertEqual(bootstrap.native_path(self.source, self.lock), other)

    def helper_fixture(self):
        helper = self.native / CONNECTED_STORAGE_HELPER
        helper.write_bytes(b"synthetic cloud helper")
        helper.chmod(0o700)
        self.lock["native"]["files"][CONNECTED_STORAGE_HELPER] = hashlib.sha256(helper.read_bytes()).hexdigest()
        self.lock["native"]["features"] = [CONNECTED_STORAGE_FEATURE]
        return helper

    def test_optional_helper_is_hash_bound_and_advertised_missing_helper_fails(self):
        helper = self.helper_fixture()
        bootstrap.verify_native(self.native, self.lock)
        helper.write_bytes(b"tampered helper")
        with self.assertRaisesRegex(SetupError, "Prüfsumme"):
            bootstrap.verify_native(self.native, self.lock)
        del self.lock["native"]["files"][CONNECTED_STORAGE_HELPER]
        with self.assertRaisesRegex(SetupError, "Dateiliste"):
            bootstrap.verify_native(self.native, self.lock)

    def test_component_cache_survives_workspace_and_checks_every_file(self):
        import shutil
        self.helper_fixture()
        with patch.object(bootstrap, "data_home", return_value=self.root / "data"), \
             patch.object(bootstrap, "__file__", str(self.source / "flightdeck/bootstrap.py")):
            target = bootstrap.cached_native_path(self.lock)
            target.parent.mkdir(parents=True, mode=0o700)
            bootstrap.cache_native(self.native, self.lock)
            self.assertEqual(target.stat().st_mode & 0o777, 0o700)
            shutil.rmtree(self.native)
            self.assertEqual(bootstrap.native_path(self.source, self.lock), target)
            for name in self.lock["native"]["files"]:
                path = target / name
                payload = path.read_bytes()
                path.write_bytes(b"tampered")
                self.assertIsNone(bootstrap.native_path(self.source, self.lock))
                path.write_bytes(payload)
            self.assertEqual(list(target.parent.glob(".native-*")), [])

    def test_cache_cancel_and_symlink_never_publish_partial_components(self):
        self.helper_fixture()
        with patch.object(bootstrap, "data_home", return_value=self.root / "data"):
            target = bootstrap.cached_native_path(self.lock)
            target.parent.mkdir(parents=True)
            cancel = threading.Event(); cancel.set()
            with self.assertRaises(SetupCancelled):
                bootstrap.cache_native(self.native, self.lock, cancel)
            self.assertFalse(target.exists())
            self.assertEqual(list(target.parent.glob(".native-*")), [])
            target.symlink_to(self.native, target_is_directory=True)
            with self.assertRaises(SetupError):
                bootstrap.cache_native(self.native, self.lock)

    def test_bootstrap_packaged_and_downloaded_components_keep_six_files(self):
        self.helper_fixture()
        original = b"synthetic original runtime"
        runner_archive = b"synthetic compressed runner"
        self.lock["native"].update(archive_url="https://example.invalid/native", archive_sha256="1" * 64)
        self.lock["runner"] = {"url": "https://example.invalid/runner", "zip_sha256": "2" * 64,
                               "archive_sha256": hashlib.sha256(runner_archive).hexdigest(),
                               "directory": "runner", "original_runtime_sha256": hashlib.sha256(original).hexdigest()}
        for packaged in (True, False):
            with self.subTest(packaged=packaged):
                data = self.root / ("packaged" if packaged else "downloaded")
                cache = data / "flightdeck/components"
                cache.mkdir(parents=True, mode=0o700)
                (cache / "runner.tar.xz").write_bytes(runner_archive)
                plan = bootstrap.BootstrapPlan({}, self.root / "runtime", self.lock, self.source,
                                                self.native if packaged else None)
                def extract(archive, output, _cancel):
                    self.assertIsNone(transfers[-1])
                    if output.name == "artifacts":
                        for name in self.lock["native"]["files"]:
                            bootstrap._copy_file(self.native / name, output / name)
                    else:
                        file = output / "runner/files/lib/wine/x86_64-windows/xgameruntime.dll"
                        file.parent.mkdir(parents=True)
                        file.write_bytes(original)
                transfers = []
                def download(*args, progress=None, **kwargs):
                    progress({"kind": "components", "received_bytes": 7, "verified_bytes": 7, "total_bytes": 7,
                              "completed_files": None, "total_files": None})
                    return cache / "fixture"
                with patch.object(bootstrap, "data_home", return_value=data), \
                     patch.object(bootstrap, "download", side_effect=download) as fetch, \
                     patch.object(bootstrap, "extract", side_effect=extract), \
                     patch.object(bootstrap, "prepare_prefix", side_effect=lambda *a, **k: self.assertIsNone(transfers[-1])):
                    result = bootstrap.bootstrap(plan, transfer=transfers.append)
                    manifest = json.loads((result["artifacts_path"] / "manifest.json").read_text())
                    self.assertEqual(manifest["features"], [CONNECTED_STORAGE_FEATURE])
                    bootstrap.verify_native(result["artifacts_path"], self.lock)
                    bootstrap.verify_native(bootstrap.cached_native_path(self.lock), self.lock)
                    self.assertEqual(fetch.call_count, 1 if packaged else 2)
                    self.assertIsNone(transfers[0])
                    self.assertEqual(len([value for value in transfers if value]), fetch.call_count)

    def test_preflight_is_read_only_and_rejects_insufficient_space(self):
        data = {"destination_path": str(self.root / "runtime"), "market": "AT", "local_saves": True}
        with patch.object(bootstrap, "__file__", str(self.source / "flightdeck/bootstrap.py")), patch.object(bootstrap.platform, "system", return_value="Linux"), patch.object(bootstrap.platform, "machine", return_value="x86_64"), patch.object(bootstrap.platform, "libc_ver", return_value=("glibc", "2.40")), patch.object(bootstrap.shutil, "disk_usage", return_value=SimpleNamespace(free=120 * 1024**3)), patch.dict(bootstrap.os.environ, {"DISPLAY": ":synthetic", "DBUS_SESSION_BUS_ADDRESS": "synthetic"}), patch.object(bootstrap.ctypes.util, "find_library", return_value="synthetic"), patch.object(bootstrap.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run:
            with patch.object(bootstrap, "check_media") as media:
                plan = bootstrap.preflight(data, source_root=self.source)
            media.assert_called_once()
            self.assertEqual(plan.destination, self.root / "runtime")
            self.assertFalse(plan.destination.exists())
            self.assertEqual(run.call_args.args[0], [str(self.native / "bin/xodus-cli"), "--help"])
            with patch.object(bootstrap.shutil, "disk_usage", return_value=SimpleNamespace(free=1)):
                with self.assertRaisesRegex(SetupError, "100 GiB"):
                    bootstrap.preflight(data, source_root=self.source)

    def test_media_probes_fixed_plugins_with_temporary_registry(self):
        with patch.object(bootstrap.shutil, "which", return_value="/fixture/gst-inspect-1.0"), \
             patch.object(bootstrap.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run, \
             patch.dict(bootstrap.os.environ, {"GST_PLUGIN_SYSTEM_PATH_1_0": "/fixture/custom-plugins", "GST_PLUGIN_PATH": "/fixture/extra"}):
            bootstrap.check_media()
        self.assertEqual([call.args[0] for call in run.call_args_list],
                         [["/fixture/gst-inspect-1.0", plugin] for plugin in ("qtdemux", "h264parse", "avdec_h264")])
        for call in run.call_args_list:
            env = call.kwargs["env"]
            self.assertNotIn("GST_PLUGIN_SYSTEM_PATH_1_0", env)
            self.assertEqual(env["GST_PLUGIN_PATH"], "")
            self.assertFalse(Path(env["GST_REGISTRY_1_0"]).parent.exists())
            self.assertEqual(call.kwargs["stdout"], bootstrap.subprocess.DEVNULL)
            self.assertEqual(call.kwargs["stderr"], bootstrap.subprocess.DEVNULL)
            self.assertEqual(call.kwargs["timeout"], 5)

    def test_media_missing_tool_plugin_timeout_and_cancel_are_errors(self):
        with patch.object(bootstrap.shutil, "which", return_value=None), patch.object(bootstrap.subprocess, "run") as run:
            with self.assertRaisesRegex(SetupError, "gst-inspect-1.0"):
                bootstrap.check_media()
            run.assert_not_called()
        with patch.object(bootstrap.shutil, "which", return_value="/fixture/gst-inspect-1.0"), \
             patch.object(bootstrap.subprocess, "run", side_effect=[SimpleNamespace(returncode=1), SimpleNamespace(returncode=0), bootstrap.subprocess.TimeoutExpired("fixture", 5)]):
            with self.assertRaises(SetupError) as failed:
                bootstrap.check_media()
            self.assertIn("qtdemux, avdec_h264", str(failed.exception))
            self.assertNotIn("h264parse", str(failed.exception))
        cancelled = threading.Event()
        cancelled.set()
        with patch.object(bootstrap.subprocess, "run") as run:
            with self.assertRaises(SetupCancelled):
                bootstrap.check_media(cancel=cancelled)
            run.assert_not_called()

    def test_download_verifies_before_publishing_and_reuses_correct_cache(self):
        target = self.root / "component.tar"
        payload = b"synthetic downloaded archive"
        response = io.BytesIO(payload)
        response.url = "https://example.invalid/component"
        with patch.object(bootstrap.urllib.request, "build_opener") as create:
            create.return_value.open.return_value = response
            result = bootstrap.download(response.url, hashlib.sha256(payload).hexdigest(), target)
            self.assertEqual(result.read_bytes(), payload)
            bootstrap.download(response.url, hashlib.sha256(payload).hexdigest(), target)
            self.assertEqual(create.return_value.open.call_count, 1)
        self.assertEqual(list(self.root.glob(".download-*")), [])

    def test_bad_checksum_preserves_existing_cache_and_removes_temporary(self):
        target = self.root / "component.tar"
        target.write_bytes(b"previous")
        response = io.BytesIO(b"wrong")
        response.url = "https://example.invalid/component"
        with patch.object(bootstrap.urllib.request, "build_opener") as create:
            create.return_value.open.return_value = response
            with self.assertRaisesRegex(SetupError, "Prüfsumme"):
                bootstrap.download(response.url, "0" * 64, target)
        self.assertEqual(target.read_bytes(), b"previous")
        self.assertEqual(list(self.root.glob(".download-*")), [])

    def test_download_cancel_keeps_no_partial_target(self):
        cancel = threading.Event()
        cancel.set()
        response = io.BytesIO(b"payload")
        response.url = "https://example.invalid/component"
        with patch.object(bootstrap.urllib.request, "build_opener") as create:
            create.return_value.open.return_value = response
            with self.assertRaises(SetupCancelled):
                bootstrap.download(response.url, "0" * 64, self.root / "target", cancel=cancel)
        self.assertFalse((self.root / "target").exists())
        self.assertEqual(list(self.root.glob(".download-*")), [])

    def response(self, payload, lengths=()):
        response = io.BytesIO(payload)
        response.url = "https://example.invalid/component"
        response.headers = Message()
        for value in lengths:
            response.headers.add_header("Content-Length", value)
        return response

    def test_download_progress_is_throttled_and_finishes_only_after_publication(self):
        payload = b"abcdefghijklmnopqrst"
        response = self.response(payload, [str(len(payload))])
        read = response.read
        response.read = lambda _maximum: read(4)
        target = self.root / "archive"
        updates = []
        def progress(value):
            self.assertEqual(set(value), {"kind", "received_bytes", "verified_bytes", "total_bytes", "completed_files", "total_files"})
            self.assertEqual(value["kind"], "components")
            self.assertIsNone(value["completed_files"])
            self.assertIsNone(value["total_files"])
            if value["received_bytes"] == len(payload):
                self.assertEqual(target.read_bytes(), payload)
                self.assertEqual(value["verified_bytes"], len(payload))
            else:
                self.assertFalse(target.exists())
                self.assertEqual(value["verified_bytes"], 0)
            updates.append(value)
        with patch.object(bootstrap.urllib.request, "build_opener") as opener, \
             patch.object(bootstrap.time, "monotonic", side_effect=[0, .1, .3, .5, .7, 1]):
            opener.return_value.open.return_value = response
            bootstrap.download(response.url, hashlib.sha256(payload).hexdigest(), target, progress=progress)
        self.assertEqual([value["received_bytes"] for value in updates], [0, 12, 20])
        self.assertEqual([value["total_bytes"] for value in updates], [20, 20, 20])

    def test_missing_ambiguous_or_invalid_lengths_remain_unknown(self):
        payload = b"verified synthetic archive"
        cases = [(), ("",), ("0",), ("-1",), ("+12",), (" 12 ",), ("12,12",),
                 ("12", "12"), (str(2 * 1024**3 + 1),), ("9" * 1000,)]
        for index, lengths in enumerate(cases):
            with self.subTest(lengths=index):
                response = self.response(payload, lengths)
                updates = []
                with patch.object(bootstrap.urllib.request, "build_opener") as opener:
                    opener.return_value.open.return_value = response
                    bootstrap.download(response.url, hashlib.sha256(payload).hexdigest(), self.root / str(index), progress=updates.append)
                self.assertEqual(updates[0]["received_bytes"], 0)
                self.assertEqual(updates[-1]["received_bytes"], len(payload))
                self.assertEqual(updates[-1]["verified_bytes"], len(payload))
                self.assertTrue(all(value["total_bytes"] is None for value in updates))
        response = self.response(payload, [str(len(payload))])
        response.headers["Transfer-Encoding"] = "chunked"
        self.assertIsNone(bootstrap._content_length(response))

    def test_incorrect_length_is_not_used_for_a_false_final_percentage(self):
        payload = b"verified payload"
        for length in (1, len(payload) + 100):
            response = self.response(payload, [str(length)])
            updates = []
            with patch.object(bootstrap.urllib.request, "build_opener") as opener:
                opener.return_value.open.return_value = response
                bootstrap.download(response.url, hashlib.sha256(payload).hexdigest(), self.root / str(length), progress=updates.append)
            self.assertEqual(updates[-1]["received_bytes"], len(payload))
            self.assertIsNone(updates[-1]["total_bytes"])

    def test_verified_cache_reports_exact_size_without_a_network_call(self):
        target = self.root / "cached"
        target.write_bytes(b"verified cached archive")
        size = target.stat().st_size
        updates = []
        with patch.object(bootstrap.urllib.request, "build_opener") as opener:
            bootstrap.download("https://example.invalid/component", hashlib.sha256(target.read_bytes()).hexdigest(), target, progress=updates.append)
        opener.assert_not_called()
        self.assertEqual(updates, [{"kind": "components", "received_bytes": size, "verified_bytes": size, "total_bytes": size,
                                    "completed_files": None, "total_files": None}])

    def test_bad_hash_and_oversize_never_emit_verified_completion(self):
        payload = b"not trusted yet"
        for oversize in (False, True):
            with self.subTest(oversize=oversize):
                response = self.response(payload, [str(len(payload))])
                updates = []
                with patch.object(bootstrap.urllib.request, "build_opener") as opener, \
                     patch.object(bootstrap, "_MAX_COMPONENT_BYTES", 4 if oversize else 2 * 1024**3):
                    opener.return_value.open.return_value = response
                    with self.assertRaises(SetupError):
                        bootstrap.download(response.url, "0" * 64, self.root / "rejected", progress=updates.append)
                self.assertEqual([value["received_bytes"] for value in updates], [0])
                self.assertTrue(all(value["verified_bytes"] == 0 for value in updates))
                self.assertFalse((self.root / "rejected").exists())
                self.assertEqual(list(self.root.glob(".download-*")), [])

    def test_cancel_during_transfer_preserves_previous_cache_without_final_update(self):
        target = self.root / "cached"
        target.write_bytes(b"previous cached bytes")
        payload = b"abcdefgh"
        response = self.response(payload, [str(len(payload))])
        read = response.read
        response.read = lambda _maximum: read(4)
        updates, cancel = [], threading.Event()
        def progress(value):
            updates.append(value)
            if value["received_bytes"]:
                cancel.set()
        with patch.object(bootstrap.urllib.request, "build_opener") as opener, \
             patch.object(bootstrap.time, "monotonic", side_effect=[0, 1]):
            opener.return_value.open.return_value = response
            with self.assertRaises(SetupCancelled):
                bootstrap.download(response.url, hashlib.sha256(payload).hexdigest(), target, cancel=cancel, progress=progress)
        self.assertEqual([value["received_bytes"] for value in updates], [0, 4])
        self.assertTrue(all(value["verified_bytes"] == 0 for value in updates))
        self.assertEqual(target.read_bytes(), b"previous cached bytes")
        self.assertEqual(list(self.root.glob(".download-*")), [])

    def test_redirect_downgrade_rejected_before_following(self):
        request = urllib.request.Request("https://example.invalid/start")
        handler = bootstrap.HTTPSOnlyRedirect()
        with self.assertRaisesRegex(SetupError, "unsichere"):
            handler.redirect_request(request, None, 302, "Found", {}, "http://example.invalid/unsafe")
        redirected = handler.redirect_request(request, None, 302, "Found", {}, "https://example.invalid/safe")
        self.assertEqual(redirected.full_url, "https://example.invalid/safe")

    def archive(self, name, kind=None, link=None):
        file = self.root / "fixture.tar"
        with tarfile.open(file, "w") as archive:
            info = tarfile.TarInfo(name)
            if kind:
                info.type = kind
            if link:
                info.linkname = link
            if kind is None:
                info.size = 4
                archive.addfile(info, io.BytesIO(b"data"))
            else:
                archive.addfile(info)
        return file

    def test_tar_traversal_and_external_symlink_cannot_escape(self):
        output = self.root / "out"
        output.mkdir()
        for name, kind, link in (("../escaped", None, None), ("link", tarfile.SYMTYPE, "../../outside"), ("fifo", tarfile.FIFOTYPE, None)):
            with self.subTest(name=name):
                file = self.archive(name, kind, link)
                with self.assertRaises((SetupError, tarfile.FilterError)):
                    bootstrap.extract(file, output)
        self.assertFalse((self.root / "escaped").exists())
        self.assertFalse((output / "link").exists())

    def test_tar_normal_file_and_pre_cancel(self):
        archive = self.archive("folder/file")
        output = self.root / "out"
        output.mkdir()
        bootstrap.extract(archive, output)
        self.assertEqual((output / "folder/file").read_bytes(), b"data")
        cancel = threading.Event()
        cancel.set()
        with self.assertRaises(SetupCancelled):
            bootstrap.extract(archive, self.root / "cancelled", cancel)
        self.assertFalse((self.root / "cancelled").exists())

    def test_dangling_prefix_link_is_rejected_before_wine(self):
        prefix = self.root / "prefix"
        outside = self.root / "outside"
        prefix.symlink_to(outside)
        with patch.object(bootstrap, "_command") as command, patch.object(bootstrap.subprocess, "run") as run:
            with self.assertRaisesRegex(SetupError, "existiert bereits"):
                bootstrap.prepare_prefix(self.root / "runner", prefix)
        command.assert_not_called()
        run.assert_not_called()
        self.assertTrue(prefix.is_symlink())
        self.assertFalse(outside.exists())

    def test_prefix_graphics_replaces_file_links_without_changing_targets(self):
        runner, prefix = self.root / "runner", self.root / "prefix"
        outside = self.root / "outside.dll"
        outside.write_bytes(b"unrelated")
        for arch in ("x86_64-windows", "i386-windows"):
            for group, names in (("dxvk", ("dxgi", "d3d11", "d3d10core")), ("vkd3d-proton", ("d3d12", "d3d12core"))):
                for name in names:
                    path = runner / "files/lib/wine" / group / arch / (name + ".dll")
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(b"verified synthetic graphics")
        def command(arguments, **kwargs):
            if arguments[1] == "wineboot":
                for folder in ("system32", "syswow64"):
                    path = prefix / "drive_c/windows" / folder
                    path.mkdir(parents=True)
                    (path / "dxgi.dll").symlink_to(outside)
                for name in ("system.reg", "user.reg"):
                    (prefix / name).write_text("synthetic registry")
        with patch.object(bootstrap, "_command", side_effect=command), patch.object(bootstrap.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as kill:
            bootstrap.prepare_prefix(runner, prefix)
        self.assertEqual(outside.read_bytes(), b"unrelated")
        self.assertFalse((prefix / "drive_c/windows/system32/dxgi.dll").is_symlink())
        self.assertEqual((prefix / "drive_c/windows/system32/dxgi.dll").read_bytes(), b"verified synthetic graphics")
        self.assertEqual(kill.call_args.kwargs["env"]["WINEPREFIX"], str(prefix))


if __name__ == "__main__":
    unittest.main()

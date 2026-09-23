# SPDX-License-Identifier: MIT
"""Backend regression tests using temporary, synthetic runtimes only.

No installed games, account stores, network requests, or real diagnostic logs
are used. The tiny launcher below owns an ordinary flock and handles SIGTERM.
"""
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from flightdeck import backend


FAKE_LAUNCHER = r'''
import fcntl, os, signal, sys, time
from pathlib import Path
root = Path(__file__).resolve().parents[1]
with (root / "private/play.lock").open("a") as lock:
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        sys.exit(19)
    def stop(signum, frame):
        (root / "private/stopped").write_text(str(signum))
        sys.exit(0)
    signal.signal(signal.SIGTERM, stop)
    (root / "private/ready").write_text(str(os.getpid()))
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        time.sleep(0.01)
'''


class BackendTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="flightdeck-test-")
        self.base = Path(self.temp.name)
        self.runtime = self.make_runtime("runtime")
        self.state = self.base / "state"
        self.launcher = backend.Launcher(self.state, str(self.runtime))
        self.children = []

    def tearDown(self):
        children = self.children + [self.launcher.process]
        for process in children:
            if process is not None and process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=3)
        self.temp.cleanup()

    def make_runtime(self, name):
        root = self.base / name
        for relative in (
            "tools/play-msfs.sh",
            "games/MSFS2024/FlightSimulator2024.exe",
            "local/msfs-prefix/system.reg",
            "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll",
        ):
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("synthetic fixture, not game content\n")
        script = root / "tools/play-msfs.sh"
        script.write_text("#!" + sys.executable + "\n" + FAKE_LAUNCHER)
        script.chmod(0o700)
        (root / "private").mkdir(mode=0o700)
        return root

    def enable_saves(self):
        folder = self.runtime / "private/local-saves"
        folder.mkdir()
        (self.runtime / "private/local-saves.enabled").write_text("enabled\n")
        (folder / "namespace").mkdir()
        (folder / "namespace/state.bin").write_bytes(b"synthetic saved state\x00\xff")
        return folder

    def write_log(self, text, suffix="20260101-120000-Synthetic"):
        run = self.runtime / "private" / ("run-" + suffix)
        run.mkdir()
        (run / "game.log").write_text(text, encoding="utf-8")
        return run

    def wait_for(self, predicate, timeout=3):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(0.01)
        self.fail("Synthetic process did not reach the expected state")

    @contextmanager
    def external_lock(self):
        with (self.runtime / "private/play.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            yield

    def test_unconfigured_status_is_stopped_and_not_ready(self):
        status = backend.Launcher(self.base / "unconfigured").status()
        self.assertFalse(status["runtime"]["configured"])
        self.assertFalse(status["game"]["can_start"])
        self.assertFalse(status["game"]["can_stop"])
        self.assertFalse(status["saves"]["can_backup"])

    def test_config_and_state_are_private_and_reloadable(self):
        restored = backend.Launcher(self.state)
        self.assertEqual(restored.runtime, self.runtime)
        self.assertEqual(self.state.stat().st_mode & 0o777, 0o700)
        self.assertEqual((self.state / "config.json").stat().st_mode & 0o777, 0o600)

    def test_corrupt_or_wrong_shape_config_is_ignored(self):
        for text in ("not-json", "null", "[]", "true", "42", '"text"', "{}",
                     '{"runtime_path": []}', '{"runtime_path": null}'):
            with self.subTest(text=text):
                (self.state / "config.json").write_text(text)
                restored = backend.Launcher(self.state)
                self.assertIsNone(restored.runtime)
                self.assertFalse(restored.status()["game"]["can_start"])

    def test_invalid_runtime_values_leave_configuration_unchanged(self):
        original = (self.state / "config.json").read_bytes()
        for value in (None, [], "", "relative/runtime", str(self.base / "missing")):
            with self.subTest(value=value), self.assertRaises(backend.LauncherError):
                self.launcher.configure(value)
        self.assertEqual((self.state / "config.json").read_bytes(), original)
        self.assertEqual(self.launcher.runtime, self.runtime)

    def test_missing_runtime_piece_prevents_launch(self):
        (self.runtime / "local/msfs-prefix/system.reg").unlink()
        self.assertFalse(self.launcher.status()["game"]["can_start"])
        with self.assertRaises(backend.LauncherError):
            self.launcher.launch()
        self.assertIsNone(self.launcher.process)

    def test_external_flock_prevents_start_stop_configure_and_backup(self):
        self.enable_saves()
        other = self.make_runtime("other")
        with self.external_lock():
            status = self.launcher.status()
            self.assertEqual(status["game"]["state"], "external")
            self.assertFalse(status["game"]["managed"])
            self.assertFalse(status["game"]["can_stop"])
            self.assertFalse(status["saves"]["can_backup"])
            for operation in (self.launcher.launch, self.launcher.stop,
                              self.launcher.backup, lambda: self.launcher.configure(str(other))):
                with self.assertRaises(backend.LauncherError):
                    operation()
        self.assertTrue(self.launcher.status()["game"]["can_start"])

    def test_external_launcher_is_not_signalled(self):
        child = subprocess.Popen([str(self.runtime / "tools/play-msfs.sh")],
                                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL, start_new_session=True)
        self.children.append(child)
        self.wait_for(lambda: (self.runtime / "private/ready").exists())
        self.assertEqual(self.launcher.status()["game"]["state"], "external")
        with self.assertRaises(backend.LauncherError):
            self.launcher.stop()
        self.assertIsNone(child.poll())
        self.assertFalse((self.runtime / "private/stopped").exists())

    def test_owned_launch_stop_and_exit_status(self):
        self.enable_saves()
        self.assertTrue(self.launcher.launch()["ok"])
        child = self.launcher.process
        self.children.append(child)
        self.wait_for(lambda: (self.runtime / "private/ready").exists())
        status = self.launcher.status()
        self.assertTrue(status["game"]["managed"])
        self.assertTrue(status["game"]["can_stop"])
        self.assertFalse(status["saves"]["can_backup"])
        self.launcher.started_monotonic -= 16
        self.assertEqual(self.launcher.status()["game"]["state"], "running")
        with self.assertRaises(backend.LauncherError):
            self.launcher.backup()
        self.assertTrue(self.launcher.stop()["ok"])
        child.wait(timeout=3)
        status = self.launcher.status()
        self.assertEqual(status["game"]["state"], "stopped")
        self.assertFalse(status["game"]["managed"])
        self.assertEqual(status["game"]["exit_code"], 0)
        self.assertEqual((self.runtime / "private/stopped").read_text(), str(int(signal.SIGTERM)))
        self.assertTrue(status["saves"]["can_backup"])

    def test_competing_flock_after_preflight_is_not_inherited_or_stopped(self):
        # A different launcher wins between the UI preflight and exec. The
        # synthetic child must see that lock and fail without owning the game.
        real_popen = subprocess.Popen
        with (self.runtime / "private/play.lock").open("a") as lock:
            def compete(*args, **kwargs):
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                return real_popen(*args, **kwargs)
            with patch.object(backend.subprocess, "Popen", side_effect=compete):
                self.launcher.launch()
            child = self.launcher.process
            self.children.append(child)
            child.wait(timeout=3)
            status = self.launcher.status()
            self.assertEqual(status["game"]["state"], "external")
            self.assertEqual(status["game"]["exit_code"], 19)
            self.assertFalse(status["game"]["can_stop"])
            with self.assertRaises(backend.LauncherError):
                self.launcher.stop()

    def test_spawn_failure_has_safe_error_and_no_managed_process(self):
        with patch.object(backend.subprocess, "Popen", side_effect=OSError("synthetic private detail")):
            with self.assertRaises(backend.LauncherError) as result:
                self.launcher.launch()
        self.assertNotIn("synthetic private detail", str(result.exception))
        self.assertFalse(self.launcher.status()["game"]["managed"])

    def test_lock_symlink_does_not_offer_start_or_backup(self):
        self.enable_saves()
        foreign = self.base / "foreign-lock"
        foreign.write_text("unchanged")
        (self.runtime / "private/play.lock").symlink_to(foreign)
        status = self.launcher.status()
        self.assertFalse(status["game"]["can_start"])
        self.assertFalse(status["saves"]["can_backup"])
        self.assertEqual(foreign.read_text(), "unchanged")

    def test_unreadable_runtime_lock_does_not_offer_start_or_backup(self):
        self.enable_saves()
        with patch.object(self.launcher, "runtime_lock", side_effect=PermissionError("synthetic private detail")):
            status = self.launcher.status()
        self.assertFalse(status["game"]["can_start"])
        self.assertFalse(status["saves"]["can_backup"])
        self.assertNotIn("synthetic private detail", json.dumps(status))

    def test_read_only_diagnostics_never_return_raw_identity_or_tokens(self):
        secrets = ("synthetic-bearer-secret", "synthetic-account-identifier",
                   "synthetic-user-display-name", "synthetic-url-query-secret")
        log = "\n".join((
            "Authorization: Bearer " + secrets[0],
            "xuid=" + secrets[1] + " gamertag=" + secrets[2],
            "https://example.test/path?token=" + secrets[3],
            "xodus-title-auth: host=user.auth.xboxlive.com status=200",
            "xodus-title-auth: host=xsts.auth.xboxlive.com status=401",
            "xodus-title-auth: host=user.auth.xboxlive.com status=200 token=" + secrets[0],
            "[xodus-gamesave] local_init enabled=1 sync_on_demand=0 hr=00000000",
            "[xodus-gamesave] local_init enabled=1 sync_on_demand=1 hr=80004001",
            "[xodus-store] XStoreShowPurchaseUIAsync store_id=ABCD1234EFGH hr=80004001 token=" + secrets[0],
            "[xodus-store] XStoreAcquireLicenseForDurablesAsync store_id=ABCD1234EFGH hr=80004001",
            "[xodus-store] XStoreQueryGameAndDlcPackageUpdatesAsync hr=80004001",
            "[xodus-store-query] kind=1 hr=80004001 account=" + secrets[1],
            "[xodus-store-query] kind=4 hr=00000000 token=" + secrets[0],
            "[xodus-store-query] kind=6 hr=00000000 account=" + secrets[1],
            "[xodus-store-query] kind=7 hr=00000000",
            "[xodus-store-query] kind=9 hr=00000000",
            "xodus-wine-launch: wine_pid=123 exit_code=0 elapsed_seconds=12.5",
        ))
        run = self.write_log(log)
        before = (run / "game.log").read_bytes()
        result = self.launcher.diagnostics()
        encoded = json.dumps(result)
        for secret in secrets:
            self.assertNotIn(secret, encoded)
        self.assertNotIn(str(run), encoded)
        self.assertEqual(result["summary"]["auth_http"], [200, 401])
        self.assertEqual(result["summary"]["local_save_init"], [
            {"enabled": 1, "sync_on_demand": 0, "hresult": "00000000"},
            {"enabled": 1, "sync_on_demand": 1, "hresult": "80004001"},
        ])
        self.assertEqual(result["summary"]["store_calls"], [
            {"method": "XStoreAcquireLicenseForDurablesAsync", "hresult": "00000000"},
            {"method": "XStoreAcquireLicenseForDurablesAsync", "hresult": "80004001"},
            {"method": "XStoreQueryEntitledProductsAsync", "hresult": "80004001"},
            {"method": "XStoreQueryGameAndDlcPackageUpdatesAsync", "hresult": "00000000"},
            {"method": "XStoreQueryGameAndDlcPackageUpdatesAsync", "hresult": "80004001"},
            {"method": "XStoreQueryProductsAsync", "hresult": "00000000"},
            {"method": "XStoreShowPurchaseUIAsync", "hresult": "80004001"},
        ])
        self.assertEqual(result["summary"]["exit"], {"code": 0, "seconds": 12.5})
        self.assertEqual((run / "game.log").read_bytes(), before)

    def test_diagnostics_choose_latest_valid_run_and_ignore_symlinks(self):
        self.write_log("xodus-title-auth: host=user.auth.xboxlive.com status=401")
        self.write_log("xodus-title-auth: host=user.auth.xboxlive.com status=200", "20260102-120000-New")
        outside = self.base / "outside"
        outside.mkdir()
        (outside / "game.log").write_text("xodus-title-auth: host=user.auth.xboxlive.com status=599")
        (self.runtime / "private/run-20260103-120000-Link").symlink_to(outside, target_is_directory=True)
        self.write_log("xodus-title-auth: host=user.auth.xboxlive.com status=598", "invalid-name")
        self.assertEqual(self.launcher.diagnostics()["summary"]["auth_http"], [200])

    def test_diagnostics_do_not_follow_game_log_symlink(self):
        run = self.write_log("")
        (run / "game.log").unlink()
        target = self.base / "other.log"
        target.write_text("xodus-title-auth: host=user.auth.xboxlive.com status=599")
        (run / "game.log").symlink_to(target)
        summary = self.launcher.diagnostics()["summary"]
        self.assertFalse(summary["run_found"])
        self.assertEqual(summary["auth_http"], [])

    def test_diagnostics_fifo_is_rejected_without_blocking(self):
        run = self.write_log("")
        (run / "game.log").unlink()
        os.mkfifo(run / "game.log")
        # Bound the regression even against the previous blocking os.open.
        code = (
            "from pathlib import Path; from flightdeck.backend import Launcher; "
            "import json,sys; "
            "print(json.dumps(Launcher(Path(sys.argv[1])).diagnostics()['summary']))"
        )
        try:
            result = subprocess.run([sys.executable, "-c", code, str(self.state)],
                                    cwd=Path(__file__).resolve().parents[1],
                                    capture_output=True, text=True, timeout=2, check=True)
        except subprocess.TimeoutExpired:
            self.fail("Diagnostics blocked opening a non-regular synthetic log")
        summary = json.loads(result.stdout)
        self.assertFalse(summary["run_found"])
        self.assertEqual(summary["auth_http"], [])

    def test_diagnostics_survive_malformed_elapsed_value(self):
        self.write_log("xodus-wine-launch: wine_pid=123 exit_code=0 elapsed_seconds=1..2\n")
        self.assertIsNone(self.launcher.diagnostics()["summary"]["exit"])

    def test_diagnostics_read_bounded_head_and_tail(self):
        run = self.write_log("xodus-title-auth: host=user.auth.xboxlive.com status=200\n")
        with (run / "game.log").open("a") as stream:
            stream.write("x" * (2 * 1024 * 1024))
            stream.write("\nxodus-title-auth: host=user.auth.xboxlive.com status=599\n")
            stream.write("x" * (2 * 1024 * 1024))
            stream.write("\nxodus-wine-launch: wine_pid=123 exit_code=7 elapsed_seconds=2.25\n")
        summary = self.launcher.diagnostics()["summary"]
        self.assertEqual(summary["auth_http"], [200])
        self.assertEqual(summary["exit"], {"code": 7, "seconds": 2.25})

    def test_backup_copies_bytes_hashes_permissions_and_no_symlinks(self):
        source = self.enable_saves()
        (source / "namespace/session.lock").write_text("lock")
        (source / "namespace/.tmp-uncommitted").write_text("temporary")
        foreign = self.base / "foreign"
        foreign.mkdir()
        (foreign / "private.bin").write_text("must not enter backup")
        (source / "linked-dir").symlink_to(foreign, target_is_directory=True)
        (source / "linked-file").symlink_to(foreign / "private.bin")
        os.mkfifo(source / "fifo")
        before = (source / "namespace/state.bin").read_bytes()
        result = self.launcher.backup()
        self.assertEqual(result["files"], 1)
        dest = self.runtime / "private/save-backups" / result["backup"]["name"]
        manifest = json.loads((dest / "manifest.json").read_text())
        self.assertEqual(manifest["files"], {"namespace/state.bin": hashlib.sha256(before).hexdigest()})
        self.assertEqual((dest / "data/namespace/state.bin").read_bytes(), before)
        self.assertEqual((dest / "data/namespace/state.bin").stat().st_mode & 0o777, 0o600)
        self.assertEqual(dest.stat().st_mode & 0o777, 0o700)
        self.assertEqual((dest / "manifest.json").stat().st_mode & 0o777, 0o600)
        self.assertEqual((source / "namespace/state.bin").read_bytes(), before)
        self.assertEqual(self.launcher.status()["saves"]["backups"], 1)

    def test_backup_requires_opt_in_and_regular_saved_content(self):
        with self.assertRaises(backend.LauncherError):
            self.launcher.backup()
        folder = self.enable_saves()
        (folder / "namespace/state.bin").unlink()
        (folder / "namespace/write.lock").write_text("not save data")
        self.assertFalse(self.launcher.status()["saves"]["can_backup"])
        with self.assertRaises(backend.LauncherError):
            self.launcher.backup()

    def test_backup_rejects_save_root_and_destination_symlinks(self):
        source = self.enable_saves()
        held = source.with_name("kept-saves")
        source.rename(held)
        source.symlink_to(held, target_is_directory=True)
        self.assertFalse(self.launcher.status()["saves"]["can_backup"])
        with self.assertRaises(backend.LauncherError):
            self.launcher.backup()
        source.unlink()
        held.rename(source)
        outside = self.base / "elsewhere"
        outside.mkdir()
        (self.runtime / "private/save-backups").symlink_to(outside, target_is_directory=True)
        with self.assertRaises(backend.LauncherError):
            self.launcher.backup()
        self.assertEqual(list(outside.iterdir()), [])

    def test_copy_failure_removes_partial_backup_and_releases_lock(self):
        self.enable_saves()
        with patch.object(backend.os, "fsync", side_effect=OSError("synthetic disk failure")):
            with self.assertRaises((OSError, backend.LauncherError)):
                self.launcher.backup()
        destination = self.runtime / "private/save-backups"
        self.assertFalse(destination.exists() and list(destination.iterdir()))
        self.assertTrue(self.launcher.backup()["ok"])

    def test_backup_rechecks_external_lock_after_status(self):
        self.enable_saves()
        previous = self.launcher.status
        with (self.runtime / "private/play.lock").open("a") as lock:
            def competing_status():
                result = previous()
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                return result
            with patch.object(self.launcher, "status", side_effect=competing_status):
                with self.assertRaises(backend.LauncherError):
                    self.launcher.backup()
        self.assertFalse((self.runtime / "private/save-backups").exists())

    def test_backup_does_not_follow_parent_symlink_swapped_after_walk(self):
        source = self.enable_saves()
        foreign = self.base / "foreign-state"
        foreign.mkdir()
        (foreign / "state.bin").write_bytes(b"must not copy this unrelated file")
        walk = backend.regular_files
        changed = False

        def replace_parent(root):
            nonlocal changed
            for path in walk(root):
                # First enumeration is status(). The second runs after the
                # staging directory exists and represents the copy-time race.
                if root == source and not changed and (self.runtime / "private/save-backups").exists():
                    (source / "namespace").rename(source / "old-namespace")
                    (source / "namespace").symlink_to(foreign, target_is_directory=True)
                    changed = True
                yield path

        with patch.object(backend, "regular_files", side_effect=replace_parent):
            try:
                self.launcher.backup()
            except (OSError, backend.LauncherError):
                pass
        self.assertTrue(changed, "The deterministic parent-symlink race was not exercised")
        destination = self.runtime / "private/save-backups"
        for path in destination.rglob("state.bin"):
            self.assertNotEqual(path.read_bytes(), b"must not copy this unrelated file")
        self.assertFalse(list(destination.glob(".backup-*")))

    def test_backup_rejects_leaf_symlink_swapped_after_walk(self):
        source = self.enable_saves()
        foreign = self.base / "foreign-file"
        foreign.write_bytes(b"unrelated bytes")
        walk = backend.regular_files
        changed = False

        def replace_leaf(root):
            nonlocal changed
            for path in walk(root):
                if root == source and not changed and (self.runtime / "private/save-backups").exists():
                    path.unlink()
                    path.symlink_to(foreign)
                    changed = True
                yield path

        with patch.object(backend, "regular_files", side_effect=replace_leaf):
            with self.assertRaises((OSError, backend.LauncherError)):
                self.launcher.backup()
        self.assertTrue(changed)
        self.assertEqual(list((self.runtime / "private/save-backups").iterdir()), [])


    def managed_fixture(self):
        from flightdeck.setup import resource_paths
        tools, _ = resource_paths()
        (self.runtime / "tools/runtime-env.sh").write_bytes((tools / "runtime-env.sh").read_bytes())
        command = r"""
import fcntl,json,os,signal,sys,time
from pathlib import Path
root=Path(__file__).resolve().parents[1]
kind='service' if 'service' in Path(__file__).name else 'game'
lock=(root/'private/play.lock').stat()
inherited=False
for item in Path('/proc/self/fd').iterdir():
    try:
        info=os.fstat(int(item.name))
        inherited |= (info.st_dev,info.st_ino)==(lock.st_dev,lock.st_ino)
    except OSError:pass
probe=os.open(root/'private/play.lock',os.O_RDONLY)
try:
    try:fcntl.flock(probe,fcntl.LOCK_EX|fcntl.LOCK_NB); blocked=False
    except BlockingIOError:blocked=True
finally:os.close(probe)
(root/('private/'+kind+'-ready')).write_text(json.dumps({'inherited':inherited,'blocked':blocked}))
def stop(signum,frame):
    (root/('private/'+kind+'-stopped')).write_text('stopped');raise SystemExit(0)
signal.signal(signal.SIGTERM,stop);signal.signal(signal.SIGINT,stop)
while not (root/'private/game-done').exists() or kind=='service':time.sleep(.01)
"""
        for name in ("launch-msfs.sh", "xodus-service.sh"):
            path = self.runtime / "tools" / name
            path.write_text("#!" + sys.executable + "\n" + command)
            path.chmod(0o700)
        shim_root = self.base / "test-bin"
        shim_root.mkdir()
        shim = shim_root / "python3"
        shim.write_text("#!" + sys.executable + "\nimport os,sys\nfrom pathlib import Path\n"
            + "if len(sys.argv)==3 and sys.argv[1]=='-' and sys.argv[2].endswith('/xodus.sock'):\n"
            + f"    raise SystemExit(0 if Path({str(self.runtime / 'private/service-ready')!r}).exists() else 1)\n"
            + f"os.execv({sys.executable!r},[{sys.executable!r},*sys.argv[1:]])\n")
        shim.chmod(0o700)
        xdg = self.base / "test-xdg"; xdg.mkdir(mode=0o700)
        return {"PATH": str(shim_root) + os.pathsep + os.environ["PATH"], "XDG_RUNTIME_DIR": str(xdg)}

    def assert_competitor_blocked(self):
        command = "import fcntl,sys; f=open(sys.argv[1],'r'); fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)"
        result = subprocess.run([sys.executable, "-c", command, str(self.runtime / "private/play.lock")],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.assertNotEqual(result.returncode, 0)

    def test_managed_shell_retains_same_lease_through_exit_and_postwork(self):
        environment = self.managed_fixture()
        environment.update(FLIGHTDECK_SOCKET_DIR="/unexpected-inherited-runtime",
                           XODUS_USER_SOCKET_SUFFIX="unexpected-inherited-runtime/xodus.sock")
        shim = self.base / "test-bin/python3"
        shim.write_text(shim.read_text().replace(
            "    raise SystemExit(0 if Path(",
            f"    Path({str(self.runtime / 'private/probed-socket')!r}).write_text(sys.argv[2])\n"
            "    raise SystemExit(0 if Path("))
        self.launcher.reserve_setup()
        self.launcher.managed_session = "synthetic-session"
        with self.launcher.runtime_lock(operation="cloud_session") as fd, patch.dict(os.environ, environment):
            with self.launcher.lock:
                child = self.launcher._spawn_reserved(fd)
            self.children.append(child)
            self.wait_for(lambda: (self.runtime / "private/game-ready").exists())
            for kind in ("service", "game"):
                value = json.loads((self.runtime / ("private/" + kind + "-ready")).read_text())
                self.assertEqual(value, {"inherited": False, "blocked": True})
            socket_id = hashlib.sha256(str(self.runtime).encode()).hexdigest()[:16]
            self.assertEqual((self.runtime / "private/probed-socket").read_text(),
                             str(Path(environment["XDG_RUNTIME_DIR"]) / f"flightdeck-{socket_id}" / "xodus.sock"))
            self.assert_competitor_blocked()
            (self.runtime / "private/game-done").touch()
            self.assertEqual(child.wait(timeout=8), 0)
            self.launcher.status()
            self.assertIs(self.launcher.process, child)
            self.assertIsNone(self.launcher.exit_code)
            self.assertTrue((self.runtime / "private/service-stopped").exists())
            self.assert_competitor_blocked()  # Post-exit sync still owns it.
            with self.assertRaises(backend.LauncherError):
                self.launcher.stop()
            with self.launcher.lock:
                self.launcher.process = None
                self.launcher.managed_session = None
                self.launcher.release_setup()
        self.assertFalse(self.launcher._external())

    def test_managed_stop_signals_only_supervisor_and_owned_children(self):
        environment = self.managed_fixture()
        self.launcher.reserve_setup(); self.launcher.managed_session = "synthetic-session"
        with self.launcher.runtime_lock(operation="cloud_session") as fd, patch.dict(os.environ, environment):
            with self.launcher.lock:
                child = self.launcher._spawn_reserved(fd)
            self.children.append(child)
            self.wait_for(lambda: (self.runtime / "private/game-ready").exists())
            self.assertTrue(self.launcher.stop()["ok"])
            child.wait(timeout=8)
            self.assertTrue((self.runtime / "private/game-stopped").exists())
            self.assertTrue((self.runtime / "private/service-stopped").exists())
            self.assert_competitor_blocked()

    def fenix_companion(self, name, label, *, other_prefix=False, stubborn=False):
        ready = self.base / (label + "-ready")
        stopped = self.base / (label + "-stopped")
        code = """
import os,signal,sys,time
from pathlib import Path
def stop(number,frame):
    Path(sys.argv[2]).write_text(str(number));raise SystemExit(0)
signal.signal(signal.SIGUSR1,stop)
signal.signal(signal.SIGTERM,signal.SIG_IGN if sys.argv[3]=='stubborn' else stop)
Path(sys.argv[1]).write_text(str(os.getpid()))
while True:time.sleep(.01)
"""
        prefix = self.base / "other-prefix" if other_prefix else self.runtime / "local/msfs-prefix"
        process = subprocess.Popen([name, "-c", code, str(ready), str(stopped), "stubborn" if stubborn else "normal"],
            executable=sys.executable, env=dict(os.environ, WINEPREFIX=str(prefix)),
            start_new_session=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.children.append(process)
        self.wait_for(ready.exists)
        return process, stopped

    def test_game_exit_closes_detached_fenix_gracefully_and_preserves_other_apps(self):
        environment = self.managed_fixture()
        companion, stopped = self.fenix_companion(r"C:\Program Files\FenixSim A320\Fenix.exe", "fenix")
        survivors = [self.fenix_companion("Fenix.exe", "other-fenix", other_prefix=True)[0],
                     self.fenix_companion("FenixApp.exe", "manager")[0],
                     self.fenix_companion("OtherAircraft.exe", "aircraft")[0]]
        wine = self.runtime / "runner/files/bin/wine"; wine.parent.mkdir(parents=True)
        arguments = self.base / "taskkill-arguments"
        wine.write_text("#!" + sys.executable + "\nimport json,os,signal,sys\nfrom pathlib import Path\n"
            + f"Path({str(arguments)!r}).write_text(json.dumps(sys.argv[1:]))\n"
            + f"os.kill({companion.pid},signal.SIGUSR1)\n")
        wine.chmod(0o700)
        self.launcher.reserve_setup(); self.launcher.managed_session = "synthetic-session"
        with self.launcher.runtime_lock(operation="cloud_session") as fd, patch.dict(os.environ, environment):
            child = self.launcher._spawn_reserved(fd); self.children.append(child)
            self.wait_for(lambda: (self.runtime / "private/game-ready").exists())
            (self.runtime / "private/game-done").touch()
            self.assertEqual(child.wait(timeout=10), 0)
            self.assertEqual(companion.wait(timeout=1), 0)
            self.assertEqual(int(stopped.read_text()), signal.SIGUSR1)
            self.assertEqual(json.loads(arguments.read_text()), ["taskkill.exe", "/IM", "fenix.exe"])
            self.assertTrue(all(process.poll() is None for process in survivors))
            self.assert_competitor_blocked()

    def test_launcher_stop_cleans_stuck_detached_fenix_before_releasing_runtime(self):
        environment = self.managed_fixture()
        companion, stopped = self.fenix_companion("FenixDisplay.exe", "display", stubborn=True)
        self.launcher.reserve_setup(); self.launcher.managed_session = "synthetic-session"
        with self.launcher.runtime_lock(operation="cloud_session") as fd, patch.dict(os.environ, environment):
            child = self.launcher._spawn_reserved(fd); self.children.append(child)
            self.wait_for(lambda: (self.runtime / "private/game-ready").exists())
            self.launcher.stop(); child.wait(timeout=12)
            self.assertEqual(companion.wait(timeout=1), -signal.SIGKILL)
            self.assertFalse(stopped.exists())
            self.assertTrue((self.runtime / "private/service-stopped").exists())
            self.assert_competitor_blocked()

    def test_game_crash_closes_detached_fenix_and_preserves_exit_code(self):
        environment = self.managed_fixture()
        game = self.runtime / "tools/launch-msfs.sh"
        game.write_text(game.read_text() + "\nraise SystemExit(42)\n")
        companion, stopped = self.fenix_companion("FenixSystem.exe", "system")
        self.launcher.reserve_setup(); self.launcher.managed_session = "synthetic-session"
        with self.launcher.runtime_lock(operation="cloud_session") as fd, patch.dict(os.environ, environment):
            child = self.launcher._spawn_reserved(fd); self.children.append(child)
            self.wait_for(lambda: (self.runtime / "private/game-ready").exists())
            (self.runtime / "private/game-done").touch()
            self.assertEqual(child.wait(timeout=10), 42)
            self.assertEqual(companion.wait(timeout=1), 0)
            self.assertEqual(int(stopped.read_text()), signal.SIGTERM)
            self.assert_competitor_blocked()

    def test_managed_legacy_default_socket_ignores_inherited_other_runtime(self):
        environment = self.managed_fixture()
        environment.update(FLIGHTDECK_SOCKET_DIR="/unexpected-inherited-runtime",
                           XODUS_USER_SOCKET_SUFFIX="unexpected-inherited-runtime/xodus.sock")
        # This is the legacy contract: runtime-env sets the runtime root but
        # never declares either socket-selection variable.
        (self.runtime / "tools/runtime-env.sh").write_text(
            'umask 077\nMSFS_LINUX_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"\n'
            'export MSFS_LINUX_ROOT\n')
        shim = self.base / "test-bin/python3"
        shim.write_text(shim.read_text().replace(
            "    raise SystemExit(0 if Path(",
            f"    Path({str(self.runtime / 'private/probed-socket')!r}).write_text(sys.argv[2])\n"
            "    raise SystemExit(0 if Path("))
        for name in ("launch-msfs.sh", "xodus-service.sh"):
            command = self.runtime / "tools" / name
            command.write_text(command.read_text().replace(
                "lock=(root/'private/play.lock').stat()",
                "(root/('private/'+kind+'-socket-env')).write_text(json.dumps({"
                "'directory':os.environ.get('FLIGHTDECK_SOCKET_DIR'),"
                "'suffix':os.environ.get('XODUS_USER_SOCKET_SUFFIX')}))\n"
                "lock=(root/'private/play.lock').stat()"))
        self.launcher.reserve_setup()
        self.launcher.managed_session = "synthetic-legacy-session"
        with self.launcher.runtime_lock(operation="cloud_session") as fd, patch.dict(os.environ, environment):
            with self.launcher.lock:
                child = self.launcher._spawn_reserved(fd)
            self.children.append(child)
            self.wait_for(lambda: (self.runtime / "private/game-ready").exists())
            self.assertEqual((self.runtime / "private/probed-socket").read_text(),
                             str(Path(environment["XDG_RUNTIME_DIR"]) / "xodus.sock"))
            for kind in ("game", "service"):
                observed = json.loads((self.runtime / f"private/{kind}-socket-env").read_text())
                self.assertEqual(observed, {"directory": None, "suffix": None})
            self.assert_competitor_blocked()
            self.launcher.stop()
            child.wait(timeout=8)
            self.assertEqual(child.returncode, 0)
            self.assertTrue((self.runtime / "private/game-stopped").exists())
            self.assertTrue((self.runtime / "private/service-stopped").exists())
            self.assert_competitor_blocked()

    def test_managed_custom_suffix_without_socket_directory_is_not_guessed(self):
        environment = self.managed_fixture()
        (self.runtime / "tools/runtime-env.sh").write_text(
            'umask 077\nMSFS_LINUX_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"\n'
            'export MSFS_LINUX_ROOT XODUS_USER_SOCKET_SUFFIX=custom-runtime/xodus.sock\n')
        self.launcher.reserve_setup()
        self.launcher.managed_session = "synthetic-unsupported-socket"
        with self.launcher.runtime_lock() as fd, patch.dict(os.environ, environment):
            with self.launcher.lock:
                child = self.launcher._spawn_reserved(fd)
            self.children.append(child)
            self.assertEqual(child.wait(timeout=3), 2)
            self.assertFalse((self.runtime / "private/game-ready").exists())
            self.assertFalse((self.runtime / "private/service-ready").exists())
            self.assert_competitor_blocked()

    def test_managed_spawn_requires_reservation_and_exact_held_descriptor(self):
        with self.launcher.runtime_lock() as fd:
            with self.assertRaises(backend.LauncherError):self.launcher._spawn_reserved(fd)
        self.launcher.reserve_setup(); self.launcher.managed_session = "synthetic-session"
        with (self.runtime / "private/play.lock").open("r") as unlocked:
            with self.assertRaises(backend.LauncherError):self.launcher._spawn_reserved(unlocked.fileno())
        self.managed_fixture()
        with self.launcher.runtime_lock() as owned, (self.runtime / "private/play.lock").open("r") as other:
            with self.assertRaises(backend.LauncherError):self.launcher._spawn_reserved(other.fileno())
            self.assert_competitor_blocked()
        self.assertIsNone(self.launcher.process)

    def test_managed_spawn_unknown_runtime_fails_without_running_legacy_script(self):
        self.launcher.reserve_setup(); self.launcher.managed_session = "synthetic-session"
        with self.launcher.runtime_lock() as fd:
            with self.assertRaises(backend.LauncherError):self.launcher._spawn_reserved(fd)
            self.assert_competitor_blocked()
        self.assertFalse((self.runtime / "private/ready").exists())

    def test_launch_delegates_and_never_falls_back_after_coordinator_error(self):
        with patch.object(self.launcher.cloud_saves, "launch", return_value={"ok": True, "session": "synthetic"}, create=True) as auto:
            self.assertEqual(self.launcher.launch(), {"ok": True, "session": "synthetic"})
            auto.assert_called_once()
        with patch.object(self.launcher.cloud_saves, "launch", side_effect=backend.LauncherError("synthetic"), create=True):
            with self.assertRaises(backend.LauncherError):self.launcher.launch()
        self.assertIsNone(self.launcher.process)

    def test_reserved_backup_offline_empty_and_complete_without_new_runtime_lock(self):
        self.launcher.reserve_setup(); self.launcher.managed_session = "synthetic-session"
        with self.launcher.runtime_lock() as fd:
            self.assertIsNone(self.launcher._backup_reserved(fd))
            self.enable_saves()
            with patch.object(self.launcher, "runtime_lock", side_effect=AssertionError("nested lease")):
                result = self.launcher._backup_reserved(fd)
            self.assertEqual(result["files"], 1)
            path = self.runtime / "private/save-backups" / result["backup"]["name"]
            self.assertEqual((path / "data/namespace/state.bin").read_bytes(), b"synthetic saved state\x00\xff")
            self.assert_competitor_blocked()

    def test_reserved_backup_requires_held_fd_and_refuses_live_process(self):
        self.enable_saves()
        self.launcher.reserve_setup(); self.launcher.managed_session = "synthetic-session"
        with self.launcher.runtime_lock() as fd:
            child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(10)"])
            self.children.append(child); self.launcher.process = child
            with self.assertRaises(backend.LauncherError):self.launcher._backup_reserved(fd)
            self.assertFalse((self.runtime / "private/save-backups").exists())
            child.terminate();child.wait(timeout=3)
            self.launcher.process = None
        with (self.runtime / "private/play.lock").open("r") as unheld:
            with self.assertRaises(backend.LauncherError):self.launcher._backup_reserved(unheld.fileno())

    def test_backup_never_replaces_existing_backup_and_syncs_parent(self):
        self.enable_saves()
        with patch.object(backend, "datetime") as clock:
            clock.now.return_value.strftime.return_value = "backup-fixed-synthetic"
            clock.now.return_value.isoformat.return_value = "2026-01-01T00:00:00+00:00"
            seen = []
            real_sync = os.fsync

            def sync(descriptor):
                seen.append(os.readlink(f"/proc/self/fd/{descriptor}"))
                return real_sync(descriptor)

            with patch.object(backend.os, "fsync", side_effect=sync):
                first = self.launcher.backup()
            destination = self.runtime / "private/save-backups"
            saved = destination / first["backup"]["name"] / "data/namespace/state.bin"
            original = saved.read_bytes()
            (self.runtime / "private/local-saves/namespace/state.bin").write_bytes(b"new synthetic state")
            with self.assertRaises(OSError):
                self.launcher.backup()
            self.assertEqual(saved.read_bytes(), original)
            self.assertIn(str(destination), seen)
            self.assertFalse(list(destination.glob(".backup-*")))

    def test_managed_shell_rejects_noncanonical_descriptor_before_sourcing(self):
        from flightdeck.setup import resource_paths
        tools, _ = resource_paths()
        environment = self.managed_fixture()
        marker = self.runtime / "private/env-sourced"
        (self.runtime / "tools/runtime-env.sh").write_text(f"touch '{marker}'\n")
        for argument in ("08", "0", "-1", "3x", "99999999"):
            with self.subTest(argument=argument), patch.dict(os.environ, environment):
                result = subprocess.run(["bash", str(tools / "play-msfs.sh"), "--runtime", str(self.runtime),
                                         "--lock-fd", argument], capture_output=True)
                self.assertEqual(result.returncode, 2)
                self.assertFalse(marker.exists())


if __name__ == "__main__":
    unittest.main()

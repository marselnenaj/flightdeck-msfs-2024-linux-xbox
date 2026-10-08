# SPDX-License-Identifier: MIT
"""Exercise failure cleanup and evidence boundaries without a Docker daemon."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("linux_distro_check", ROOT / "scripts/check-linux-distro.py")
distro = importlib.util.module_from_spec(spec)
spec.loader.exec_module(distro)


class LinuxDistro(unittest.TestCase):
    def test_daemon_owned_container_is_removed_after_timeout_failure_and_success(self):
        for outcome in ("timeout", "failed", "success"):
            with self.subTest(outcome=outcome), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                package = root / "package ä"
                package.mkdir()
                (package / "FLIGHTDECK-PACKAGE.json").write_text("{}")
                containers = {"unrelated-user-session"}
                invocations = []

                def docker(command, **options):
                    invocations.append((command, options))
                    if command[:3] == ["docker", "image", "inspect"]:
                        return subprocess.CompletedProcess(command, 0, "sha256:" + "a" * 64 + "\n", "")
                    if command[:2] == ["docker", "run"]:
                        containers.add(command[command.index("--name") + 1])
                        # Killing the Docker CLI deliberately leaves the daemon's
                        # container alive, just as a subprocess timeout does.
                        if outcome == "timeout":
                            raise subprocess.TimeoutExpired(command, options["timeout"])
                        if outcome == "failed":
                            raise subprocess.CalledProcessError(19, command)
                    if command[:3] == ["docker", "rm", "--force"]:
                        containers.remove(command[3])
                    return subprocess.CompletedProcess(command, 0, "", "")

                args = argparse.Namespace(distribution="ubuntu", package=package, output=root / "results")
                with patch.object(distro.subprocess, "run", side_effect=docker):
                    if outcome == "success":
                        distro.outside(args)
                    else:
                        expected = subprocess.TimeoutExpired if outcome == "timeout" else subprocess.CalledProcessError
                        with self.assertRaises(expected):
                            distro.outside(args)
                self.assertEqual(containers, {"unrelated-user-session"})
                command, options = next((c, o) for c, o in invocations if c[:2] == ["docker", "run"])
                self.assertLessEqual(options["timeout"], 420)
                for flag in ("--network=none", "--read-only", "--cap-drop=ALL", "--security-opt=no-new-privileges"):
                    self.assertIn(flag, command)
                self.assertEqual(command[command.index("--user") + 1], f"{os.getuid()}:{os.getgid()}")
                self.assertFalse(any(v.startswith(("--privileged", "--pid", "--device", "--ipc", "--cap-add")) for v in command))
                mounts = [command[i + 1] for i, arg in enumerate(command) if arg == "--mount"]
                self.assertEqual(len(mounts), 4)
                self.assertEqual([m for m in mounts if not m.endswith(",readonly")],
                                 [f"type=bind,source={args.output},target=/results"])
                self.assertIn(f"type=bind,source={package},target=/package,readonly", mounts)
                self.assertFalse(any("docker.sock" in v or "/dev/dri" in v for v in command))
                image = json.loads((args.output / "image.json").read_text())
                self.assertEqual(image["test_image"], "sha256:" + "a" * 64)
                self.assertFalse((args.output / "result.json").exists(), "only the inside checks may claim PASS")

    def test_wayland_requires_a_real_buffer_commit_on_the_same_surface(self):
        for separator in ("@", "#"):
            with self.subTest(separator=separator):
                initial = (f'xdg_wm_base{separator}1.get_xdg_surface(new id xdg_surface{separator}9, wl_surface{separator}7)\n'
                           f'xdg_surface{separator}9.get_toplevel(new id xdg_toplevel{separator}8)\n'
                           f'xdg_toplevel{separator}8.set_title("Flightdeck")\n'
                           f'wl_surface{separator}7.commit()\nxdg_surface{separator}9.ack_configure(1)\n')
                attach = f"wl_surface{separator}7.attach(wl_buffer{separator}10, 0, 0)\n"
                decoration = f"wl_surface{separator}17.attach(wl_buffer{separator}20, 0, 0)\nwl_surface{separator}17.commit()\n"
                self.assertFalse(distro.wayland_painted(initial))
                self.assertFalse(distro.wayland_painted(initial + decoration))
                self.assertFalse(distro.wayland_painted(initial + attach))
                self.assertFalse(distro.wayland_painted(initial + attach + f"wl_surface{separator}17.commit()\n"))
                self.assertFalse(distro.wayland_painted(initial + f"wl_surface{separator}7.attach(nil, 0, 0)\nwl_surface{separator}7.commit()\n"))
                painted = initial + decoration + attach + f"wl_surface{separator}7.commit()\n"
                self.assertTrue(distro.wayland_painted(painted))
                self.assertFalse(distro.wayland_painted(painted.replace('set_title("Flightdeck")', 'set_title("Other")')))
                self.assertFalse(distro.wayland_painted(painted.replace(f"xdg_surface{separator}9.ack_configure", f"xdg_surface{separator}99.ack_configure")))

    def test_corrupt_service_record_cannot_skip_owned_window_and_server_cleanup(self):
        with tempfile.TemporaryDirectory() as temporary:
            work = Path(temporary)
            output = work / "results"
            output.mkdir()
            server, gui = Mock(name="own Xvfb"), Mock(name="own GUI")
            children = iter((server, gui))

            def spawn(*args, **kwargs):
                child = next(children)
                if child is gui:
                    state = work / "x11-state"
                    state.mkdir()
                    (state / "desktop-service.json").write_text('{"token": "synthetic-secret", broken')
                return child

            with patch.object(distro.subprocess, "Popen", side_effect=spawn), \
                    patch.object(distro, "wait_for", side_effect=[True, RuntimeError("paint failed")]), \
                    patch.object(distro, "stop") as stop, patch.object(distro.os, "kill") as kill:
                with self.assertRaisesRegex(RuntimeError, "paint failed"):
                    distro.window(Path("/synthetic/flightdeck"), "x11", work, output)
                self.assertEqual([call.args[0] for call in stop.call_args_list], [gui, server])
                kill.assert_not_called()
            self.assertFalse((output / "x11-service.json").exists())
            self.assertFalse(any("synthetic-secret" in p.read_text() for p in output.iterdir()))


if __name__ == "__main__":
    unittest.main()

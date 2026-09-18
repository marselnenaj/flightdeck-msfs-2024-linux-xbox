"""Numeric progress is optional feedback, never trusted installer state."""
# SPDX-License-Identifier: MIT
import json
import os
import unittest
from unittest.mock import patch

from flightdeck.download_progress import ProgressPipe, normalize_transfer


def frame(**values):
    return {"format": 1, "received_bytes": 40, "verified_bytes": 20,
            "total_bytes": 100, "completed_files": 1, "total_files": 3, **values}


class DownloadProgressTests(unittest.TestCase):
    def test_only_complete_finite_consistent_numeric_schema_is_exposed(self):
        value = frame()
        value.pop("format")
        good = {"kind": "game", **value}
        self.assertEqual(normalize_transfer(good), good)
        self.assertIsNot(normalize_transfer(good), good)
        unknown = {**good, "kind": "components", "total_bytes": None,
                   "completed_files": None, "total_files": None}
        self.assertEqual(normalize_transfer(unknown), unknown)
        invalid = [None, {}, {**good, "account": "never-public"}, {**good, "kind": "account"},
                   {**good, "received_bytes": True}, {**good, "received_bytes": -1},
                   {**good, "received_bytes": 40.0}, {**good, "received_bytes": float('nan')},
                   {**good, "received_bytes": 2**53}, {**good, "verified_bytes": 41},
                   {**good, "total_bytes": 39}, {**good, "total_bytes": 0},
                   {**good, "completed_files": 4}, {**good, "total_files": None}]
        for value in invalid:
            with self.subTest(value=value):
                self.assertIsNone(normalize_transfer(value))

    def test_actual_pipe_frames_can_be_fragmented_and_throttled(self):
        seen = []
        pipe = ProgressPipe(seen.append)
        try:
            raw = json.dumps(frame()).encode() + b'\n'
            os.write(pipe.write_fd, raw[:17])
            pipe.poll()
            self.assertEqual(seen, [])
            os.write(pipe.write_fd, raw[17:])
            with patch('flightdeck.download_progress.time.monotonic', return_value=10):
                pipe.poll()
            self.assertEqual(seen[-1]['received_bytes'], 40)
            os.write(pipe.write_fd, json.dumps(frame(received_bytes=80)).encode() + b'\n')
            with patch('flightdeck.download_progress.time.monotonic', return_value=10.1):
                pipe.poll()
            self.assertEqual(len(seen), 1)
            with patch('flightdeck.download_progress.time.monotonic', return_value=10.2):
                pipe.poll(final=True)
            self.assertEqual([value['received_bytes'] for value in seen], [40, 80])
        finally:
            pipe.close()

    def test_malformed_and_overlong_frames_never_expose_text_and_resynchronize(self):
        seen = []
        pipe = ProgressPipe(seen.append)
        try:
            pipe._consume(b'sensitive-url-or-token:' + b'x' * 100000)
            self.assertLessEqual(len(pipe.buffer), pipe.MAX_FRAME)
            pipe.poll(final=True)
            self.assertEqual(seen, [None])
            pipe._consume(b'\n' + json.dumps(frame()).encode() + b'\n')
            pipe.poll(final=True)
            self.assertEqual(seen[-1]['verified_bytes'], 20)
            for raw in (b'{"format":1,"format":1}\n', b'{"secret":"no"}\n',
                        b'\xff\n', b'[]\n', json.dumps(frame(format=True)).encode()+b'\n'):
                pipe._consume(raw)
                pipe.poll(final=True)
                self.assertIsNone(seen[-1])
            self.assertNotIn('secret', repr(seen))
            self.assertNotIn('sensitive', repr(seen))
        finally:
            pipe.close()

    def test_callback_failure_is_optional_and_all_descriptors_close(self):
        def unavailable(value):
            raise ValueError('Display unavailable')
        pipe = ProgressPipe(unavailable)
        read_fd, write_fd = pipe.read_fd, pipe.write_fd
        pipe._consume(json.dumps(frame()).encode()+b'\n')
        pipe.poll(final=True)
        pipe.child_started()
        pipe.close()
        pipe.close()
        for fd in (read_fd, write_fd):
            with self.assertRaises(OSError):
                os.fstat(fd)


if __name__ == '__main__':
    unittest.main()

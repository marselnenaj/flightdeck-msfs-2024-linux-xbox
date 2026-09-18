import hashlib
import struct
import unittest
from unittest.mock import patch

from flightdeck.save_state import (Container, State, SaveStateError, decode,
                                   encode, content_digest, namespace_key)


def seal(body):
    return body + hashlib.sha256(body).digest()


class SaveStateTests(unittest.TestCase):
    def fixture(self):
        # Independent wire fixture: native little-endian XDLOCAL1, generation 7,
        # one container (profile, Pilot), Unix time 123, one binary blob (data).
        body = bytes.fromhex(
            '58444c4f43414c31 01000000 0700000000000000 01000000 '
            '07000000 70726f66696c65 05000000 50696c6f74 '
            '7b00000000000000 01000000 04000000 64617461 '
            '03000000 00ff42')
        return seal(body)

    def test_independent_wire_fixture(self):
        state = decode(self.fixture())
        self.assertEqual(state.generation, 7)
        self.assertEqual(state.containers['profile'].display_name, 'Pilot')
        self.assertEqual(state.containers['profile'].modified, 123)
        self.assertEqual(state.containers['profile'].blobs, {'data': b'\0\xffB'})
        self.assertEqual(encode(state), self.fixture())

    def test_truncation_corruption_and_appended_bytes(self):
        fixture = self.fixture()
        for data in (fixture[:-1], fixture[:20], fixture + b'\0',
                     fixture[:28] + b'X' + fixture[29:]):
            with self.subTest(size=len(data)), self.assertRaises(SaveStateError):
                decode(data)

    def test_valid_checksum_does_not_bypass_schema_validation(self):
        body = self.fixture()[:-32]
        cases = [body + b'extra', body[:20] + struct.pack('<I', 4097) + body[24:],
                 body[:24] + struct.pack('<I', 0xffffffff) + body[28:],
                 body.replace(b'profile', b'../evil'),
                 body.replace(b'Pilot', b'Pi\0ot'),
                 body.replace(b'Pilot', b'Pi\xffot')]
        for broken in cases:
            with self.subTest(body_size=len(broken)), self.assertRaises(SaveStateError):
                decode(seal(broken))

    def test_duplicate_containers_and_blobs(self):
        body = self.fixture()[:-32]
        container = body[24:]
        with self.assertRaises(SaveStateError):
            decode(seal(body[:20] + struct.pack('<I', 2) + container + container))
        blob_offset = 24 + 4 + 7 + 4 + 5 + 8 + 4
        duplicate = body[:blob_offset - 4] + struct.pack('<I', 2) + body[blob_offset:] * 2
        with self.assertRaises(SaveStateError):
            decode(seal(duplicate))

    def test_namespace_binds_account_title_and_configuration(self):
        scid = '11111111-2222-3333-4444-555555555555'
        base = namespace_key(123, scid, 456)
        self.assertEqual(len(base), 64)
        self.assertNotEqual(base, namespace_key(124, scid, 456))
        self.assertNotEqual(base, namespace_key(123, scid, 457))
        self.assertNotEqual(base, namespace_key(123, '11111111-2222-3333-4444-555555555556', 456))
        for title, user in ((0, 456), (123, 0), (True, 456), (123, -1)):
            with self.assertRaises(SaveStateError):
                namespace_key(title, scid, user)

    def test_changes_distinguished_from_generation_and_mtime(self):
        first = State(1, {'profile': Container('Pilot', 10, {'data': b'one'})})
        same = State(2, {'profile': Container('Pilot', 20, {'data': b'one'})})
        changed = State(2, {'profile': Container('Pilot', 20, {'data': b'two'})})
        self.assertEqual(content_digest(first), content_digest(same))
        self.assertNotEqual(content_digest(first), content_digest(changed))

    def test_names_payloads_and_metadata_are_bounded(self):
        for name in ('', '../escape', 'trailing.', '/root', 'a//b', 'a.b/child', 'a\\b', 'x' * 257):
            with self.subTest(name=name), self.assertRaises(SaveStateError):
                encode(State(0, {name: Container('x', 0)}))
        for item in (Container('a\0b', 0), Container('x' * 4097, 0),
                     Container('x', -1), Container('x', 0, {'blob': 'text'})):
            with self.assertRaises(SaveStateError):
                encode(State(0, {'profile': item}))
        good = State(9, {'folder/profile': Container('Flugzeug ✈', 0, {'data-1.bin': b''})})
        self.assertEqual(decode(encode(good)), good)

    def test_native_combined_size_limit_includes_metadata_allowance(self):
        # Scale down only the size limits: 160 bytes of body, including one
        # payload byte, plus its 32-byte checksum. Native accepts metadata
        # larger than the allowance while the combined size remains bounded.
        good = State(7, {'profile': Container('D' * 96, 123, {'data': b'X'})})
        oversized = State(7, {'profile': Container('D' * 97, 123, {'data': b'X'})})
        encoded = encode(good)
        too_large = encode(oversized)
        self.assertEqual(len(encoded), 192)
        with patch.multiple('flightdeck.save_state', QUOTA=128, METADATA_LIMIT=32):
            self.assertEqual(encode(good), encoded)
            self.assertEqual(decode(encoded), good)
            with self.assertRaises(SaveStateError):
                encode(oversized)
            with self.assertRaises(SaveStateError):
                decode(too_large)


if __name__ == '__main__':
    unittest.main()

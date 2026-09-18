# SPDX-License-Identifier: MIT
"""Synthetic ConnectedStorage tests. No account, network or existing saves."""
import copy
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile
import threading
import unittest
from unittest.mock import patch

from flightdeck import cloud_storage as cloud


ATOM = "11111111-2222-4333-8444-555555555555"
SCID = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"


def scope(**kwargs):
    fields = dict(xuid="123456789", scid=SCID,
                  package_family_name="Example.Game_0123456789abc", title_id=1234)
    fields.update(kwargs)
    return cloud.Scope(**fields)


def response(value, status=200, headers=None):
    body = value if isinstance(value, bytes) else json.dumps(value).encode()
    return cloud.Response(status, headers or {}, body)


def index(*, name="profile", size=4, etag="revision-one"):
    return {"blobs": [{"fileName": name, "displayName": "Profile", "etag": etag,
                       "clientFileTime": 123456, "size": size}],
            "pagingInfo": {"continuationToken": None, "totalItems": 1}}


def atoms(*, name="progress", size=4, identifier=ATOM):
    return {"atoms": [{"atom": identifier, "name": name, "size": size}]}


class SequenceTransport:
    def __init__(self, values):
        self.values = list(values)
        self.requests = []

    def __call__(self, request, *, timeout, max_bytes):
        if not self.values:
            raise AssertionError("Unexpected extra request")
        self.requests.append((request, timeout, max_bytes))
        value = self.values.pop(0)
        return value(request) if callable(value) else response(value)


class CloudStorageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.parent = Path(self.temp.name)
        self.parent.chmod(0o700)

    def client(self, values, **limits):
        transport = SequenceTransport(values)
        return cloud.CloudStorageClient(scope(), transport,
                                       limits=cloud.Limits(**limits)), transport

    def fails(self, client, code, *, snapshot=False, **kwargs):
        with self.assertRaises(cloud.CloudStorageError) as caught:
            if snapshot:
                client.download_snapshot(self.parent, **kwargs)
            else:
                client.read_inventory(**kwargs)
        self.assertEqual(caught.exception.code, code)
        return str(caught.exception)

    def test_scope_binds_every_authenticated_identity_field(self):
        original = scope().binding
        for fields in ({"xuid": "987654321"}, {"scid": ATOM},
                       {"title_id": 4321}, {"package_family_name": "Example.Other_abc"}):
            self.assertNotEqual(scope(**fields).binding, original)
        self.assertEqual(scope(scid=SCID.upper()).binding, original)
        for fields in ({"xuid": "0"}, {"xuid": "1)/foreign"}, {"xuid": str(2**64)},
                       {"scid": "bad"}, {"title_id": True}, {"title_id": 0},
                       {"package_family_name": "Example\r\nAuthorization: fake"}):
            with self.subTest(fields=tuple(fields)), self.assertRaises(cloud.CloudStorageError):
                scope(**fields)

    def test_inventory_normalizes_and_does_not_touch_disk(self):
        client, transport = self.client([index()])
        inventory = client.read_inventory()
        self.assertEqual((inventory.container_count, inventory.total_bytes), (1, 4))
        self.assertEqual(inventory.scope_binding, scope().binding)
        self.assertEqual(inventory.containers[0].etag, "revision-one")
        self.assertEqual(list(self.parent.iterdir()), [])
        self.assertEqual(transport.requests[0][0].url, scope().base_url)
        self.assertEqual(transport.requests[0][0].method, "GET")

    def test_successful_empty_requires_complete_success_response(self):
        empty = {"blobs": [], "pagingInfo": {"continuationToken": None, "totalItems": 0}}
        client, transport = self.client([empty, empty])
        snapshot = client.download_snapshot(self.parent)
        self.assertEqual((snapshot.container_count, snapshot.blob_count, snapshot.total_bytes), (0, 0, 0))
        self.assertEqual(len(transport.requests), 2)
        self.assertEqual(json.loads((snapshot.path / "manifest.json").read_text())["containers"], [])

    def test_http_errors_and_redirects_never_become_empty(self):
        for status, code in ((401, "authentication"), (403, "authentication"),
                             (404, "not_found"), (204, "http_error"), (301, "http_error"),
                             (302, "http_error"), (429, "http_error"), (500, "http_error")):
            with self.subTest(status=status):
                client, transport = self.client([lambda request: cloud.Response(status, {}, b"private error body")])
                message = self.fails(client, code, snapshot=True)
                self.assertNotIn("private", message)
                self.assertEqual(len(transport.requests), 1)
                self.assertEqual(list(self.parent.iterdir()), [])

    def test_incomplete_paging_and_unknown_schema_fail_closed(self):
        bad = []
        paged = index(); paged["pagingInfo"]["continuationToken"] = "opaque-next-page"
        client, _ = self.client([paged]); self.fails(client, "invalid_response")
        value = index(); value["pagingInfo"]["totalItems"] = 2; bad.append(value)
        value = index(); value.pop("pagingInfo"); bad.append(value)
        value = index(); value["pagingInfo"]["continuationToken"] = ""; bad.append(value)
        value = index(); value["pagingInfo"]["totalItems"] = True; bad.append(value)
        value = index(); value["blobs"].append(copy.deepcopy(value["blobs"][0])); value["pagingInfo"]["totalItems"] = 2; bad.append(value)
        for value in bad:
            client, _ = self.client([value]); self.fails(client, "invalid_response")

    @staticmethod
    def pages(token="opaque-next-page"):
        first, second = index(name="first"), index(name="second")
        first["pagingInfo"] = {"totalItems": 2, "continuationToken": token}
        second["pagingInfo"] = {"totalItems": 2, "continuationToken": None}
        return first, second

    def test_complete_paged_inventory_preserves_scope_and_encodes_opaque_token(self):
        token = "https://other.invalid/path?x=a&y=+ /%☁"
        first, second = self.pages(token)
        client, transport = self.client([first, second])
        inventory = client.read_inventory()
        self.assertEqual((inventory.container_count, inventory.total_bytes), (2, 8))
        self.assertEqual([c.name for c in inventory.containers], ["first", "second"])
        self.assertEqual(len(transport.requests), 2)
        url = transport.requests[1][0].url
        self.assertEqual(url, scope().base_url + "?" + cloud.index_query(1, token))
        self.assertNotIn("https://other.invalid", url)
        self.assertEqual(cloud.parse_index_query(url.split("?", 1)[1]), (1, token))
        self.assertTrue(all(request.method == "GET" for request, _, _ in transport.requests))

    def test_paging_failure_is_not_partial_success(self):
        first, _ = self.pages()
        client, _ = self.client([first, lambda request: cloud.Response(403, {}, b"private")])
        self.fails(client, "authentication")
        client, _ = self.client([first, {"blobs": []}])
        self.fails(client, "invalid_response")

    def test_paging_changed_totals_duplicates_repeated_tokens_and_no_progress(self):
        first, second = self.pages()
        duplicate = copy.deepcopy(second); duplicate["blobs"][0]["fileName"] = "first"
        changed = copy.deepcopy(second); changed["pagingInfo"]["totalItems"] = 3
        empty = copy.deepcopy(second); empty["blobs"] = []; empty["pagingInfo"]["continuationToken"] = "third"
        incomplete = copy.deepcopy(second); incomplete["blobs"] = []
        for last, code in ((duplicate, "invalid_response"), (changed, "changed"),
                           (empty, "invalid_response"), (incomplete, "invalid_response")):
            client, _ = self.client([first, last]); self.fails(client, code)
        first["pagingInfo"]["totalItems"] = 3
        second["pagingInfo"] = {"totalItems": 3, "continuationToken": "opaque-next-page"}
        client, _ = self.client([first, second]); self.fails(client, "invalid_response")

    def test_paging_has_page_and_combined_size_limits(self):
        first, second = self.pages()
        client, transport = self.client([first, second], max_pages=1)
        self.fails(client, "bounds")
        self.assertEqual(len(transport.requests), 1)
        client, _ = self.client([first, second], max_total_bytes=7)
        self.fails(client, "bounds")

    def test_paged_snapshot_rechecks_all_pages_before_publication(self):
        first, second = self.pages()
        for page in (first, second): page["blobs"][0]["size"] = 0
        empty = {"atoms": {}}
        client, transport = self.client([first, second, empty, empty, empty, empty, first, second])
        result = client.download_snapshot(self.parent)
        self.assertEqual((result.container_count, result.blob_count, result.total_bytes), (2, 0, 0))
        self.assertEqual(sum("?skipItems=" in r.url for r, _, _ in transport.requests), 2)
        changed = copy.deepcopy(second); changed["blobs"][0]["etag"] = "new-revision"
        client, _ = self.client([first, second, empty, empty, empty, empty, first, changed])
        self.fails(client, "changed", snapshot=True)
        self.assertEqual(list(self.parent.iterdir()), [result.path])

    def test_cancel_between_inventory_pages(self):
        cancel = threading.Event()
        first, second = self.pages()
        def cancel_first(request):
            cancel.set()
            return response(first)
        client, transport = self.client([cancel_first, second])
        self.fails(client, "cancelled", cancel=cancel)
        self.assertEqual(len(transport.requests), 1)

    def test_paging_query_accepts_only_canonical_fixed_fields(self):
        for query in ("skipItems=0&continuationToken=next", "skipItems=01&continuationToken=next",
                      "skipItems=1&continuationToken=", "skipItems=1&continuationToken=x&url=elsewhere",
                      "continuationToken=x&skipItems=1", "skipItems=1&skipItems=2",
                      "skipItems=1&continuationToken=a+b", "skipItems=1&continuationToken=%0A",
                      "skipItems=1&continuationToken=%FF", "skipItems=4097&continuationToken=x"):
            with self.subTest(query=query), self.assertRaises(cloud.CloudStorageError):
                cloud.parse_index_query(query)
        valid = cloud.index_query(1, "a+b")
        cloud._allowed_url(scope().base_url + "?" + valid)
        with self.assertRaises(cloud.CloudStorageError):
            cloud._allowed_url(scope().base_url + "/container?" + valid)

    def test_json_duplicates_types_utf8_and_nonfinite_values_rejected(self):
        for body in (b'{"blobs":[],"blobs":[]}', b'[]', b'null', b'\xff',
                     b'{"blobs":[],"pagingInfo":{"continuationToken":null,"totalItems":NaN}}'):
            client, _ = self.client([body]); self.fails(client, "invalid_response")
        for key, value in (("size", True), ("size", -1), ("size", 1.5), ("etag", ""),
                           ("clientFileTime", -1), ("clientFileTime", "unproven timestamp"),
                           ("fileName", ".."), ("displayName", "bad\x00name")):
            value_index = index(); value_index["blobs"][0][key] = value
            client, _ = self.client([value_index]); self.fails(client, "invalid_response")

    def test_response_body_and_content_length_bounds(self):
        for value, maximum, code in ((cloud.Response(200, {}, b"x" * 101), 100, "bounds"),
                                      (cloud.Response(200, {"Content-Length": "200"}, b"{}"), 100, "bounds"),
                                      (cloud.Response(200, {"Content-Length": "3"}, b"{}"), 100, "invalid_response"),
                                      (cloud.Response(200, {"Content-Encoding": "gzip"}, b"{}"), 100, "invalid_response")):
            client, _ = self.client([lambda request, value=value: value], max_json_bytes=maximum)
            self.fails(client, code)

    def test_snapshot_is_private_bound_and_complete_with_exact_bytes(self):
        client, transport = self.client([index(), atoms(), b"data", atoms(), index()])
        snapshot = client.download_snapshot(self.parent)
        manifest = json.loads((snapshot.path / "manifest.json").read_text())
        self.assertEqual(snapshot.consistency, "rechecked-unlocked")
        self.assertEqual(manifest["scope_binding"], scope().binding)
        self.assertEqual((snapshot.container_count, snapshot.blob_count, snapshot.total_bytes), (1, 1, 4))
        blob = manifest["containers"][0]["blobs"][0]
        self.assertEqual((snapshot.path / blob["file"]).read_bytes(), b"data")
        self.assertEqual(blob["sha256"], hashlib.sha256(b"data").hexdigest())
        self.assertNotIn(scope().xuid, (snapshot.path / "manifest.json").read_text())
        self.assertNotIn(scope().package_family_name, (snapshot.path / "manifest.json").read_text())
        for path in (snapshot.path, snapshot.path / "blobs"):
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o700)
        for path in (snapshot.path / "manifest.json", snapshot.path / blob["file"]):
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
        self.assertEqual([r.method for r, _, _ in transport.requests], ["GET"] * 5)
        self.assertEqual(list(self.parent.iterdir()), [snapshot.path])

    def test_remote_names_are_encoded_once_and_never_filesystem_paths(self):
        name = "folder/../../elsewhere ?#%✓"
        source = index(name=name)
        blob_rows = atoms(name="../../outside")
        client, transport = self.client([source, blob_rows, b"data", blob_rows, source])
        snapshot = client.download_snapshot(self.parent)
        urls = [request.url for request, _, _ in transport.requests]
        self.assertIn("/folder%2F..%2F..%2Felsewhere%20%3F%23%25%E2%9C%93", urls[1])
        manifest = json.loads((snapshot.path / "manifest.json").read_text())
        self.assertEqual(manifest["containers"][0]["name"], name)
        self.assertEqual(manifest["containers"][0]["blobs"][0]["name"], "../../outside")
        self.assertEqual([p.name for p in (snapshot.path / "blobs").iterdir()], ["00000000.bin"])

    def test_contract107_filename_datetime_and_dictionary_atoms(self):
        source = index(name="progress,savedgame")
        source["blobs"][0]["clientFileTime"] = "2026-09-18T12:34:56.1234567+02:00"
        rows = {"atoms": {"data": ATOM + ",binary"}}
        client, transport = self.client([source, rows, b"data", rows, source])
        snapshot = client.download_snapshot(self.parent)
        manifest = json.loads((snapshot.path / "manifest.json").read_text())
        self.assertEqual(manifest["containers"][0]["name"], "progress,savedgame")
        self.assertEqual(manifest["containers"][0]["client_file_time"], cloud._file_time("2026-09-18T10:34:56.1234567Z"))
        self.assertEqual(manifest["containers"][0]["client_file_time"] % 10_000_000, 1234567)
        self.assertEqual(transport.requests[1][0].url, scope().base_url + "/progress%2Csavedgame")
        self.assertEqual(transport.requests[2][2], 4)
        self.assertEqual(transport.requests[0][0].headers["x-xbl-contract-version"], "107")

    def test_dictionary_atoms_enforce_container_sum_and_fixed_atom_route(self):
        rows = {"atoms": {"data": ATOM + ",binary"}}
        for data, code in ((b"short"[:3], "changed"), (b"longer", "bounds")):
            client, _ = self.client([index(), rows, data])
            self.fails(client, code, snapshot=True)
            self.assertEqual(list(self.parent.iterdir()), [])
        for wire_atom in ("https://elsewhere.invalid/private", ATOM, ATOM + ",binary?query", "../other,binary"):
            client, _ = self.client([index(), {"atoms": {"data": wire_atom}}])
            self.fails(client, "invalid_response", snapshot=True)
        client, _ = self.client([index(), {"atoms": {}}])
        self.fails(client, "changed", snapshot=True)

    def test_atom_wire_case_is_preserved_for_both_documented_shapes(self):
        wire_id = SCID.upper()
        for rows in ({"atoms": {"data": wire_id + ",binary"}},
                     atoms(identifier=wire_id)):
            with self.subTest(dictionary=isinstance(rows["atoms"], dict)):
                client, transport = self.client([index(), rows, b"data", rows, index()])
                snapshot = client.download_snapshot(self.parent)
                self.assertEqual(transport.requests[2][0].url,
                                 scope().base_url + "/" + wire_id + ",binary")
                manifest = json.loads((snapshot.path / "manifest.json").read_text())
                self.assertEqual(manifest["containers"][0]["blobs"][0]["atom"], wire_id)

    def test_atom_case_aliases_cannot_bypass_duplicate_detection(self):
        for rows in ({"atoms": {"first": SCID + ",binary", "second": SCID.upper() + ",binary"}},
                     {"atoms": [{"name": "first", "atom": SCID, "size": 2},
                                {"name": "second", "atom": SCID.upper(), "size": 2}]}):
            client, transport = self.client([index(), rows])
            self.fails(client, "invalid_response", snapshot=True)
            self.assertEqual(len(transport.requests), 2)

    def test_datetime_offsets_precision_and_invalid_times(self):
        self.assertEqual(cloud._file_time("1601-01-01T00:00:00Z"), 0)
        self.assertEqual(cloud._file_time("1970-01-01T00:00:00Z"), 116444736000000000)
        self.assertEqual(cloud._file_time("2026-01-01T01:00:00+01:00"), cloud._file_time("2026-01-01T00:00:00Z"))
        for value in ("2026-01-01", "2026-01-01T00:00:00", "2026-02-30T00:00:00Z",
                      "2026-01-01T00:00:00.12345678Z", "2026-01-01T00:00:00+25:00",
                      "1600-01-01T00:00:00Z", "2026-01-01T00:00:00Z\n"):
            with self.assertRaises(cloud.CloudStorageError): cloud._file_time(value)

    def test_exchanged_parent_is_detected_without_following_replacement(self):
        target = self.parent / "snapshot-root"; target.mkdir(mode=0o700)
        moved = self.parent / "old-root"
        def exchange_parent(request):
            target.rename(moved)
            target.mkdir(mode=0o700)
            (target / "preserve").write_bytes(b"new owner data")
            return response(index())
        client, _ = self.client([index(), atoms(), b"data", atoms(), exchange_parent])
        with self.assertRaises(cloud.CloudStorageError) as caught:
            client.download_snapshot(target)
        self.assertEqual(caught.exception.code, "local_storage")
        self.assertEqual(list(moved.iterdir()), [])
        self.assertEqual((target / "preserve").read_bytes(), b"new owner data")

    def test_changed_index_or_atom_list_removes_only_own_staging(self):
        existing = self.parent / "existing"; existing.mkdir(); (existing / "marker").write_text("preserve")
        for values in ([index(), atoms(), b"data", atoms(), index(etag="revision-two")],
                       [index(), atoms(), b"data", atoms(identifier=SCID)]):
            client, _ = self.client(values)
            self.fails(client, "changed", snapshot=True)
            self.assertEqual(list(self.parent.iterdir()), [existing])
            self.assertEqual((existing / "marker").read_text(), "preserve")

    def test_missing_truncated_or_failed_blob_never_publishes(self):
        for failure, code in ((b"dat", "invalid_response"), (b"extra", "bounds"),
                              (lambda request: cloud.Response(404, {}, b""), "not_found")):
            client, _ = self.client([index(), atoms(), failure])
            self.fails(client, code, snapshot=True)
            self.assertEqual(list(self.parent.iterdir()), [])

    def test_atom_duplicates_invalid_guid_and_size_mismatch_rejected(self):
        cases = []
        value = atoms(); value["atoms"].append(copy.deepcopy(value["atoms"][0])); cases.append((value, "invalid_response"))
        cases.extend(((atoms(identifier="https://outside.invalid/blob"), "invalid_response"),
                      (atoms(size=3), "changed"), (atoms(size=True), "invalid_response")))
        for value, code in cases:
            client, _ = self.client([index(), value]); self.fails(client, code, snapshot=True)
            self.assertEqual(list(self.parent.iterdir()), [])

    def test_total_bytes_and_blob_count_limits(self):
        client, _ = self.client([index(size=5)], max_total_bytes=4)
        self.fails(client, "invalid_response")
        value = atoms(size=2); value["atoms"].append({"name": "other", "atom": SCID, "size": 2})
        client, _ = self.client([index(), value], max_blobs=1)
        self.fails(client, "bounds", snapshot=True)

    def test_cancel_before_network_and_after_download_never_publishes(self):
        cancel = threading.Event(); cancel.set()
        client, transport = self.client([])
        self.fails(client, "cancelled", cancel=cancel)
        self.assertEqual(transport.requests, [])
        cancel.clear()
        def cancelled_blob(request):
            cancel.set()
            return response(b"data")
        client, _ = self.client([index(), atoms(), cancelled_blob])
        self.fails(client, "cancelled", snapshot=True, cancel=cancel)
        self.assertEqual(list(self.parent.iterdir()), [])

    def test_operation_deadline_checked_after_transport(self):
        now = [0.0]
        def slow(request):
            now[0] = 11.0
            return response(index())
        client, _ = self.client([slow], deadline_seconds=10)
        with patch.object(cloud.time, "monotonic", side_effect=lambda: now[0]):
            self.fails(client, "deadline")

    def test_no_secret_response_or_authorizer_exception_is_reported(self):
        def broken(request):
            raise RuntimeError("PRIVATE_AUTH_VALUE")
        client, _ = self.client([broken])
        self.assertNotIn("PRIVATE_AUTH_VALUE", self.fails(client, "transport"))
        for obj in (scope(), cloud.Request(scope().base_url), response(b"PRIVATE_BODY")):
            self.assertNotIn(scope().xuid, repr(obj))
            self.assertNotIn("PRIVATE_BODY", repr(obj))

    def test_parent_symlink_and_nonprivate_directory_rejected_before_network(self):
        real = self.parent / "real"; real.mkdir(mode=0o700)
        alias = self.parent / "alias"; alias.symlink_to(real, target_is_directory=True)
        client, transport = self.client([])
        for target in (alias, alias / "child", Path("relative")):
            with self.assertRaises(cloud.CloudStorageError):
                client.download_snapshot(target)
        real.chmod(0o755)
        with self.assertRaises(cloud.CloudStorageError): client.download_snapshot(real)
        self.assertEqual(transport.requests, [])

    def test_write_failure_preserves_previous_snapshot_and_cleans_partial(self):
        old = self.parent / "old"; old.write_bytes(b"existing")
        client, _ = self.client([index(), atoms(), b"data"])
        def failing_write(fd, name, data):
            out = os.open(name, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600, dir_fd=fd)
            os.write(out, b"partial"); os.close(out)
            raise OSError("synthetic disk failure")
        with patch.object(cloud, "_write", side_effect=failing_write):
            self.fails(client, "local_storage", snapshot=True)
        self.assertEqual(list(self.parent.iterdir()), [old])
        self.assertEqual(old.read_bytes(), b"existing")

    def test_publish_never_overwrites_an_existing_generation(self):
        stage = self.parent / "stage"; stage.mkdir()
        target = self.parent / "target"; target.mkdir(); (target / "marker").write_bytes(b"old")
        fd = os.open(self.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            with self.assertRaises(cloud.CloudStorageError):
                cloud._rename_noreplace(fd, "stage", "target")
        finally:
            os.close(fd)
        self.assertTrue(stage.is_dir())
        self.assertEqual((target / "marker").read_bytes(), b"old")


class TransportBoundaryTests(unittest.TestCase):
    def test_disallowed_target_is_rejected_before_transport(self):
        for url in ("http://titlestorage.xboxlive.com/connectedstorage/users/xuid(1)",
                    "https://outside.invalid/connectedstorage/users/xuid(1)",
                    "https://titlestorage.xboxlive.com@outside.invalid/a",
                    scope().base_url + "?redirect=elsewhere", scope().base_url + "#fragment"):
            with self.assertRaises(cloud.CloudStorageError): cloud._allowed_url(url)
        transport = SequenceTransport([index()])
        client = cloud.CloudStorageClient(scope(), transport)
        with patch.object(cloud, "_allowed_url", side_effect=cloud.CloudStorageError("invalid_request")):
            with self.assertRaises(cloud.CloudStorageError): client.read_inventory()
        self.assertEqual(transport.requests, [])

    def test_unstructured_response_is_rejected(self):
        client = cloud.CloudStorageClient(scope(), lambda request, **kwargs: {"status": 200})
        with self.assertRaises(cloud.CloudStorageError) as caught: client.read_inventory()
        self.assertEqual(caught.exception.code, "invalid_response")

    def test_response_header_injection_and_duplicates_are_rejected(self):
        for headers in ({"ETag": "value\r\nAuthorization: private"},
                        {"Content-Length": "0", "content-length": "0"},
                        {"Bad Header": "value"}):
            client = cloud.CloudStorageClient(scope(), lambda request, **kwargs: cloud.Response(200, headers, b""))
            with self.assertRaises(cloud.CloudStorageError) as caught: client.read_inventory()
            self.assertEqual(caught.exception.code, "invalid_response")
            self.assertNotIn("private", str(caught.exception))


if __name__ == "__main__":
    unittest.main()

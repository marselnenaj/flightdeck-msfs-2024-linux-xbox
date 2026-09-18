# SPDX-License-Identifier: MIT
"""Real reader + private snapshot cache, entirely synthetic HTTP and saves."""
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
from urllib.parse import unquote
import uuid

from flightdeck import cloud_cache as cache, cloud_import as ci, cloud_storage as cs, save_state


def scope(**changes):
    values = dict(xuid="123456789", scid="aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
                  package_family_name="Example.Game_0123456789abc", title_id=1234)
    values.update(changes)
    return cs.Scope(**values)


class Service:
    def __init__(self, binding=None, count=18):
        self.scope = binding or scope()
        self.rows = {f"c{i:02}": {"data": bytes([65 + i]) * 4, "etag": f'"revision-{i}"',
                     "display": f"Save {i}", "time": (11644473600 + 100) * 10_000_000,
                     "atom": str(uuid.UUID(int=i + 1))} for i in range(count)}
        self.calls = []
        self.index_reads = 0
        self.index_hook = None
        self.failure = None
        self.atom_hook = None

    def __call__(self, request, *, timeout, max_bytes):
        assert request.method == "GET" and request.url.startswith(self.scope.base_url)
        suffix = unquote(request.url[len(self.scope.base_url):])
        self.calls.append(suffix)
        if self.failure:
            return cs.Response(self.failure, {}, b"private synthetic error")
        if suffix == "":
            self.index_reads += 1
            if self.index_hook:
                self.index_hook(self)
            value = {"blobs": [{"fileName": name + ",savedgame", "displayName": row["display"],
                     "etag": row["etag"], "clientFileTime": row["time"], "size": len(row["data"])}
                     for name, row in self.rows.items()],
                     "pagingInfo": {"totalItems": len(self.rows), "continuationToken": None}}
        elif suffix.endswith(",binary"):
            identifier = suffix[1:-7]
            return cs.Response(200, {}, next(row["data"] for row in self.rows.values() if row["atom"] == identifier))
        elif suffix.endswith(",savedgame"):
            row = self.rows[suffix[1:-10]]
            if self.atom_hook:
                self.atom_hook(self, row)
            value = {"atoms": {"data": row["atom"] + ",binary"}}
        else:
            raise AssertionError("Unexpected synthetic route")
        return cs.Response(200, {}, json.dumps(value).encode())

    def reset(self):
        self.calls.clear(); self.index_reads = 0

    def client(self, **limits):
        return cs.CloudStorageClient(self.scope, self, limits=cs.Limits(**limits))


class CacheTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.runtime = Path(self.temp.name) / "runtime"; self.runtime.mkdir(mode=0o700)
        (self.runtime / "private").mkdir(mode=0o700)
        self.service = Service()

    def download(self, service=None, **kwargs):
        return cache.download(self.runtime, (service or self.service).client(), **kwargs)

    def pointer(self, binding=None):
        return self.runtime / "private/cloud-cache" / ((binding or scope()).binding + ".json")

    def warm(self):
        snapshot = self.download()
        self.assertEqual(len(self.service.calls), 56)
        self.service.reset()
        return snapshot

    def test_cold_then_warm_real_reader_uses_two_live_indexes(self):
        old = self.warm()
        current = self.download()
        self.assertEqual(self.service.calls, ["", ""])
        self.assertEqual((current.container_count, current.blob_count, current.total_bytes), (18, 18, 72))
        self.assertEqual(save_state.content_digest(ci.read_snapshot(self.runtime, scope(), current)),
                         save_state.content_digest(ci.read_snapshot(self.runtime, scope(), old)))
        self.assertNotEqual(current.path, old.path)
        self.assertTrue(old.path.exists())

    def test_written_container_bypasses_cache_even_when_etag_is_unchanged(self):
        self.warm()
        self.service.rows['c00']['data'] = b'NEW!'
        result = self.download(force_containers=frozenset({'c00'}))
        self.assertEqual(len(self.service.calls), 5)
        self.assertEqual(sum(route.endswith(',binary') for route in self.service.calls), 1)
        self.assertEqual(ci.read_snapshot(self.runtime, scope(), result).containers['c00'].blobs['data'], b'NEW!')
        self.service.reset()
        result = self.download()
        self.assertEqual(self.service.calls, ['', ''])
        self.assertEqual(ci.read_snapshot(self.runtime, scope(), result).containers['c00'].blobs['data'], b'NEW!')

    def test_deleted_forced_container_requires_fresh_complete_absence(self):
        self.warm()
        del self.service.rows['c00']
        result = self.download(force_containers=frozenset({'c00'}))
        self.assertEqual(self.service.calls, ['', ''])
        self.assertNotIn('c00', ci.read_snapshot(self.runtime, scope(), result).containers)

    def test_invalid_forced_selection_fails_before_any_remote_call(self):
        for selection in ({'c00'}, ['c00'], 'c00', frozenset({'../c00'}), frozenset({'c00,savedgame'})):
            with self.subTest(selection=selection), self.assertRaises(ValueError):
                self.download(force_containers=selection)
        self.assertEqual(self.service.calls, [])

    def test_each_revision_field_change_redownloads_complete_container(self):
        self.warm()
        for field, value in [("etag", '"new"'), ("display", "Renamed"), ("time", (11644473600+200)*10_000_000),
                             ("data", b"longer"), ("etag", '"NEW"')]:
            with self.subTest(field=field):
                self.service.rows["c00"][field] = value
                self.service.reset(); result = self.download()
                self.assertEqual(len(self.service.calls), 5)
                self.assertEqual(sum(s.endswith(",binary") for s in self.service.calls), 1)
                self.assertEqual(ci.read_snapshot(self.runtime, scope(), result).containers["c00"].blobs["data"], self.service.rows["c00"]["data"])

    def test_renamed_added_and_removed_containers_follow_complete_index(self):
        self.warm()
        self.service.rows["renamed"] = self.service.rows.pop("c00")
        del self.service.rows["c01"]
        current = self.download()
        self.assertEqual(len(self.service.calls), 5)
        state = ci.read_snapshot(self.runtime, scope(), current)
        self.assertNotIn("c00", state.containers); self.assertNotIn("c01", state.containers)
        self.assertIn("renamed", state.containers)

    def test_confirmed_empty_is_not_a_cache_fallback(self):
        self.warm(); self.service.rows.clear()
        current = self.download()
        self.assertEqual(self.service.calls, ["", ""])
        self.assertEqual(current.container_count, 0)
        self.assertEqual(ci.read_snapshot(self.runtime, scope(), current).containers, {})

    def test_profile_change_cannot_reuse_another_scope(self):
        self.warm()
        other = Service(scope(xuid="987654321"))
        self.pointer(other.scope).write_bytes(self.pointer().read_bytes()); self.pointer(other.scope).chmod(0o600)
        current = self.download(other)
        self.assertEqual(len(other.calls), 56)
        self.assertEqual(current.scope_binding, other.scope.binding)

    def test_cached_payload_corruption_is_a_full_network_read(self):
        old = self.warm(); (old.path / "blobs/00000000.bin").write_bytes(b"evil")
        current = self.download()
        self.assertEqual(len(self.service.calls), 56)
        self.assertEqual(ci.read_snapshot(self.runtime, scope(), current).containers["c00"].blobs["data"], b"AAAA")

    def test_manifest_rewrite_cannot_reseal_itself(self):
        old = self.warm(); manifest = old.path / "manifest.json"
        data = json.loads(manifest.read_bytes()); data["containers"][0]["display_name"] = "Edited"
        manifest.write_text(json.dumps(data))
        self.download(); self.assertEqual(len(self.service.calls), 56)

    def test_missing_snapshot_and_invalid_pointer_are_cache_misses(self):
        self.warm()
        for payload in [b"invalid", b"{}", b"x" * 4097]:
            self.pointer().write_bytes(payload); self.service.reset()
            self.download(); self.assertEqual(len(self.service.calls), 56)
        data = json.loads(self.pointer().read_text()); data["snapshot_id"] = "snapshot-" + "0" * 32
        self.pointer().write_text(json.dumps(data)); self.service.reset()
        self.download(); self.assertEqual(len(self.service.calls), 56)

    def test_snapshot_path_traversal_and_symlink_never_read_foreign_data(self):
        old = self.warm()
        foreign = Path(self.temp.name) / "outside"; foreign.write_bytes(b"must remain unchanged")
        data = json.loads(self.pointer().read_text()); data["snapshot_id"] = "../../outside"
        self.pointer().write_text(json.dumps(data)); self.download()
        self.assertEqual(foreign.read_bytes(), b"must remain unchanged")
        current = self.warm_after_current()
        blob = current.path / "blobs/00000000.bin"; blob.unlink(); blob.symlink_to(foreign)
        self.download(); self.assertEqual(len(self.service.calls), 56)
        self.assertEqual(foreign.read_bytes(), b"must remain unchanged")

    def warm_after_current(self):
        ref = json.loads(self.pointer().read_text()); self.service.reset()
        return cs.Snapshot(self.runtime / "private/cloud-saves" / ref["snapshot_id"], scope().binding,
                           ref["container_count"], ref["blob_count"], ref["total_bytes"])

    def test_live_auth_errors_never_return_cache_or_change_pointer(self):
        self.warm(); before = self.pointer().read_bytes()
        for status in [401, 403, 404, 429, 500, 302]:
            self.service.failure = status; self.service.reset()
            with self.subTest(status=status), self.assertRaises(cs.CloudStorageError): self.download()
            self.assertEqual(self.service.calls, [""])
            self.assertEqual(self.pointer().read_bytes(), before)

    def test_final_index_change_rejects_mixed_snapshot(self):
        self.warm(); before = self.pointer().read_bytes()
        def hook(service):
            if service.index_reads == 2: service.rows["c00"]["etag"] = '"changed"'
        self.service.index_hook = hook
        with self.assertRaises(cs.CloudStorageError) as caught: self.download()
        self.assertEqual(caught.exception.code, "changed")
        self.assertEqual(self.pointer().read_bytes(), before)
        self.assertFalse(any(p.name.startswith(".snapshot-") for p in (self.runtime / "private/cloud-saves").iterdir()))

    def test_changed_container_atom_recheck_is_not_skipped(self):
        self.warm(); self.service.rows["c00"]["etag"] = '"new"'; count = 0
        def hook(service, row):
            nonlocal count
            count += 1
            if count == 2: row["atom"] = str(uuid.uuid4())
        self.service.atom_hook = hook
        with self.assertRaises(cs.CloudStorageError) as caught: self.download()
        self.assertEqual(caught.exception.code, "changed")

    def test_cancel_before_or_during_freshness_check_never_succeeds(self):
        self.warm(); event = threading.Event(); event.set()
        with self.assertRaises(cs.CloudStorageError) as caught: self.download(cancel=event)
        self.assertEqual(caught.exception.code, "cancelled"); self.assertEqual(self.service.calls, [])
        event.clear(); self.service.index_hook = lambda service: event.set()
        with self.assertRaises(cs.CloudStorageError) as caught: self.download(cancel=event)
        self.assertEqual(caught.exception.code, "cancelled")
        self.assertEqual(len(self.service.calls), 1)

    def test_cache_data_and_pointer_are_private_and_atomic(self):
        current = self.warm(); pointer = self.pointer()
        self.assertEqual(stat.S_IMODE(pointer.stat().st_mode), 0o600)
        self.assertEqual(stat.S_IMODE(pointer.parent.stat().st_mode), 0o700)
        self.assertFalse(any(p.name.startswith(".cache-") for p in pointer.parent.iterdir()))
        self.assertNotIn(scope().xuid.encode(), pointer.read_bytes())
        original = pointer.read_bytes()
        with patch.object(cache.os, "replace", side_effect=OSError("synthetic")):
            self.assertFalse(cache.remember(self.runtime, scope(), current))
        self.assertEqual(pointer.read_bytes(), original)
        self.assertFalse(any(p.name.startswith(".cache-") for p in pointer.parent.iterdir()))

    def test_unsafe_cache_directory_is_optional_but_snapshot_directory_is_not(self):
        outside = Path(self.temp.name) / "outside"; outside.mkdir(mode=0o700)
        (self.runtime / "private/cloud-cache").symlink_to(outside)
        current = self.download(); self.assertEqual(len(self.service.calls), 56)
        self.assertEqual(list(outside.iterdir()), [])
        self.assertFalse(cache.remember(self.runtime, scope(), current))
        (self.runtime / "private/cloud-saves").rename(self.runtime / "private/old-cloud")
        (self.runtime / "private/cloud-saves").symlink_to(outside)
        self.service.reset()
        with self.assertRaises(cs.CloudStorageError): self.download()
        self.assertEqual(self.service.calls, []); self.assertEqual(list(outside.iterdir()), [])

    def test_remember_rejects_corrupt_snapshot_and_network_readback_stays_uncached(self):
        current = self.warm()
        direct = self.service.client().download_snapshot(self.runtime / "private/cloud-saves")
        self.assertEqual(len(self.service.calls), 56)
        self.assertTrue(cache.remember(self.runtime, scope(), direct))
        (current.path / "blobs/00000000.bin").write_bytes(b"evil")
        with self.assertRaises(ci.CloudImportError): cache.remember(self.runtime, scope(), current)

    def test_optional_cache_revalidation_failure_does_not_fail_successful_download(self):
        with patch.object(cache, "remember", side_effect=ci.CloudImportError("invalid_snapshot")):
            result = self.download()
        self.assertEqual(ci.read_snapshot(self.runtime, scope(), result).containers["c00"].blobs["data"], b"AAAA")
        self.assertFalse(self.pointer().exists())

    def test_two_complete_paginated_indexes_are_required_even_for_cache_hits(self):
        self.warm()
        complete = json.loads(self.service(cs.Request(scope().base_url), timeout=1, max_bytes=100000).body)
        pages = []
        for _ in range(2):
            for number in range(2):
                page = copy.deepcopy(complete); page["blobs"] = page["blobs"][number*9:(number+1)*9]
                page["pagingInfo"]["continuationToken"] = "opaque-next" if number == 0 else None
                pages.append(page)
        calls = []
        def transport(request, **kwargs):
            calls.append(request.url)
            return cs.Response(200, {}, json.dumps(pages.pop(0)).encode())
        result = cache.download(self.runtime, cs.CloudStorageClient(scope(), transport))
        self.assertEqual(len(calls), 4); self.assertEqual(result.container_count, 18)
        self.assertTrue(calls[1].endswith("?skipItems=9&continuationToken=opaque-next"))
        self.assertEqual(calls[1], calls[3])

    def test_live_invalid_paging_and_empty_etag_never_fall_back_to_cache(self):
        self.warm(); original = self.pointer().read_bytes()
        for kind in ["missing_page", "empty_etag"]:
            def transport(request, **kwargs):
                value = json.loads(self.service(cs.Request(scope().base_url), **kwargs).body)
                if kind == "missing_page": value["pagingInfo"]["totalItems"] += 1
                else: value["blobs"][0]["etag"] = ""
                return cs.Response(200, {}, json.dumps(value).encode())
            with self.subTest(kind=kind), self.assertRaises(cs.CloudStorageError):
                cache.download(self.runtime, cs.CloudStorageClient(scope(), transport))
            self.assertEqual(self.pointer().read_bytes(), original)

    def test_cache_work_consumes_the_same_overall_deadline(self):
        original = cache._load
        clock = [0.0]
        def slow_load(*args):
            result = original(*args); clock[0] = 181.0; return result
        with patch.object(cache.time, "monotonic", side_effect=lambda:clock[0]), patch.object(cache, "_load", side_effect=slow_load):
            with self.assertRaises(cs.CloudStorageError) as caught: self.download()
        self.assertEqual(caught.exception.code, "deadline"); self.assertEqual(self.service.calls, [])

    def test_changed_container_downloads_all_blobs_even_with_old_atom_guid(self):
        atoms = {"data": str(uuid.UUID(int=1)), "other": str(uuid.UUID(int=2))}
        payloads = {atoms["data"]: b"AAAA", atoms["other"]: b"BBBB"}
        revision = ["revision-one"]
        calls = []
        def transport(request, **kwargs):
            suffix = unquote(request.url[len(scope().base_url):]); calls.append(suffix)
            if suffix.endswith(",binary"):
                return cs.Response(200, {}, payloads[suffix[1:-7]])
            if suffix:
                value = {"atoms": {name: atom+",binary" for name, atom in atoms.items()}}
            else:
                value = {"blobs": [{"fileName": "c00,savedgame", "displayName": "Pair", "etag": revision[0],
                    "clientFileTime": (11644473600+100)*10_000_000, "size": 8}],
                    "pagingInfo": {"totalItems": 1, "continuationToken": None}}
            return cs.Response(200, {}, json.dumps(value).encode())
        client = cs.CloudStorageClient(scope(), transport)
        cache.download(self.runtime, client); self.assertEqual(len(calls), 6)
        calls.clear(); cache.download(self.runtime, client); self.assertEqual(calls, ["", ""])
        calls.clear(); revision[0] = "revision-two"; payloads[atoms["data"]] = b"CCCC"
        result = cache.download(self.runtime, client)
        self.assertEqual(len(calls), 6)
        self.assertEqual(sum(route.endswith(",binary") for route in calls), 2)
        self.assertEqual(ci.read_snapshot(self.runtime, scope(), result).containers["c00"].blobs, {"data": b"CCCC", "other": b"BBBB"})


if __name__ == "__main__": unittest.main()

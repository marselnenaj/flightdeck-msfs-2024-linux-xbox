import dataclasses
import threading
import unittest
from unittest.mock import patch

from flightdeck import cloud_write as write, save_state as saves
from flightdeck.cloud_storage import CloudStorageError, Scope

SCOPE = Scope('1', '10000000-0000-0000-0000-000000000001', 'Synthetic.Game_123', 1)
ATOM = 'ABCDEF01-1234-1234-1234-1234567890AB'


def state(**containers):
    return saves.State(1, {name: saves.Container(name, 1700000000, {'data': payload})
                           for name, payload in containers.items()})


def copy(value):
    return saves.decode(saves.encode(value))


class Operations:
    scope = SCOPE
    def __init__(self, remote):
        self.remote = copy(remote)
        self.calls = []
        self.owner = 'generation-one'
        self.renew_status = 200
        self.acquire_status = 201
        self.release_status = 204
        self.fault = None
        self.atoms = {}
        self.cancel_after_commit = None
        self.extra_after_commit = False
        self.quota = saves.QUOTA

    def record(self, name, timeout):
        assert 0 < timeout <= 600
        self.calls.append(name)

    def acquire(self, *, timeout):
        self.record('acquire', timeout)
        return write.LeaseReply(self.acquire_status, self.owner, self.quota)

    def renew(self, *, timeout):
        self.record('renew', timeout)
        return write.LeaseReply(self.renew_status, self.owner, self.quota)

    def release(self, *, timeout):
        self.record('release', timeout)
        if self.release_status is None:
            raise CloudStorageError('transport')
        return self.release_status

    def upload_atom(self, payload, *, timeout):
        self.record('atom', timeout)
        atom = f'{len(self.atoms):08X}-1234-1234-1234-1234567890AB'
        self.atoms[atom] = payload
        return atom

    def after_commit(self):
        if self.cancel_after_commit:
            self.cancel_after_commit.set()
        if self.extra_after_commit:
            self.remote.containers['external'] = saves.Container('unexpected', 1, {})
        if self.fault == 'lost_reply':
            raise CloudStorageError('transport')
        return 200

    def put_container(self, name, display, modified, atoms, *, timeout):
        self.record('put', timeout)
        if self.fault == '409':
            return 409
        if self.fault == '403':
            return 403
        if self.fault != 'missing_commit':
            self.remote.containers[name.removesuffix(',savedgame')] = saves.Container(
                display, modified, {key: self.atoms[value] for key, value in atoms.items()})
        return self.after_commit()

    def delete_container(self, name, *, timeout):
        self.record('delete', timeout)
        self.remote.containers.pop(name.removesuffix(',savedgame'), None)
        return self.after_commit()

    def read(self, *, timeout):
        self.record('read', timeout)
        return copy(self.remote)


class CloudWriteTests(unittest.TestCase):
    def upload(self, ops, target, **kwargs):
        return write.upload(SCOPE, ops, ops.read, target,
                            expected_remote_digest=kwargs.pop('expected_remote_digest', saves.content_digest(ops.remote)),
                            assert_local_unchanged=kwargs.pop('assert_local_unchanged', lambda: None), **kwargs)

    def test_changed_container_and_delete_are_verified_and_sealed(self):
        ops = Operations(state(keep=b'same', replace=b'old', remove=b'old'))
        target = state(keep=b'same', replace=b'new', add=b'new')
        receipt = self.upload(ops, target)
        self.assertEqual(saves.content_digest(ops.remote), saves.content_digest(target))
        self.assertEqual((receipt.changed_containers, receipt.container_count, receipt.blob_count), (3, 3, 3))
        self.assertEqual(ops.calls.count('put'), 2)
        self.assertEqual(ops.calls.count('delete'), 1)
        self.assertEqual(ops.calls.count('atom'), 2)
        self.assertEqual(ops.calls[-1], 'release')
        self.assertTrue(write.verify_receipt(receipt, scope_binding=SCOPE.binding,
                                            source_content_digest=saves.content_digest(target)))
        self.assertTrue(receipt.lease_released)

    def test_receipt_cannot_be_forged_or_rebound(self):
        ops = Operations(state(one=b'same'))
        receipt = self.upload(ops, ops.remote)
        self.assertFalse(write.verify_receipt(dataclasses.replace(receipt, changed_containers=9),
                         scope_binding=SCOPE.binding, source_content_digest=receipt.source_content_digest))
        self.assertFalse(write.verify_receipt(receipt, scope_binding='wrong',
                                             source_content_digest=receipt.source_content_digest))
        self.assertFalse(write.verify_receipt(object(), scope_binding='', source_content_digest=''))

    def test_unchanged_content_does_not_upload(self):
        ops = Operations(state(one=b'same'))
        target = copy(ops.remote)
        target.generation += 1
        target.containers['one'].modified += 100
        receipt = self.upload(ops, target)
        self.assertEqual(receipt.changed_containers, 0)
        self.assertNotIn('put', ops.calls)
        self.assertNotIn('atom', ops.calls)

    def test_conflicting_remote_never_writes(self):
        ops = Operations(state(one=b'changed'))
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(one=b'local'), expected_remote_digest=saves.content_digest(state(one=b'old')))
        self.assertEqual(error.exception.code, 'conflict')
        self.assertFalse(error.exception.recovery_required)
        self.assertNotIn('atom', ops.calls)
        self.assertEqual(ops.calls[-1], 'release')

    def test_renewal_requires_existing_owner(self):
        for status in (201, 409, 403, 500):
            with self.subTest(status=status):
                ops = Operations(state())
                ops.renew_status = status
                with self.assertRaises(write.CloudWriteError):
                    self.upload(ops, state(one=b'new'))
                self.assertNotIn('atom', ops.calls)
                self.assertEqual('release' in ops.calls, status == 201)

    def test_owner_change_on_200_fences_upload(self):
        ops = Operations(state())
        original = ops.read
        def read(*, timeout):
            ops.owner = 'different-generation'
            return original(timeout=timeout)
        ops.read = read
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(one=b'new'))
        self.assertEqual(error.exception.code, 'lease_lost')
        self.assertNotIn('atom', ops.calls)

    def test_malformed_or_lost_renewal_does_not_assume_cleanup_ownership(self):
        for kind in ('invalid', 'lost'):
            ops = Operations(state())
            def renewal(**kwargs):
                if kind == 'lost':
                    raise CloudStorageError('transport')
                return write.LeaseReply(200, '', 1)
            ops.renew = renewal
            with self.assertRaises(write.CloudWriteError):
                self.upload(ops, state(one=b'new'))
            self.assertNotIn('release', ops.calls)
            self.assertNotIn('put', ops.calls)

    def test_missing_or_invalid_owner_and_quota_rejected(self):
        for reply in (write.LeaseReply(200, '', 1), write.LeaseReply(200, 'bad\r\n', 1),
                      write.LeaseReply(200, 'x', True), write.LeaseReply(True, 'x', 1)):
            ops = Operations(state())
            ops.acquire = lambda **_: reply
            with self.assertRaises(write.CloudWriteError):
                self.upload(ops, state())
            self.assertEqual(ops.calls, [])

    def test_acquire_conflict_or_denial_never_releases_foreign_lock(self):
        for status in (409, 403, 401):
            ops = Operations(state())
            ops.acquire_status = status
            with self.assertRaises(write.CloudWriteError):
                self.upload(ops, state(one=b'new'))
            self.assertEqual(ops.calls, ['acquire'])

    def test_lost_acquire_reply_does_not_assume_ownership(self):
        ops = Operations(state())
        def lost(**kwargs):
            raise CloudStorageError('transport')
        ops.acquire = lost
        with self.assertRaises(write.CloudWriteError):
            self.upload(ops, state())
        self.assertNotIn('release', ops.calls)

    def test_lost_commit_reply_is_recovered_by_readback_without_retry(self):
        ops = Operations(state(one=b'old'))
        ops.fault = 'lost_reply'
        receipt = self.upload(ops, state(one=b'new'))
        self.assertTrue(receipt.committed)
        self.assertEqual(ops.calls.count('put'), 1)
        self.assertEqual(receipt.changed_containers, 1)

    def test_success_status_without_actual_commit_never_succeeds(self):
        ops = Operations(state(one=b'old'))
        ops.fault = 'missing_commit'
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(one=b'new'))
        self.assertEqual(error.exception.code, 'readback')
        self.assertTrue(error.exception.recovery_required)
        self.assertEqual(error.exception.committed_containers, 0)
        self.assertEqual(ops.calls.count('put'), 1)

    def test_conflict_and_auth_commit_responses_stop_without_retry(self):
        for fault, code in (('409', 'lease_lost'), ('403', 'authentication')):
            ops = Operations(state(one=b'old'))
            ops.fault = fault
            with self.assertRaises(write.CloudWriteError) as error:
                self.upload(ops, state(one=b'new'))
            self.assertEqual(error.exception.code, code)
            self.assertTrue(error.exception.recovery_required)
            self.assertEqual(ops.calls.count('put'), 1)
            self.assertNotIn('release', ops.calls)

    def test_final_readback_detects_unexpected_container(self):
        ops = Operations(state())
        ops.extra_after_commit = True
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(one=b'new'))
        self.assertEqual(error.exception.code, 'readback')
        self.assertEqual(error.exception.committed_containers, 1)
        self.assertTrue(error.exception.recovery_required)

    def test_cancel_before_acquire_does_nothing(self):
        ops, cancel = Operations(state()), threading.Event()
        cancel.set()
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(), cancel=cancel)
        self.assertEqual(error.exception.code, 'cancelled')
        self.assertEqual(ops.calls, [])

    def test_cancel_after_commit_requires_recovery_and_cleanup(self):
        ops, cancel = Operations(state()), threading.Event()
        ops.cancel_after_commit = cancel
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(first=b'1', second=b'2'), cancel=cancel)
        self.assertEqual(error.exception.code, 'cancelled')
        self.assertTrue(error.exception.recovery_required)
        self.assertEqual(ops.calls.count('put'), 1)
        self.assertEqual(ops.calls[-1], 'release')

    def test_partial_success_is_not_a_whole_library_commit(self):
        ops = Operations(state())
        original = ops.put_container
        def put(name, *args, **kwargs):
            if name == 'second,savedgame':
                return 409
            return original(name, *args, **kwargs)
        ops.put_container = put
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(first=b'1', second=b'2'))
        self.assertEqual(error.exception.committed_containers, 1)
        self.assertTrue(error.exception.recovery_required)

    def test_release_failure_does_not_erase_verified_commit(self):
        ops = Operations(state())
        ops.release_status = None
        receipt = self.upload(ops, state(one=b'new'))
        self.assertTrue(receipt.committed)
        self.assertFalse(receipt.lease_released)

    def test_quota_and_size_limits_prevent_upload(self):
        ops = Operations(state())
        ops.quota = 1
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(one=b'ab'))
        self.assertEqual(error.exception.code, 'quota')
        self.assertNotIn('atom', ops.calls)
        ops.calls.clear()
        with self.assertRaises(write.CloudWriteError):
            self.upload(ops, state(one=b'ab'), limits=write.Limits(max_blob_bytes=1))
        self.assertEqual(ops.calls, [])

    def test_operation_and_absolute_deadline_limits(self):
        ops = Operations(state())
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(one=b'new'), limits=write.Limits(max_operations=1))
        self.assertEqual(error.exception.code, 'bounds')
        ops = Operations(state())
        with patch('flightdeck.cloud_write.time.monotonic', side_effect=[0, 601]):
            with self.assertRaises(write.CloudWriteError) as error:
                self.upload(ops, state())
        self.assertEqual(error.exception.code, 'deadline')
        self.assertEqual(ops.calls, [])

    def test_expired_read_callback_cannot_produce_success(self):
        ops = Operations(state())
        clock = [0]
        def late_read(*, timeout):
            self.assertLessEqual(timeout, 600)
            clock[0] = 601
            return state()
        ops.read = late_read
        with patch('flightdeck.cloud_write.time.monotonic', side_effect=lambda: clock[0]):
            with self.assertRaises(write.CloudWriteError) as error:
                self.upload(ops, state())
        self.assertEqual(error.exception.code, 'deadline')
        self.assertEqual(ops.calls[-1], 'release')

    def test_changed_local_or_other_scope_stops_before_auth(self):
        ops = Operations(state())
        def changed():
            raise ValueError('private data must not escape')
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(), assert_local_unchanged=changed)
        self.assertEqual(error.exception.code, 'local_changed')
        self.assertNotIn('private', str(error.exception))
        self.assertEqual(ops.calls, [])
        ops.scope = dataclasses.replace(SCOPE, xuid='2')
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state())
        self.assertEqual(error.exception.code, 'invalid_scope')

    def test_empty_state_is_explicitly_verified_and_atoms_are_bounded(self):
        ops = Operations(state(one=b'old'))
        receipt = self.upload(ops, state())
        self.assertEqual(receipt.changed_containers, 1)
        self.assertEqual(receipt.total_bytes, 0)
        ops = Operations(state())
        ops.upload_atom = lambda *args, **kwargs: 'https://not-an-atom.invalid/token'
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(one=b'new'))
        self.assertEqual(error.exception.code, 'invalid_response')
        self.assertNotIn('put', ops.calls)

    def test_selective_readback_names_include_each_commit_and_final_union(self):
        ops = Operations(state(keep=b'same', remove=b'old', replace=b'old'))
        target = state(keep=b'same', replace=b'new', add=b'new')
        requested = []
        def committed(*, timeout, container_names):
            self.assertIs(type(container_names), frozenset)
            requested.append(container_names)
            return ops.read(timeout=timeout)
        receipt = self.upload(ops, target, read_committed=committed)
        self.assertEqual(requested, [frozenset({'add'}), frozenset({'remove'}),
            frozenset({'replace'}), frozenset({'add', 'remove', 'replace'})])
        self.assertEqual(ops.calls.count('read'), 5)
        self.assertEqual(receipt.changed_containers, 3)
        self.assertTrue(write.verify_receipt(receipt, scope_binding=SCOPE.binding,
                                            source_content_digest=saves.content_digest(target)))

    def test_selective_noop_still_requires_two_live_comparisons(self):
        ops = Operations(state(one=b'same'))
        def committed(**kwargs):
            self.fail('No write was attempted, so no forced-container callback is needed.')
        receipt = self.upload(ops, copy(ops.remote), read_committed=committed)
        self.assertEqual(ops.calls.count('read'), 2)
        self.assertEqual(receipt.changed_containers, 0)
        self.assertTrue(receipt.committed)
        self.assertNotIn('atom', ops.calls)

    def test_selective_missing_commit_never_accepts_upload_success_status(self):
        ops = Operations(state(one=b'old'))
        ops.fault = 'missing_commit'
        requested = []
        def committed(*, timeout, container_names):
            requested.append(container_names)
            return ops.read(timeout=timeout)
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(one=b'new'), read_committed=committed)
        self.assertEqual(requested, [frozenset({'one'})])
        self.assertEqual(error.exception.code, 'readback')
        self.assertTrue(error.exception.recovery_required)
        self.assertEqual(error.exception.committed_containers, 0)

    def test_selective_lost_reply_is_read_back_without_replaying_commit(self):
        ops = Operations(state(one=b'old'))
        ops.fault = 'lost_reply'
        requested = []
        def committed(*, timeout, container_names):
            requested.append(container_names)
            return ops.read(timeout=timeout)
        receipt = self.upload(ops, state(one=b'new'), read_committed=committed)
        self.assertEqual(requested, [frozenset({'one'}), frozenset({'one'})])
        self.assertEqual(ops.calls.count('put'), 1)
        self.assertEqual(receipt.changed_containers, 1)

    def test_selective_final_read_requires_complete_namespace(self):
        ops = Operations(state(one=b'old', unchanged=b'same'))
        count = [0]
        def committed(*, timeout, container_names):
            count[0] += 1
            result = ops.read(timeout=timeout)
            if count[0] == 2:
                del result.containers['unchanged']
            return result
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(one=b'new', unchanged=b'same'), read_committed=committed)
        self.assertEqual(error.exception.code, 'readback')
        self.assertEqual(error.exception.committed_containers, 1)
        self.assertTrue(error.exception.recovery_required)

    def test_selective_read_preserves_partial_error_and_deadline_facts(self):
        for code in ('authentication', 'deadline', 'cancelled', 'changed'):
            with self.subTest(code=code):
                ops = Operations(state(first=b'old', second=b'old'))
                def committed(*, timeout, container_names):
                    if container_names == frozenset({'second'}):
                        raise CloudStorageError(code)
                    return ops.read(timeout=timeout)
                with self.assertRaises(write.CloudWriteError) as error:
                    self.upload(ops, state(first=b'new', second=b'new'), read_committed=committed)
                self.assertEqual(error.exception.committed_containers, 1)
                self.assertTrue(error.exception.recovery_required)
                self.assertEqual(ops.calls.count('put'), 2)

    def test_selective_late_callback_cannot_produce_verified_receipt(self):
        ops, clock = Operations(state(one=b'old')), [0]
        def committed(*, timeout, container_names):
            self.assertLessEqual(timeout, 600)
            clock[0] = 601
            return copy(ops.remote)
        with patch('flightdeck.cloud_write.time.monotonic', side_effect=lambda: clock[0]):
            with self.assertRaises(write.CloudWriteError) as error:
                self.upload(ops, state(one=b'new'), read_committed=committed)
        self.assertEqual(error.exception.code, 'deadline')
        self.assertEqual(error.exception.committed_containers, 0)
        self.assertTrue(error.exception.recovery_required)

    def test_selective_callback_is_validated_before_any_network_operation(self):
        ops = Operations(state())
        with self.assertRaises(write.CloudWriteError) as error:
            self.upload(ops, state(), read_committed=True)
        self.assertEqual(error.exception.code, 'invalid_input')
        self.assertEqual(ops.calls, [])

    def test_selective_callback_receives_exact_logical_name_not_wire_suffix(self):
        ops = Operations(state())
        name = 'profile/savedgame'
        names = []
        def committed(*, timeout, container_names):
            names.append(container_names)
            return ops.read(timeout=timeout)
        receipt = self.upload(ops, state(**{name: b'new'}), read_committed=committed)
        self.assertEqual(names, [frozenset({name}), frozenset({name})])
        self.assertEqual(receipt.changed_containers, 1)


if __name__ == '__main__':
    unittest.main()

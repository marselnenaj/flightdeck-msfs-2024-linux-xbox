# SPDX-License-Identifier: MIT
"""Pure policy tests: no filesystem, account, process or cloud operation."""
from dataclasses import FrozenInstanceError
import hashlib
import unittest

from flightdeck import cloud_policy as policy, save_state

SCOPE = "a" * 64
OTHER = "b" * 64


def state(**entries):
    return save_state.State(7, {name: save_state.Container(name, 100, {"data": data})
                               for name, data in entries.items()})


def saves(**entries):
    return policy.SaveSet.from_state(SCOPE, state(**entries))


class CloudPolicyTests(unittest.TestCase):
    def decide(self, local, remote, baseline=None):
        return policy.decide(scope_binding=SCOPE, local=local, remote=remote, baseline=baseline)

    def test_first_nonempty_cloud_wins_with_backup_even_over_local_only_containers(self):
        remote = saves(profile=b"cloud")
        result = self.decide(saves(profile=b"local", extra=b"only local"), remote)
        self.assertEqual((result.action, result.reason), ("import_cloud", "first_cloud"))
        self.assertEqual(result.target, remote)
        self.assertTrue(result.backup_required)
        self.assertTrue(result.baseline_required)
        self.assertEqual([(s.name, s.source, s.digest is None) for s in result.selections],
                         [("extra", "cloud", True), ("profile", "cloud", False)])

    def test_first_confirmed_empty_cloud_keeps_and_uploads_local(self):
        local = saves(profile=b"local")
        result = self.decide(local, saves())
        self.assertEqual((result.action, result.reason, result.target),
                         ("upload_local", "first_local", local))
        self.assertTrue(result.backup_required)

    def test_first_local_empty_imports_cloud(self):
        self.assertEqual(self.decide(saves(), saves(profile=b"cloud")).action, "import_cloud")

    def test_first_both_empty_need_common_baseline_but_no_mutation(self):
        result = self.decide(saves(), saves())
        self.assertEqual(result.action, "noop")
        self.assertTrue(result.baseline_required)
        self.assertFalse(result.backup_required)

    def test_first_same_nonempty_content_needs_verified_common_baseline(self):
        result = self.decide(saves(profile=b"same"), saves(profile=b"same"))
        self.assertEqual(result.action, "noop")
        self.assertTrue(result.baseline_required)

    def test_missing_or_failed_observation_never_means_empty_or_deletion(self):
        for local, remote in [(saves(profile=b"local"), None), (None, saves()), (None, None)]:
            for baseline in (None, saves(profile=b"old")):
                with self.subTest(local=local is not None, remote=remote is not None, baseline=baseline is not None):
                    result = self.decide(local, remote, baseline)
                    self.assertEqual(result.action, "blocked")
                    self.assertIsNone(result.target)
                    self.assertEqual(result.selections, ())
                    self.assertFalse(result.baseline_required)
                    self.assertFalse(result.backup_required)

    def test_all_input_scope_bindings_are_checked_even_when_contents_equal(self):
        matching, wrong = saves(), policy.SaveSet(OTHER)
        for local, remote, baseline in [(wrong, matching, None), (matching, wrong, None),
                                         (matching, matching, wrong), (None, matching, wrong)]:
            with self.subTest(local=local, remote=remote, baseline=baseline):
                result = self.decide(local, remote, baseline)
                self.assertEqual((result.action, result.reason), ("blocked", "scope_mismatch"))

    def test_only_local_changed_uploads(self):
        baseline = saves(profile=b"old")
        result = self.decide(saves(profile=b"local"), baseline, baseline)
        self.assertEqual((result.action, result.reason), ("upload_local", "local_changed"))
        self.assertTrue(result.backup_required)

    def test_only_remote_changed_imports_with_backup(self):
        baseline = saves(profile=b"old")
        result = self.decide(baseline, saves(profile=b"cloud"), baseline)
        self.assertEqual((result.action, result.reason), ("import_cloud", "remote_changed"))
        self.assertTrue(result.backup_required)

    def test_neither_changed_preserves_existing_baseline(self):
        baseline = saves(profile=b"same")
        result = self.decide(baseline, baseline, baseline)
        self.assertEqual(result.action, "noop")
        self.assertFalse(result.baseline_required)

    def test_both_changed_identically_advance_common_baseline_without_writes(self):
        current = saves(profile=b"new")
        result = self.decide(current, current, saves(profile=b"old"))
        self.assertEqual(result.action, "noop")
        self.assertTrue(result.baseline_required)
        self.assertFalse(result.backup_required)

    def test_conflicting_edits_have_no_partial_target_or_mutation_plan(self):
        result = self.decide(saves(profile=b"local", localextra=b"a"),
                             saves(profile=b"remote", remoteextra=b"b"), saves(profile=b"old"))
        self.assertEqual((result.action, result.conflicts), ("conflict", ("profile",)))
        self.assertIsNone(result.target)
        self.assertEqual(result.selections, ())
        self.assertFalse(result.backup_required)
        self.assertFalse(result.baseline_required)

    def test_disjoint_container_edits_and_additions_merge_without_blob_merging(self):
        baseline = saves(first=b"one", second=b"two")
        local = saves(first=b"one changed", second=b"two", localnew=b"a")
        remote = saves(first=b"one", second=b"two changed", cloudnew=b"b")
        result = self.decide(local, remote, baseline)
        self.assertEqual(result.action, "merge")
        self.assertEqual(result.target, saves(first=b"one changed", second=b"two changed", localnew=b"a", cloudnew=b"b"))
        self.assertEqual({s.name:s.source for s in result.selections},
                         {"first":"local", "second":"cloud", "localnew":"local", "cloudnew":"cloud"})
        self.assertTrue(result.backup_required)
        self.assertTrue(result.baseline_required)

    def test_local_deletion_after_baseline_is_upload_not_first_cloud_restore(self):
        old = saves(profile=b"old")
        result = self.decide(saves(), old, old)
        self.assertEqual(result.action, "upload_local")
        self.assertEqual(result.target, saves())
        self.assertIsNone(result.selections[0].digest)

    def test_remote_deletion_after_baseline_imports_empty_with_backup(self):
        old = saves(profile=b"old")
        result = self.decide(old, saves(), old)
        self.assertEqual(result.action, "import_cloud")
        self.assertTrue(result.backup_required)
        self.assertEqual(result.target, saves())

    def test_both_delete_is_equal_and_advances_baseline(self):
        result = self.decide(saves(), saves(), saves(profile=b"old"))
        self.assertEqual(result.action, "noop")
        self.assertTrue(result.baseline_required)

    def test_delete_versus_edit_is_conflict_in_both_directions(self):
        old = saves(profile=b"old")
        for local, remote in [(saves(), saves(profile=b"new")), (saves(profile=b"new"), saves())]:
            self.assertEqual(self.decide(local, remote, old).action, "conflict")

    def test_independent_deletions_merge_to_empty(self):
        result = self.decide(saves(first=b"1"), saves(second=b"2"), saves(first=b"1", second=b"2"))
        self.assertEqual(result.action, "merge")
        self.assertEqual(result.target, saves())
        self.assertTrue(all(item.digest is None for item in result.selections))

    def test_merge_target_matches_existing_import_executor_with_empty_choice(self):
        # Exercise the real pure selection helpers without filesystem or cloud.
        from types import SimpleNamespace
        from flightdeck.cloud_import import _decisions, _select
        baseline = state(first=b"one", second=b"two", deleted=b"old")
        local = state(first=b"edited", second=b"two", localnew=b"a")
        remote = state(first=b"one", second=b"remote edit", deleted=b"old", cloudnew=b"b")
        baseline_set = policy.SaveSet.from_state(SCOPE, baseline)
        result = self.decide(policy.SaveSet.from_state(SCOPE, local),
                             policy.SaveSet.from_state(SCOPE, remote), baseline_set)
        self.assertEqual(result.action, "merge")
        plan = SimpleNamespace(decisions=_decisions(local, remote, dict(baseline_set.containers)))
        selected = _select(local, remote, plan, {})
        self.assertEqual(policy.SaveSet.from_state(SCOPE, selected), result.target)
        self.assertNotIn("deleted", selected.containers)

    def test_different_new_content_under_same_name_is_conflict(self):
        result = self.decide(saves(profile=b"a"), saves(profile=b"b"), saves())
        self.assertEqual(result.action, "conflict")

    def test_logical_digests_ignore_generation_and_timestamp_and_match_import_contract(self):
        original = state(profile=b"same")
        expected = save_state.content_digest(save_state.State(0, {"profile": original.containers["profile"]}))
        before = policy.SaveSet.from_state(SCOPE, original)
        original.generation += 1
        original.containers["profile"].modified += 999
        after = policy.SaveSet.from_state(SCOPE, original)
        self.assertEqual(before, after)
        self.assertEqual(before.containers, (("profile", expected),))
        original.containers["profile"].blobs["data"] = b"different"
        self.assertNotEqual(before, policy.SaveSet.from_state(SCOPE, original))

    def test_evidence_is_canonical_and_immutable(self):
        a, b = hashlib.sha256(b"a").hexdigest(), hashlib.sha256(b"b").hexdigest()
        evidence = policy.SaveSet(SCOPE, (("z",b),("a",a)))
        self.assertEqual(evidence.containers, (("a",a),("z",b)))
        with self.assertRaises(FrozenInstanceError):evidence.containers = ()

    def test_invalid_evidence_never_leaks_names_or_is_treated_as_absent_baseline(self):
        secret = "private-save-name"
        for binding, entries in [("invalid", ()), (SCOPE, [(secret,"a"*64)]),
                (SCOPE, ((secret,"bad"),)), (SCOPE, ((secret,"a"*64),(secret,"a"*64))),
                (SCOPE, (("../private", "a"*64),)), (SCOPE, ((secret,),))]:
            with self.subTest(binding=binding),self.assertRaises(ValueError) as caught:
                policy.SaveSet(binding, entries)
            self.assertNotIn(secret, str(caught.exception))
        with self.assertRaises(ValueError):
            policy.decide(scope_binding="invalid",local=None,remote=None)
        with self.assertRaises(ValueError):self.decide(saves(),saves(),{})

    def test_merged_container_bound_fails_closed(self):
        size = save_state.CONTAINER_LIMIT
        local = policy.SaveSet(SCOPE, tuple((f"l{i}","a"*64) for i in range(size)))
        remote = policy.SaveSet(SCOPE, tuple((f"r{i}","b"*64) for i in range(size)))
        result = self.decide(local, remote, saves())
        self.assertEqual((result.action,result.reason), ("blocked","bounds"))
        self.assertIsNone(result.target)
        self.assertEqual(result.selections, ())


if __name__ == "__main__":
    unittest.main()

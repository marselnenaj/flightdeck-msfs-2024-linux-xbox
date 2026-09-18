# SPDX-License-Identifier: MIT
"""Pure, container-level policy for automatic Xbox save comparison.

Inputs are complete, already verified observations of the same game profile.
``None`` for local/remote means unavailable, never an empty namespace. Only a
successful read with no containers may be represented by an empty ``SaveSet``.
``baseline=None`` means a verified absence of a historical common-state receipt;
authentication, corrupt receipts and pending-transaction recovery must be dealt
with by the caller before calling this policy.

Decisions authorize no I/O themselves. The caller retains the runtime/native
writer locks, makes the required local backup, rechecks revisions, applies the
existing import/lease-write contracts and persists only a verified baseline.
Container digests deliberately use the same logical digest as cloud_import and
cloud_write; timestamps and native generations never select a winning side.
"""
from __future__ import annotations

from dataclasses import dataclass
import re

from . import save_state

_DIGEST = re.compile(r"[0-9a-f]{64}\Z")


def _digest(value):
    return isinstance(value, str) and _DIGEST.fullmatch(value) is not None


@dataclass(frozen=True, repr=False)
class SaveSet:
    """Immutable content fingerprints; not an authentication or receipt proof."""

    scope_binding: str
    containers: tuple[tuple[str, str], ...] = ()

    def __post_init__(self):
        if (not _digest(self.scope_binding) or type(self.containers) is not tuple
                or len(self.containers) > save_state.CONTAINER_LIMIT):
            raise ValueError("Invalid cloud sync evidence.")
        names = set()
        for entry in self.containers:
            if type(entry) is not tuple or len(entry) != 2:
                raise ValueError("Invalid cloud sync evidence.")
            name, digest = entry
            try:
                save_state._name(name, container=True)
            except save_state.SaveStateError:
                raise ValueError("Invalid cloud sync evidence.") from None
            if name in names or not _digest(digest):
                raise ValueError("Invalid cloud sync evidence.")
            names.add(name)
        object.__setattr__(self, "containers", tuple(sorted(self.containers)))

    @classmethod
    def from_state(cls, scope_binding, state):
        """Fingerprint an already read state, validating its complete format."""
        # Validation is bounded by the native format's quota/metadata limits.
        # The copy also detaches the result from the caller's mutable State.
        checked = save_state.decode(save_state.encode(state))
        return cls(scope_binding, tuple(
            (name, save_state.content_digest(save_state.State(0, {name: entry})))
            for name, entry in checked.containers.items()))


@dataclass(frozen=True, repr=False)
class Selection:
    name: str
    source: str  # local or cloud; digest=None is a confirmed deletion.
    digest: str | None


@dataclass(frozen=True, repr=False)
class Decision:
    action: str  # noop, import_cloud, upload_local, merge, conflict, blocked
    reason: str
    target: SaveSet | None = None
    selections: tuple[Selection, ...] = ()
    conflicts: tuple[str, ...] = ()
    backup_required: bool = False
    baseline_required: bool = False


def decide(*, scope_binding: str, local: SaveSet | None,
           remote: SaveSet | None, baseline: SaveSet | None = None) -> Decision:
    """Choose a whole plan, never a partial merge around unresolved conflicts.

    On first sync a nonempty cloud wins, with a local backup. A confirmed empty
    cloud never deletes first-sync local data: that local state is uploaded.
    Subsequently each container is compared with its last verified common
    digest; independent edits can merge, same-container divergent edits cannot.

    A merge means the existing import plan's automatic ``choice={}``, followed
    by upload of the merged state against the freshly locked remote revision.
    The caller must validate full merged-state size before either mutation.
    A noop with ``baseline_required`` still needs verified common-state evidence
    (for example a zero-change lease/readback receipt), not a fabricated receipt.
    """
    if not _digest(scope_binding):
        raise ValueError("Invalid cloud sync scope.")
    for value in (local, remote, baseline):
        if value is not None and type(value) is not SaveSet:
            raise ValueError("Invalid cloud sync evidence.")
        if value is not None and value.scope_binding != scope_binding:
            return Decision("blocked", "scope_mismatch")
    if local is None:
        return Decision("blocked", "local_unavailable")
    if remote is None:
        return Decision("blocked", "remote_unavailable")

    left, right = dict(local.containers), dict(remote.containers)
    old = None if baseline is None else dict(baseline.containers)
    names = sorted(left.keys() | right.keys() | (old.keys() if old is not None else set()))

    if left == right:
        return Decision("noop", "already_equal", local,
                        tuple(Selection(name, "local", left.get(name)) for name in names),
                        baseline_required=old != left)

    if old is None:
        cloud_first = bool(right)
        selected = right if cloud_first else left
        source = "cloud" if cloud_first else "local"
        return Decision("import_cloud" if cloud_first else "upload_local",
                        "first_cloud" if cloud_first else "first_local",
                        remote if cloud_first else local,
                        tuple(Selection(name, source, selected.get(name)) for name in names),
                        backup_required=True, baseline_required=True)

    selections, conflicts, target = [], [], {}
    for name in names:
        a, b, previous = left.get(name), right.get(name), old.get(name)
        if a == b:
            source, digest = "local", a
        elif a == previous:
            source, digest = "cloud", b
        elif b == previous:
            source, digest = "local", a
        else:
            conflicts.append(name)
            continue
        selections.append(Selection(name, source, digest))
        if digest is not None:
            target[name] = digest

    if conflicts:
        return Decision("conflict", "diverged", conflicts=tuple(conflicts))
    if len(target) > save_state.CONTAINER_LIMIT:
        return Decision("blocked", "bounds")
    desired = SaveSet(scope_binding, tuple(target.items()))
    if target == right:
        action, reason = "import_cloud", "remote_changed"
    elif target == left:
        action, reason = "upload_local", "local_changed"
    else:
        action, reason = "merge", "independent_changes"
    return Decision(action, reason, desired, tuple(selections),
                    backup_required=True, baseline_required=True)

// SPDX-License-Identifier: MIT
//! Pure comparison policy. A decision is not authorization to write local or cloud data.
use crate::{
    Result,
    error::require,
    files::hex_digest,
    save_state::{self, CONTAINER_LIMIT, State},
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, PartialEq, Eq)]
pub struct SaveSet {
    binding: String,
    containers: BTreeMap<String, String>,
}
impl SaveSet {
    pub fn new(binding: &str, containers: BTreeMap<String, String>) -> Result<Self> {
        require(
            hex_digest(binding) && containers.len() <= CONTAINER_LIMIT,
            "Ungültige Cloud-Sync-Daten.",
        )?;
        for (name, digest) in &containers {
            save_state::name(name, true)?;
            require(hex_digest(digest), "Ungültige Cloud-Sync-Daten.")?;
        }
        Ok(Self {
            binding: binding.into(),
            containers,
        })
    }
    pub fn from_state(binding: &str, state: &State) -> Result<Self> {
        save_state::encode(state)?;
        let mut containers = BTreeMap::new();
        for (key, entry) in &state.containers {
            let one = State {
                generation: 0,
                containers: [(key.clone(), entry.clone())].into(),
            };
            containers.insert(key.clone(), save_state::content_digest(&one)?);
        }
        Self::new(binding, containers)
    }
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Noop,
    ImportCloud,
    UploadLocal,
    Merge,
    Conflict,
    Blocked,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Local,
    Cloud,
}
pub struct Selection {
    pub name: String,
    pub source: Source,
    pub digest: Option<String>,
}
pub struct Decision {
    pub action: Action,
    pub reason: &'static str,
    pub target: Option<SaveSet>,
    pub selections: Vec<Selection>,
    pub conflicts: Vec<String>,
    pub backup_required: bool,
    pub baseline_required: bool,
}
impl Decision {
    fn new(action: Action, reason: &'static str) -> Self {
        Self {
            action,
            reason,
            target: None,
            selections: vec![],
            conflicts: vec![],
            backup_required: false,
            baseline_required: false,
        }
    }
}

pub fn decide(
    binding: &str,
    local: Option<&SaveSet>,
    remote: Option<&SaveSet>,
    baseline: Option<&SaveSet>,
) -> Result<Decision> {
    require(hex_digest(binding), "Ungültige Cloud-Sync-Zuordnung.")?;
    for value in [local, remote, baseline].into_iter().flatten() {
        if value.binding != binding {
            return Ok(Decision::new(Action::Blocked, "scope_mismatch"));
        }
    }
    let Some(local) = local else {
        return Ok(Decision::new(Action::Blocked, "local_unavailable"));
    };
    let Some(remote) = remote else {
        return Ok(Decision::new(Action::Blocked, "remote_unavailable"));
    };
    let left = &local.containers;
    let right = &remote.containers;
    let old = baseline.map(|value| &value.containers);
    let names: BTreeSet<_> = left
        .keys()
        .chain(right.keys())
        .chain(old.into_iter().flat_map(|v| v.keys()))
        .cloned()
        .collect();
    if left == right {
        let mut result = Decision::new(Action::Noop, "already_equal");
        result.target = Some(local.clone());
        result.baseline_required = old != Some(left);
        result.selections = names
            .into_iter()
            .map(|name| Selection {
                digest: left.get(&name).cloned(),
                name,
                source: Source::Local,
            })
            .collect();
        return Ok(result);
    }
    let Some(old) = old else {
        let (selected, source, action, reason) = if right.is_empty() {
            (local, Source::Local, Action::UploadLocal, "first_local")
        } else {
            (remote, Source::Cloud, Action::ImportCloud, "first_cloud")
        };
        let mut result = Decision::new(action, reason);
        result.target = Some(selected.clone());
        result.backup_required = true;
        result.baseline_required = true;
        result.selections = names
            .into_iter()
            .map(|name| Selection {
                digest: selected.containers.get(&name).cloned(),
                name,
                source,
            })
            .collect();
        return Ok(result);
    };
    let mut selections = vec![];
    let mut conflicts = vec![];
    let mut target = BTreeMap::new();
    for name in names {
        let a = left.get(&name);
        let b = right.get(&name);
        let previous = old.get(&name);
        let (source, digest) = if a == b {
            (Source::Local, a)
        } else if a == previous {
            (Source::Cloud, b)
        } else if b == previous {
            (Source::Local, a)
        } else {
            conflicts.push(name);
            continue;
        };
        if let Some(digest) = digest {
            target.insert(name.clone(), digest.clone());
        }
        selections.push(Selection {
            name,
            source,
            digest: digest.cloned(),
        });
    }
    if !conflicts.is_empty() {
        let mut result = Decision::new(Action::Conflict, "diverged");
        result.conflicts = conflicts;
        return Ok(result);
    }
    if target.len() > CONTAINER_LIMIT {
        return Ok(Decision::new(Action::Blocked, "bounds"));
    }
    let (action, reason) = if &target == right {
        (Action::ImportCloud, "remote_changed")
    } else if &target == left {
        (Action::UploadLocal, "local_changed")
    } else {
        (Action::Merge, "independent_changes")
    };
    let mut result = Decision::new(action, reason);
    result.target = Some(SaveSet::new(binding, target)?);
    result.selections = selections;
    result.backup_required = true;
    result.baseline_required = true;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn set(entries: &[(&str, &str)]) -> SaveSet {
        SaveSet::new(
            &"a".repeat(64),
            entries
                .iter()
                .map(|(key, value)| (key.to_string(), crate::files::sha256(value.as_bytes())))
                .collect(),
        )
        .expect("fixture")
    }
    #[test]
    fn unknown_is_never_empty() {
        let local = set(&[("profile", "data")]);
        let decision = decide(&"a".repeat(64), Some(&local), None, None).expect("decision");
        assert_eq!(decision.action, Action::Blocked);
        assert!(decision.target.is_none());
    }
    #[test]
    fn divergent_edits_do_not_publish_partial_merge() {
        let old = set(&[("profile", "old")]);
        let a = set(&[("profile", "a"), ("extra", "yes")]);
        let b = set(&[("profile", "b")]);
        let decision = decide(&"a".repeat(64), Some(&a), Some(&b), Some(&old)).expect("decision");
        assert_eq!(decision.action, Action::Conflict);
        assert!(decision.target.is_none());
        assert!(decision.selections.is_empty());
    }
    #[test]
    fn disjoint_changes_merge_and_require_backup() {
        let old = set(&[("a", "old"), ("b", "old")]);
        let a = set(&[("a", "new"), ("b", "old")]);
        let b = set(&[("a", "old"), ("b", "new")]);
        let decision = decide(&"a".repeat(64), Some(&a), Some(&b), Some(&old)).expect("decision");
        assert_eq!(decision.action, Action::Merge);
        assert!(decision.backup_required && decision.baseline_required);
        assert!(decision.target == Some(set(&[("a", "new"), ("b", "new")])));
    }
    #[test]
    fn empty_first_cloud_preserves_local_and_scope_is_always_checked() {
        let local = set(&[("profile", "data")]);
        let empty = set(&[]);
        assert_eq!(
            decide(&"a".repeat(64), Some(&local), Some(&empty), None)
                .expect("decision")
                .action,
            Action::UploadLocal
        );
        assert_eq!(
            decide(&"b".repeat(64), Some(&empty), Some(&empty), None)
                .expect("decision")
                .action,
            Action::Blocked
        );
    }
}

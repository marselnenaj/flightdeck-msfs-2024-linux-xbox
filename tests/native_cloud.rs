// SPDX-License-Identifier: MIT
use flightdeck::{
    cloud::{self, Failure, Result},
    cloud_import as ci,
    cloud_storage::{self as cs, ReadOperation, Response, Scope, Transport},
    cloud_write as cw, files,
    save_state::{Container, State},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
fn scope() -> Scope {
    Scope::new(
        "123",
        "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        "Test_abc",
        123,
    )
    .expect("scope")
}
fn state(payload: &[u8]) -> State {
    State {
        generation: 3,
        containers: [(
            "profile".into(),
            Container {
                display_name: "Pilot".into(),
                modified: 123,
                blobs: [("data".into(), payload.to_vec())].into(),
            },
        )]
        .into(),
    }
}
fn json_response(value: Value) -> Response {
    Response {
        status: 200,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&value).expect("json"),
    }
}
fn inventory(etag: &str) -> Value {
    json!({"blobs":[{"fileName":"profile,savedgame","displayName":"Pilot","etag":etag,"clientFileTime":116444737230000000u64,"size":3}],"pagingInfo":{"continuationToken":null,"totalItems":1}})
}
const ATOM: &str = "ABCDEFAB-1111-2222-3333-1234567890ab";
struct Reader {
    scope: Scope,
    replies: VecDeque<Response>,
    requests: Vec<String>,
}
impl Reader {
    fn standard() -> Self {
        Self {
            scope: scope(),
            requests: vec![],
            replies: vec![
                json_response(inventory("one")),
                json_response(json!({"atoms":{"data":format!("{ATOM},binary")}})),
                Response {
                    status: 200,
                    headers: BTreeMap::new(),
                    body: b"new".to_vec(),
                },
                json_response(json!({"atoms":{"data":format!("{ATOM},binary")}})),
                json_response(inventory("one")),
            ]
            .into(),
        }
    }
}
impl Transport for Reader {
    fn scope(&self) -> &Scope {
        &self.scope
    }
    fn read(&mut self, op: ReadOperation<'_>, _: Duration, _: usize) -> Result<Response> {
        self.requests.push(match op {
            ReadOperation::Index { skip, continuation } => {
                format!("index:{skip}:{}", continuation.unwrap_or(""))
            }
            ReadOperation::Container(name) => format!("container:{name}"),
            ReadOperation::Atom(name) => format!("atom:{name}"),
        });
        self.replies
            .pop_front()
            .ok_or_else(|| Failure::new("transport"))
    }
}
fn runtime() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temp");
    for p in ["private", "private/local-saves", "private/cloud-saves"] {
        fs::create_dir_all(root.path().join(p)).expect("directory");
        fs::set_permissions(root.path().join(p), fs::Permissions::from_mode(0o700)).expect("mode");
    }
    files::atomic(
        &root.path().join("private/local-saves.enabled"),
        b"enabled\n",
    )
    .expect("marker");
    root
}
fn snapshot(root: &Path) -> cs::Snapshot {
    cs::download(
        &mut Reader::standard(),
        &root.join("private/cloud-saves"),
        &cs::Limits::default(),
        &AtomicBool::new(false),
    )
    .expect("snapshot")
}
#[test]
fn strict_protocol_rejects_duplicate_keys_and_nonnumeric_integers() {
    for raw in [
        br#"{"a":1,"a":2}"#.as_slice(),
        br#"{"a":{"b":1,"b":2}}"#,
        br#"{"a":NaN}"#,
        b"[]",
        br#"{"a":1} garbage"#,
    ] {
        assert!(cloud::json(raw).is_err());
    }
    assert!(cloud::number(&json!(true), 10).is_err());
    assert!(cloud::number(&json!(1.0), 10).is_err());
    let mut reply = json!({"xuid":"123","scid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","package_family_name":"Test_abc","title_id":123});
    assert!(Scope::from_reply(&reply).is_ok());
    reply["unexpected_account_data"] = json!("discard");
    assert!(Scope::from_reply(&reply).is_err());
    assert!(
        Scope::new(
            "18446744073709551616",
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "Test",
            123
        )
        .is_err()
    );
}
#[test]
fn exact_filetime_and_opaque_atom_case_are_preserved() {
    assert_eq!(
        cs::file_time(&json!("1970-01-01T01:00:00.0000001+01:00")).expect("ticks"),
        116444736000000001
    );
    for stamp in [
        "2026-01-01T00:00:60Z",
        "2026-01-01T00:00:00+14:01",
        "2026-01-01T00:00:00.00000001Z",
        "1600-01-01T00:00:00Z",
    ] {
        assert!(cs::file_time(&json!(stamp)).is_err());
    }
    let root = runtime();
    let mut reader = Reader::standard();
    let snap = cs::download(
        &mut reader,
        &root.path().join("private/cloud-saves"),
        &cs::Limits::default(),
        &AtomicBool::new(false),
    )
    .expect("download");
    assert!(reader.requests.contains(&format!("atom:{ATOM}")));
    assert!(
        reader
            .requests
            .contains(&"container:profile,savedgame".into())
    );
    assert_eq!(
        ci::read_snapshot(root.path(), &scope(), &snap)
            .expect("read")
            .containers["profile"]
            .blobs["data"],
        b"new"
    );
}
#[test]
fn missing_storage_and_changing_index_never_publish_empty_snapshots() {
    let root = runtime();
    let mut reader = Reader::standard();
    reader.replies[0].status = 404;
    let err = cs::download(
        &mut reader,
        &root.path().join("private/cloud-saves"),
        &cs::Limits::default(),
        &AtomicBool::new(false),
    )
    .err()
    .expect("404");
    assert_eq!(err.code, "not_found");
    let mut reader = Reader::standard();
    reader.replies[4] = json_response(inventory("changed"));
    let err = cs::download(
        &mut reader,
        &root.path().join("private/cloud-saves"),
        &cs::Limits::default(),
        &AtomicBool::new(false),
    )
    .err()
    .expect("changed");
    assert_eq!(err.code, "changed");
    assert_eq!(
        fs::read_dir(root.path().join("private/cloud-saves"))
            .expect("list")
            .count(),
        0
    );
}
#[test]
fn paged_inventory_checks_duplicate_names_and_opaque_continuations() {
    let mut first = inventory("one");
    first["pagingInfo"] = json!({"continuationToken":"token&?+/☃","totalItems":2});
    let mut second = inventory("two");
    second["blobs"][0]["fileName"] = json!("other,savedgame");
    second["pagingInfo"]["totalItems"] = json!(2);
    let mut reader = Reader {
        scope: scope(),
        requests: vec![],
        replies: vec![json_response(first.clone()), json_response(second)].into(),
    };
    assert_eq!(
        cs::inventory(&mut reader, &cs::Limits::default(), &AtomicBool::new(false))
            .expect("pages")
            .len(),
        2
    );
    assert_eq!(reader.requests[1], "index:1:token&?+/☃");
    let mut duplicate = inventory("two");
    duplicate["pagingInfo"]["totalItems"] = json!(2);
    let mut reader = Reader {
        scope: scope(),
        requests: vec![],
        replies: vec![json_response(first), json_response(duplicate)].into(),
    };
    assert!(cs::inventory(&mut reader, &cs::Limits::default(), &AtomicBool::new(false)).is_err());
}
#[test]
fn imports_backup_increment_generation_and_restore_only_exact_revision() {
    let root = runtime();
    let snap = snapshot(root.path());
    let lease = files::Lease::acquire(&root.path().join("private/play.lock"), true).expect("lease");
    let cancel = AtomicBool::new(false);
    let plan = ci::prepare(root.path(), &scope(), &snap, None).expect("plan");
    assert_eq!(plan.summary()["add_count"], 1);
    let applied = ci::apply(
        root.path(),
        &scope(),
        &plan,
        &ci::Choice::Automatic,
        &lease.0,
        &cancel,
    )
    .expect("apply");
    assert!(applied.imported && applied.durability_confirmed);
    assert_eq!(applied.generation, 1);
    assert!(
        ci::load_baseline(root.path(), &scope())
            .expect("baseline")
            .is_some()
    );
    assert_eq!(
        ci::apply(
            root.path(),
            &scope(),
            &plan,
            &ci::Choice::Cloud,
            &lease.0,
            &cancel
        )
        .err()
        .expect("stale")
        .code,
        "changed"
    );
    let id = applied.backup_id.expect("backup");
    let restored = ci::restore(root.path(), &scope(), &id, &lease.0, &cancel).expect("restore");
    assert_eq!(restored.generation, 2);
    assert_eq!(restored.container_count, 0);
    assert_eq!(
        ci::restore(root.path(), &scope(), &id, &lease.0, &cancel)
            .err()
            .expect("stale restore")
            .code,
        "changed"
    );
}
#[test]
fn changed_snapshot_wrong_scope_and_symlinks_cannot_import() {
    let root = runtime();
    let snap = snapshot(root.path());
    let lease = files::Lease::acquire(&root.path().join("private/play.lock"), true).expect("lease");
    let cancel = AtomicBool::new(false);
    let plan = ci::prepare(root.path(), &scope(), &snap, None).expect("plan");
    files::atomic(&snap.path.join("blobs/00000000.bin"), b"bad").expect("tamper");
    assert!(
        ci::apply(
            root.path(),
            &scope(),
            &plan,
            &ci::Choice::Cloud,
            &lease.0,
            &cancel
        )
        .is_err()
    );
    assert!(
        ci::read_snapshot(
            root.path(),
            &Scope::new("456", scope().scid(), "Test_abc", 123).expect("other"),
            &snap
        )
        .is_err()
    );
    let linked = root.path().join("alias");
    symlink(root.path().join("private/cloud-saves"), &linked).expect("link");
    assert!(
        cs::download(
            &mut Reader::standard(),
            &linked,
            &cs::Limits::default(),
            &cancel
        )
        .is_err()
    );
}
#[test]
fn export_requires_own_lease_and_holds_native_writer_lock() {
    let root = runtime();
    let scope = scope();
    let lease = files::Lease::acquire(&root.path().join("private/play.lock"), true).expect("lease");
    let cancel = AtomicBool::new(false);
    let other = fs::File::open(root.path().join("private/play.lock")).expect("unowned fd");
    assert_eq!(
        ci::export_local(root.path(), &scope, &other, &cancel)
            .err()
            .expect("wrong lease")
            .code,
        "invalid_lock"
    );
    let export = ci::export_local(root.path(), &scope, &lease.0, &cancel).expect("export");
    assert_eq!(
        ci::export_local(root.path(), &scope, &lease.0, &cancel)
            .err()
            .expect("busy")
            .code,
        "busy"
    );
    export.assert_unchanged().expect("valid");
    drop(export);
    ci::export_local(root.path(), &scope, &lease.0, &cancel).expect("lock released");
}

struct Writer {
    scope: Scope,
    state: State,
    payloads: BTreeMap<String, Vec<u8>>,
    calls: Vec<&'static str>,
    renew_count: usize,
    lose_at: Option<usize>,
    lost_commit_reply: bool,
    lie_readback: bool,
}
impl Writer {
    fn new() -> Self {
        Self {
            scope: scope(),
            state: state(b"old"),
            payloads: BTreeMap::new(),
            calls: vec![],
            renew_count: 0,
            lose_at: None,
            lost_commit_reply: false,
            lie_readback: false,
        }
    }
}
impl Transport for Writer {
    fn scope(&self) -> &Scope {
        &self.scope
    }
    fn read(&mut self, op: ReadOperation<'_>, _: Duration, _: usize) -> Result<Response> {
        fn id(name: &str, blob: &str) -> String {
            let hash = files::sha256(format!("{name}\0{blob}").as_bytes());
            format!(
                "{}-{}-{}-{}-{}",
                &hash[..8],
                &hash[8..12],
                &hash[12..16],
                &hash[16..20],
                &hash[20..32]
            )
        }
        match op {
            ReadOperation::Index {
                skip: 0,
                continuation: None,
            } => {
                let etag = ci::digest(&self.state)?;
                let rows: Vec<_> = self.state.containers.iter().map(|(name, entry)| json!({"fileName":format!("{name},savedgame"),"displayName":entry.display_name,"etag":etag,"clientFileTime":(entry.modified+11644473600)*10000000,"size":entry.blobs.values().map(Vec::len).sum::<usize>()})).collect();
                Ok(json_response(
                    json!({"blobs":rows,"pagingInfo":{"continuationToken":null,"totalItems":rows.len()}}),
                ))
            }
            ReadOperation::Container(wire) => {
                let name = wire
                    .strip_suffix(",savedgame")
                    .ok_or_else(|| Failure::new("invalid_request"))?;
                let entry = self
                    .state
                    .containers
                    .get(name)
                    .ok_or_else(|| Failure::new("not_found"))?;
                let atoms: BTreeMap<_, _> = entry
                    .blobs
                    .keys()
                    .map(|blob| (blob, format!("{},binary", id(name, blob))))
                    .collect();
                Ok(json_response(json!({"atoms":atoms})))
            }
            ReadOperation::Atom(atom) => {
                for (name, entry) in &self.state.containers {
                    for (blob, bytes) in &entry.blobs {
                        if id(name, blob) == atom {
                            return Ok(Response {
                                status: 200,
                                headers: BTreeMap::new(),
                                body: bytes.clone(),
                            });
                        }
                    }
                }
                Err(Failure::new("not_found"))
            }
            _ => Err(Failure::new("invalid_request")),
        }
    }
}
impl cw::Operations for Writer {
    fn acquire(&mut self, _: Duration) -> Result<cw::LeaseReply> {
        self.calls.push("acquire");
        Ok(cw::LeaseReply {
            status: 200,
            owner_change_id: "owner".into(),
            quota_bytes: 1_000_000,
        })
    }
    fn renew(&mut self, _: Duration) -> Result<cw::LeaseReply> {
        self.calls.push("renew");
        self.renew_count += 1;
        Ok(cw::LeaseReply {
            status: 200,
            owner_change_id: if self.lose_at == Some(self.renew_count) {
                "foreign"
            } else {
                "owner"
            }
            .into(),
            quota_bytes: 1_000_000,
        })
    }
    fn release(&mut self, _: Duration) -> Result<u16> {
        self.calls.push("release");
        Ok(204)
    }
    fn upload_atom(&mut self, payload: &[u8], _: Duration) -> Result<String> {
        self.calls.push("atom");
        self.payloads.insert(ATOM.into(), payload.into());
        Ok(ATOM.into())
    }
    fn put_container(
        &mut self,
        name: &str,
        display: &str,
        modified: u64,
        atoms: &BTreeMap<String, String>,
        _: Duration,
    ) -> Result<u16> {
        self.calls.push("put");
        let blobs = atoms
            .iter()
            .map(|(name, id)| (name.clone(), self.payloads[id].clone()))
            .collect();
        if !self.lie_readback {
            self.state.containers.insert(
                name.strip_suffix(",savedgame").expect("wire").into(),
                Container {
                    display_name: display.into(),
                    modified,
                    blobs,
                },
            );
        }
        if self.lost_commit_reply {
            Err(Failure::new("transport"))
        } else {
            Ok(201)
        }
    }
    fn delete_container(&mut self, name: &str, _: Duration) -> Result<u16> {
        self.calls.push("delete");
        self.state
            .containers
            .remove(name.strip_suffix(",savedgame").expect("wire"));
        Ok(204)
    }
    fn remote(&mut self, _: Duration, force: &BTreeSet<String>) -> Result<State> {
        self.calls.push(if force.is_empty() {
            "read"
        } else {
            "forced-read"
        });
        Ok(self.state.clone())
    }
}
#[test]
fn uncertain_commit_is_read_back_once_without_replaying_mutation() {
    let mut writer = Writer::new();
    writer.lost_commit_reply = true;
    let expected = ci::digest(&writer.state).expect("digest");
    let receipt = cw::upload(
        &scope(),
        &mut writer,
        &state(b"new"),
        &expected,
        || Ok(()),
        &AtomicBool::new(false),
        &cw::Limits::default(),
    )
    .expect("verified");
    assert!(receipt.verifies(
        &scope().binding(),
        &ci::digest(&state(b"new")).expect("digest")
    ));
    assert!(receipt.lease_released());
    assert_eq!(writer.calls.iter().filter(|&&v| v == "put").count(), 1);
    assert!(writer.calls.contains(&"forced-read"));
}
#[test]
fn owner_change_and_stale_remote_never_mutate_or_release_foreign_lease() {
    let mut writer = Writer::new();
    writer.lose_at = Some(1);
    let expected = ci::digest(&writer.state).expect("digest");
    let error = cw::upload(
        &scope(),
        &mut writer,
        &state(b"new"),
        &expected,
        || Ok(()),
        &AtomicBool::new(false),
        &cw::Limits::default(),
    )
    .err()
    .expect("owner change");
    assert_eq!(error.code, "lease_lost");
    assert!(!error.recovery_required);
    assert!(!writer.calls.contains(&"put"));
    assert!(!writer.calls.contains(&"release"));
    let mut writer = Writer::new();
    let error = cw::upload(
        &scope(),
        &mut writer,
        &state(b"new"),
        &"0".repeat(64),
        || Ok(()),
        &AtomicBool::new(false),
        &cw::Limits::default(),
    )
    .err()
    .expect("conflict");
    assert_eq!(error.code, "conflict");
    assert!(!writer.calls.contains(&"put"));
    assert!(writer.calls.contains(&"release"));
}
#[test]
fn failed_readback_preserves_uncertainty_and_cancel_cannot_generate_receipt() {
    let mut writer = Writer::new();
    writer.lie_readback = true;
    let expected = ci::digest(&writer.state).expect("digest");
    let error = cw::upload(
        &scope(),
        &mut writer,
        &state(b"new"),
        &expected,
        || Ok(()),
        &AtomicBool::new(false),
        &cw::Limits::default(),
    )
    .err()
    .expect("readback");
    assert_eq!(error.code, "readback");
    assert!(error.recovery_required);
    assert_eq!(error.committed_containers, 0);
    let cancel = AtomicBool::new(true);
    let mut writer = Writer::new();
    assert!(
        cw::upload(
            &scope(),
            &mut writer,
            &state(b"new"),
            &expected,
            || Ok(()),
            &cancel,
            &cw::Limits::default()
        )
        .is_err()
    );
    assert!(writer.calls.is_empty());
    cancel.store(false, Ordering::Release);
}

#[test]
fn cache_reuses_only_exact_revisions_and_forces_commit_payload_readback() {
    use flightdeck::cloud_cache;
    let root = runtime();
    let cancel = AtomicBool::new(false);
    let mut reader = Reader::standard();
    let first = cloud_cache::download(
        root.path(),
        &mut reader,
        &cs::Limits::default(),
        &cancel,
        &BTreeSet::new(),
    )
    .expect("cold");
    let mut cached = Reader {
        scope: scope(),
        requests: vec![],
        replies: vec![
            json_response(inventory("one")),
            json_response(inventory("one")),
        ]
        .into(),
    };
    let second = cloud_cache::download(
        root.path(),
        &mut cached,
        &cs::Limits::default(),
        &cancel,
        &BTreeSet::new(),
    )
    .expect("warm");
    assert_eq!(cached.requests.len(), 2);
    assert_eq!(
        ci::digest(&ci::read_snapshot(root.path(), &scope(), &first).expect("first"))
            .expect("digest"),
        ci::digest(&ci::read_snapshot(root.path(), &scope(), &second).expect("second"))
            .expect("digest")
    );
    let mut forced = Reader::standard();
    forced.replies[2].body = b"two".to_vec();
    let third = cloud_cache::download(
        root.path(),
        &mut forced,
        &cs::Limits::default(),
        &cancel,
        &["profile".into()].into(),
    )
    .expect("forced");
    assert_eq!(
        ci::read_snapshot(root.path(), &scope(), &third)
            .expect("read")
            .containers["profile"]
            .blobs["data"],
        b"two"
    );
    assert_eq!(forced.requests.len(), 5);
    files::atomic(&third.path.join("blobs/00000000.bin"), b"bad").expect("corrupt cache");
    let mut fallback = Reader::standard();
    cloud_cache::download(
        root.path(),
        &mut fallback,
        &cs::Limits::default(),
        &cancel,
        &BTreeSet::new(),
    )
    .expect("cold fallback");
    assert_eq!(fallback.requests.len(), 5);
}
#[test]
fn session_journal_needs_exact_revision_and_real_readback_receipt() {
    use flightdeck::cloud_session as session;
    let root = runtime();
    let snap = snapshot(root.path());
    let scope = scope();
    let cancel = AtomicBool::new(false);
    let lease = files::Lease::acquire(&root.path().join("private/play.lock"), true).expect("lease");
    let plan = ci::prepare(root.path(), &scope, &snap, None).expect("plan");
    ci::apply(
        root.path(),
        &scope,
        &plan,
        &ci::Choice::Cloud,
        &lease.0,
        &cancel,
    )
    .expect("import");
    let export = ci::export_local(root.path(), &scope, &lease.0, &cancel).expect("export");
    let record = session::begin(
        root.path(),
        &scope,
        &snap,
        &export,
        session::Phase::Prepared,
    )
    .expect("begin");
    assert!(session::load(root.path(), &scope).expect("load").is_some());
    assert!(
        session::begin(
            root.path(),
            &scope,
            &snap,
            &export,
            session::Phase::Prepared
        )
        .is_err()
    );
    let uploading = session::update(
        root.path(),
        &scope,
        &record,
        session::Phase::Uploading,
        &lease.0,
        Some(&export),
    )
    .expect("update");
    assert!(
        session::update(
            root.path(),
            &scope,
            &record,
            session::Phase::Pending,
            &lease.0,
            None
        )
        .is_err()
    );
    let mut writer = Writer::new();
    let expected = ci::digest(&writer.state).expect("digest");
    let receipt = cw::upload(
        &scope,
        &mut writer,
        &export.state().expect("state"),
        &expected,
        || export.assert_unchanged(),
        &cancel,
        &cw::Limits::default(),
    )
    .expect("upload");
    ci::record_common(&export, &receipt).expect("common receipt");
    let playing = session::checkpoint(root.path(), &scope, &uploading, &export, &receipt)
        .expect("checkpoint");
    assert!(session::complete(root.path(), &scope, &uploading, &export, &receipt).is_err());
    session::complete(root.path(), &scope, &playing, &export, &receipt).expect("complete");
    assert!(
        session::load(root.path(), &scope)
            .expect("absent")
            .is_none()
    );
}
#[test]
fn process_guard_retains_same_boot_interruption_and_validates_old_markers() {
    use flightdeck::cloud_process_guard as guard;
    let root = runtime();
    files::private_dir(&root.path().join("local/msfs-prefix")).expect("prefix");
    let lease = files::Lease::acquire(&root.path().join("private/play.lock"), true).expect("lease");
    guard::check(root.path(), &lease.0).expect("clean");
    guard::mark(root.path(), &lease.0).expect("mark");
    assert_eq!(
        guard::check(root.path(), &lease.0)
            .expect_err("interrupted")
            .code,
        "unsafe_session"
    );
    guard::clear(root.path(), &lease.0).expect("supervisor complete");
    guard::check(root.path(), &lease.0).expect("clear");
    let path = root.path().join("private/cloud-interrupted-process.json");
    files::atomic(
        &path,
        br#"{"schema":1,"boot_id":"12345678-1111-2222-3333-123456789abc"}"#,
    )
    .expect("old boot");
    guard::check(root.path(), &lease.0).expect("old process gone");
    assert!(!path.exists());
    files::atomic(
        &path,
        br#"{"schema":1,"schema":1,"boot_id":"12345678-1111-2222-3333-123456789abc"}"#,
    )
    .expect("invalid");
    assert!(guard::check(root.path(), &lease.0).is_err());
    assert!(path.exists());
}
#[test]
fn cloud_prefix_reuses_only_clean_profile_and_retains_dirty_same_boot_copy() {
    use flightdeck::cloud_prefix::Prefix;
    let root = runtime();
    let binding = "a".repeat(64);
    let libraries: BTreeMap<String, Vec<u8>> = [
        ("xgameruntime.dll".into(), b"one".to_vec()),
        ("xgameruntime_original.dll".into(), b"two".to_vec()),
        ("xodus_store_test.dll".into(), b"three".to_vec()),
    ]
    .into();
    let mut prefix = Prefix::new(root.path(), &binding, libraries.clone()).expect("cold");
    assert!(!prefix.reusable);
    for name in ["drive_c/windows/system32", "dosdevices"] {
        fs::create_dir_all(prefix.path().join(name)).expect("wine dirs");
    }
    for name in ["system.reg", "user.reg", "userdef.reg"] {
        files::atomic(&prefix.path().join(name), b"WINE REGISTRY Version 2\n").expect("registry");
    }
    symlink("../drive_c", prefix.path().join("dosdevices/c:")).expect("c drive");
    symlink("/", prefix.path().join("dosdevices/z:")).expect("z drive");
    prefix.install_libraries().expect("dlls");
    prefix.complete();
    let cached_path = prefix.work.clone();
    drop(prefix);
    let warm = Prefix::new(root.path(), &binding, libraries.clone()).expect("reuse");
    assert!(warm.reusable);
    drop(warm);
    let cold = Prefix::new(root.path(), &binding, libraries).expect("dirty fallback");
    assert!(!cold.reusable);
    assert_ne!(cold.work, cached_path);
    assert!(cached_path.join("prefix/system.reg").is_file());
}
#[test]
fn native_title_binding_rejects_duplicate_title_ids_and_decodes_utf16() {
    let root = runtime();
    let path = root.path().join("games/MSFS2024");
    fs::create_dir_all(&path).expect("game");
    let xml = r#"<Game><Identity Name="Microsoft.Test" Publisher="CN=Example &amp; Co"/><StoreId>9P38D19T7LRV</StoreId><TitleId>7B</TitleId><SCID>AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE</SCID></Game>"#;
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    files::atomic(&path.join("MicrosoftGame.Config"), &bytes).expect("config");
    let value = flightdeck::cloud_runtime::config(root.path()).expect("binding");
    assert_eq!(value["title_id"], 123);
    assert_eq!(value["scid"], "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    assert!(
        value["pfn"]
            .as_str()
            .expect("pfn")
            .starts_with("Microsoft.Test_")
    );
    files::atomic(
        &path.join("MicrosoftGame.Config"),
        xml.replace("</Game>", "<TitleId>7B</TitleId></Game>")
            .as_bytes(),
    )
    .expect("duplicate");
    assert!(flightdeck::cloud_runtime::config(root.path()).is_err());
}

fn put_local(root: &Path, scope: &Scope, state: &State) {
    let folder = root
        .join("private/local-saves")
        .join(scope.namespace().expect("namespace"));
    files::private_dir(&folder).expect("local folder");
    files::atomic(
        &folder.join("state.bin"),
        &flightdeck::save_state::encode(state).expect("encode"),
    )
    .expect("game save");
}
#[test]
fn complete_automatic_session_imports_then_uploads_new_game_progress() {
    use flightdeck::{cloud_flow as flow, cloud_session};
    let root = runtime();
    let scope = scope();
    let cancel = AtomicBool::new(false);
    let lease = files::Lease::acquire(&root.path().join("private/play.lock"), true).expect("lease");
    let mut writer = Writer::new();
    let request = flow::Request {
        runtime: root.path(),
        lease: &lease.0,
        cancel: &cancel,
        review: None,
        expected_binding: None,
    };
    flow::before(&request, &mut writer).unwrap_or_else(|e| panic!("before: {}", e.error));
    assert!(
        cloud_session::load(root.path(), &scope)
            .expect("playing journal")
            .is_some()
    );
    assert!(!writer.calls.contains(&"put"));
    {
        let export = ci::export_local(root.path(), &scope, &lease.0, &cancel).expect("local");
        assert_eq!(
            export.state().expect("state").containers["profile"].blobs["data"],
            b"old"
        );
    }
    flow::offline(root.path(), &lease.0, true).expect("account guard");
    put_local(root.path(), &scope, &state(b"new flight progress"));
    let binding = scope.binding();
    let request = flow::Request {
        expected_binding: Some(&binding),
        ..request
    };
    flow::after(&request, &mut writer).unwrap_or_else(|e| panic!("after: {}", e.error));
    assert_eq!(
        writer.state.containers["profile"].blobs["data"],
        b"new flight progress"
    );
    assert!(
        cloud_session::load(root.path(), &scope)
            .expect("completed journal")
            .is_none()
    );
    assert!(
        ci::load_baseline(root.path(), &scope)
            .expect("verified baseline")
            .is_some()
    );
    flow::before(&request, &mut writer).unwrap_or_else(|e| panic!("next launch: {}", e.error));
}
#[test]
fn automatic_conflict_requires_review_and_refuses_changed_review_inputs() {
    use flightdeck::cloud_flow as flow;
    let root = runtime();
    let scope = scope();
    let cancel = AtomicBool::new(false);
    let lease = files::Lease::acquire(&root.path().join("private/play.lock"), true).expect("lease");
    let mut writer = Writer::new();
    put_local(root.path(), &scope, &state(b"local progress"));
    flow::offline(root.path(), &lease.0, true).expect("offline marker");
    let request = flow::Request {
        runtime: root.path(),
        lease: &lease.0,
        cancel: &cancel,
        review: None,
        expected_binding: None,
    };
    let attention = flow::before(&request, &mut writer).expect_err("conflict");
    assert_eq!(attention.error.code, "conflict");
    let review = attention.plan.expect("private review");
    assert!(!writer.calls.contains(&"put"));
    put_local(root.path(), &scope, &state(b"newer local progress"));
    let choice = ci::Choice::Local;
    let selected = flow::Request {
        review: Some((&review, &choice)),
        ..request
    };
    let attention = flow::before(&selected, &mut writer).expect_err("changed review");
    let new_review = attention.plan.expect("fresh review");
    assert!(!writer.calls.contains(&"put"));
    let selected = flow::Request {
        review: Some((&new_review, &choice)),
        ..selected
    };
    flow::before(&selected, &mut writer).unwrap_or_else(|e| panic!("selected: {}", e.error));
    assert_eq!(
        writer.state.containers["profile"].blobs["data"],
        b"newer local progress"
    );
    let foreign = "f".repeat(64);
    let changed_account = flow::Request {
        expected_binding: Some(&foreign),
        review: None,
        ..selected
    };
    assert_eq!(
        flow::after(&changed_account, &mut writer)
            .expect_err("account changed")
            .error
            .code,
        "invalid_scope"
    );
}

// SPDX-License-Identifier: MIT
//! Bounded ConnectedStorage reads and immutable, rechecked snapshots.
use crate::{
    cloud::{self, Failure, Result, require},
    cloud_fs as fs, files,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{OnceLock, atomic::AtomicBool},
    time::{Duration, Instant},
};

#[derive(Clone, PartialEq, Eq)]
pub struct Scope {
    xuid: String,
    scid: String,
    pfn: String,
    title_id: u32,
}
impl Scope {
    pub fn new(xuid: &str, scid: &str, pfn: &str, title_id: u32) -> Result<Self> {
        require(
            !xuid.is_empty()
                && xuid.len() <= 20
                && xuid.bytes().all(|b| b.is_ascii_digit())
                && xuid.parse::<u64>().is_ok_and(|n| n > 0)
                && cloud::guid(scid)
                && !pfn.is_empty()
                && pfn.len() <= 255
                && pfn
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                && title_id > 0,
            "invalid_scope",
        )?;
        Ok(Self {
            xuid: xuid.into(),
            scid: scid.to_ascii_lowercase(),
            pfn: pfn.into(),
            title_id,
        })
    }
    pub fn from_reply(v: &Value) -> Result<Self> {
        require(
            v.as_object().is_some_and(|map| {
                map.len() == 4
                    && ["xuid", "scid", "package_family_name", "title_id"]
                        .iter()
                        .all(|key| map.contains_key(*key))
            }),
            "invalid_scope",
        )?;
        Self::new(
            v["xuid"].as_str().unwrap_or(""),
            v["scid"].as_str().unwrap_or(""),
            v["package_family_name"].as_str().unwrap_or(""),
            v["title_id"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0),
        )
    }
    pub fn pfn(&self) -> &str {
        &self.pfn
    }
    pub fn scid(&self) -> &str {
        &self.scid
    }
    pub fn title_id(&self) -> u32 {
        self.title_id
    }
    pub fn binding(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"Flightdeck ConnectedStorage read snapshot v1\0");
        for value in [
            &self.xuid,
            &self.scid,
            &self.pfn,
            &self.title_id.to_string(),
        ] {
            hash.update((value.len() as u32).to_be_bytes());
            hash.update(value.as_bytes());
        }
        hex::encode(hash.finalize())
    }
    pub fn namespace(&self) -> Result<String> {
        crate::save_state::namespace_key(
            self.title_id,
            &self.scid,
            self.xuid
                .parse()
                .map_err(|_| Failure::new("invalid_scope"))?,
        )
        .map_err(|_| Failure::new("invalid_scope"))
    }
}
/// No caller-provided URL, headers, credentials or arbitrary HTTP method crosses
/// the helper pipe. These are the only operations of the read protocol.
pub enum ReadOperation<'a> {
    Index {
        skip: usize,
        continuation: Option<&'a str>,
    },
    Container(&'a str),
    Atom(&'a str),
}
pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}
pub trait Transport {
    fn scope(&self) -> &Scope;
    fn read(
        &mut self,
        op: ReadOperation<'_>,
        timeout: Duration,
        maximum: usize,
    ) -> Result<Response>;
}
#[derive(Clone)]
pub struct Limits {
    pub pages: usize,
    pub containers: usize,
    pub blobs: usize,
    pub blob_bytes: usize,
    pub total_bytes: usize,
    pub json_bytes: usize,
    pub timeout: Duration,
    pub deadline: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            pages: 64,
            containers: 4096,
            blobs: 65536,
            blob_bytes: 64 * 1024 * 1024,
            total_bytes: 256 * 1024 * 1024,
            json_bytes: 4 * 1024 * 1024,
            timeout: Duration::from_secs(20),
            deadline: Duration::from_secs(180),
        }
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Container {
    pub name: String,
    pub display_name: String,
    pub etag: String,
    pub client_file_time: u64,
    pub size: usize,
}
#[derive(Clone, PartialEq, Eq)]
pub struct Atom {
    pub atom: String,
    pub name: String,
    pub size: Option<usize>,
}
#[derive(Clone)]
pub struct Snapshot {
    pub path: PathBuf,
    pub scope_binding: String,
    pub container_count: usize,
    pub blob_count: usize,
    pub total_bytes: usize,
    pub consistency: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SavedBlob {
    pub name: String,
    pub atom: String,
    pub file: String,
    pub size: usize,
    pub sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SavedContainer {
    pub name: String,
    pub display_name: String,
    pub etag: String,
    pub client_file_time: u64,
    pub size: usize,
    pub blobs: Vec<SavedBlob>,
}
impl SavedContainer {
    pub(crate) fn metadata(&self) -> Container {
        Container {
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            etag: self.etag.clone(),
            client_file_time: self.client_file_time,
            size: self.size,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub schema: u32,
    pub scope_binding: String,
    pub consistency: String,
    pub containers: Vec<SavedContainer>,
}
// Cache inputs can only be made from a fully verified snapshot, never HTTP data.
pub(crate) struct CachedContainer {
    pub metadata: Container,
    pub blobs: Vec<(Atom, Vec<u8>)>,
}
pub(crate) struct Cache {
    pub binding: String,
    pub containers: BTreeMap<String, CachedContainer>,
}

pub fn headers(values: BTreeMap<String, String>) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (name, value) in values {
        require(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+.^_`|~-".contains(&c))
                && !value.chars().any(|c| c < ' ' || c == '\u{7f}'),
            "invalid_response",
        )?;
        require(
            out.insert(name.to_ascii_lowercase(), value).is_none(),
            "invalid_response",
        )?;
    }
    Ok(out)
}
pub fn file_time(value: &Value) -> Result<u64> {
    if let Some(n) = value.as_u64() {
        return Ok(n);
    }
    let s = value
        .as_str()
        .ok_or_else(|| Failure::new("invalid_response"))?;
    static STAMP: OnceLock<regex::Regex> = OnceLock::new();
    let re = STAMP.get_or_init(|| regex::Regex::new(r"^([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2})(?:\.([0-9]{1,7}))?(Z|[+-][0-9]{2}:[0-9]{2})$").expect("constant regex"));
    let cap = re
        .captures(s)
        .ok_or_else(|| Failure::new("invalid_response"))?;
    let date = chrono::NaiveDateTime::parse_from_str(&cap[1], "%Y-%m-%dT%H:%M:%S")
        .map_err(|_| Failure::new("invalid_response"))?;
    // chrono accepts leap seconds; the service's .NET representation does not.
    require(&cap[1][17..19] != "60", "invalid_response")?;
    let zone = &cap[3];
    let offset = if zone == "Z" {
        0i64
    } else {
        let hours: i64 = zone[1..3]
            .parse()
            .map_err(|_| Failure::new("invalid_response"))?;
        let minutes: i64 = zone[4..6]
            .parse()
            .map_err(|_| Failure::new("invalid_response"))?;
        require(
            hours <= 14 && minutes < 60 && (hours != 14 || minutes == 0),
            "invalid_response",
        )?;
        (hours * 60 + minutes) * 60 * if zone.starts_with('+') { 1 } else { -1 }
    };
    let mut fraction = cap.get(2).map_or("0", |s| s.as_str()).to_owned();
    while fraction.len() < 7 {
        fraction.push('0');
    }
    let ticks = (date.and_utc().timestamp() as i128 - offset as i128 + 11_644_473_600i128)
        * 10_000_000
        + fraction
            .parse::<i128>()
            .map_err(|_| Failure::new("invalid_response"))?;
    u64::try_from(ticks).map_err(|_| Failure::new("invalid_response"))
}

struct Reader<'a, T: Transport + ?Sized> {
    transport: &'a mut T,
    cancel: &'a AtomicBool,
    limits: &'a Limits,
    deadline: Instant,
}
impl<T: Transport + ?Sized> Reader<'_, T> {
    fn check(&self) -> Result<Duration> {
        cloud::remaining(self.cancel, self.deadline)
    }
    fn get(&mut self, op: ReadOperation<'_>, maximum: usize) -> Result<Vec<u8>> {
        let timeout = self.check()?.min(self.limits.timeout);
        let answer = self.transport.read(op, timeout, maximum)?;
        self.check()?;
        match answer.status {
            200 => (),
            401 | 403 => return Err(Failure::new("authentication").status(answer.status)),
            404 => return Err(Failure::new("not_found").status(404)),
            _ => return Err(Failure::new("http_error").status(answer.status)),
        }
        let h = headers(answer.headers)?;
        require(
            h.get("content-encoding")
                .is_none_or(|v| v.eq_ignore_ascii_case("identity")),
            "invalid_response",
        )?;
        if let Some(length) = h.get("content-length") {
            require(
                !length.is_empty()
                    && length.len() <= 20
                    && length.bytes().all(|b| b.is_ascii_digit()),
                "invalid_response",
            )?;
            let n: u64 = length.parse().map_err(|_| Failure::new("bounds"))?;
            require(n <= maximum as u64, "bounds")?;
            require(n == answer.body.len() as u64, "invalid_response")?;
        }
        require(answer.body.len() <= maximum, "bounds")?;
        Ok(answer.body)
    }
    fn inventory(&mut self) -> Result<Vec<Container>> {
        let mut result = Vec::new();
        let mut names = BTreeSet::new();
        let mut tokens = BTreeSet::new();
        let mut expected = None;
        let mut size = 0;
        let mut continuation: Option<String> = None;
        for _ in 0..self.limits.pages {
            let data = self.get(
                ReadOperation::Index {
                    skip: result.len(),
                    continuation: continuation.as_deref(),
                },
                self.limits.json_bytes,
            )?;
            let value = cloud::json(&data)?;
            let rows = value["blobs"]
                .as_array()
                .ok_or_else(|| Failure::new("invalid_response"))?;
            let paging = value["pagingInfo"]
                .as_object()
                .ok_or_else(|| Failure::new("invalid_response"))?;
            let token = paging
                .get("continuationToken")
                .ok_or_else(|| Failure::new("invalid_response"))?;
            continuation = if token.is_null() {
                None
            } else {
                Some(cloud::text(token, 8192, false)?.into())
            };
            let total = cloud::number(
                &value["pagingInfo"]["totalItems"],
                self.limits.containers as u64,
            )? as usize;
            if let Some(n) = expected {
                require(n == total, "changed")?;
            } else {
                expected = Some(total);
            }
            require(result.len() + rows.len() <= total, "invalid_response")?;
            for row in rows {
                let name = cloud::text(&row["fileName"], 1024, false)?;
                require(
                    name != "." && name != ".." && names.insert(name.to_owned()),
                    "invalid_response",
                )?;
                let item = Container {
                    name: name.into(),
                    display_name: cloud::text(&row["displayName"], 256, true)?.into(),
                    etag: cloud::text(&row["etag"], 1024, false)?.into(),
                    client_file_time: file_time(&row["clientFileTime"])?,
                    size: cloud::number(&row["size"], self.limits.total_bytes as u64)? as usize,
                };
                size += item.size;
                require(size <= self.limits.total_bytes, "bounds")?;
                result.push(item);
            }
            let Some(token) = &continuation else {
                require(result.len() == total, "invalid_response")?;
                result.sort_by(|a, b| a.name.cmp(&b.name));
                return Ok(result);
            };
            require(
                !rows.is_empty() && result.len() < total && tokens.insert(token.clone()),
                "invalid_response",
            )?;
        }
        Err(Failure::new("bounds"))
    }
    fn atoms(&mut self, container: &Container) -> Result<Vec<Atom>> {
        let data = self.get(
            ReadOperation::Container(&container.name),
            self.limits.json_bytes,
        )?;
        let value = cloud::json(&data)?;
        let mut atoms = Vec::new();
        let dictionary = value["atoms"].is_object();
        if let Some(map) = value["atoms"].as_object() {
            require(map.len() <= self.limits.blobs, "bounds")?;
            for (name, wire) in map {
                let id = wire
                    .as_str()
                    .and_then(|s| s.strip_suffix(",binary"))
                    .ok_or_else(|| Failure::new("invalid_response"))?;
                atoms.push(Atom {
                    name: name.clone(),
                    atom: id.into(),
                    size: None,
                });
            }
        } else {
            let rows = value["atoms"]
                .as_array()
                .ok_or_else(|| Failure::new("invalid_response"))?;
            require(rows.len() <= self.limits.blobs, "bounds")?;
            for row in rows {
                atoms.push(Atom { name: cloud::text(&row["name"], 256, false)?.into(), atom: row["atom"].as_str().unwrap_or("").into(), size: Some(cloud::number(&row["size"], self.limits.blob_bytes as u64)? as usize) });
            }
        }
        let mut names = BTreeSet::new();
        let mut ids = BTreeSet::new();
        let mut size = 0;
        for atom in &atoms {
            cloud::text(&Value::String(atom.name.clone()), 256, false)?;
            require(
                cloud::guid(&atom.atom)
                    && names.insert(&atom.name)
                    && ids.insert(atom.atom.to_ascii_lowercase()),
                "invalid_response",
            )?;
            size += atom.size.unwrap_or(0);
        }
        require(dictionary || size == container.size, "changed")?;
        atoms.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(atoms)
    }
}
pub fn inventory(
    transport: &mut impl Transport,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<Vec<Container>> {
    Reader {
        transport,
        cancel,
        limits,
        deadline: Instant::now() + limits.deadline,
    }
    .inventory()
}
pub fn download(
    transport: &mut impl Transport,
    parent: &Path,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<Snapshot> {
    download_cached(transport, parent, limits, cancel, None)
}
pub(crate) fn download_cached(
    transport: &mut impl Transport,
    parent: &Path,
    limits: &Limits,
    cancel: &AtomicBool,
    cache: Option<&Cache>,
) -> Result<Snapshot> {
    let binding = transport.scope().binding();
    let cache = cache.filter(|c| c.binding == binding);
    let mut read = Reader {
        transport,
        cancel,
        limits,
        deadline: Instant::now() + limits.deadline,
    };
    let root = fs::open(parent, true)?;
    let inventory = read.inventory()?;
    let staging = format!(".snapshot-{}", uuid::Uuid::new_v4().simple());
    let destination = format!("snapshot-{}", uuid::Uuid::new_v4().simple());
    rustix::fs::mkdirat(&root, &staging, rustix::fs::Mode::from_raw_mode(0o700))?;
    let stage = fs::child(&root, &staging, false)?;
    let blobs = fs::child(&stage, "blobs", true)?;
    let mut files_written = Vec::new();
    let mut published = false;
    let result = (|| {
        let mut manifest = Manifest {
            schema: 1,
            scope_binding: binding.clone(),
            consistency: "rechecked-unlocked".into(),
            containers: Vec::new(),
        };
        let mut before = BTreeMap::new();
        let mut total = 0;
        for container in &inventory {
            read.check()?;
            let previous = cache
                .and_then(|c| c.containers.get(&container.name))
                .filter(|c| c.metadata == *container);
            let atoms = if let Some(c) = previous {
                c.blobs.iter().map(|(a, _)| a.clone()).collect()
            } else {
                let atoms = read.atoms(container)?;
                before.insert(container.name.clone(), atoms.clone());
                atoms
            };
            require(files_written.len() + atoms.len() <= limits.blobs, "bounds")?;
            let mut saved = SavedContainer {
                name: container.name.clone(),
                display_name: container.display_name.clone(),
                etag: container.etag.clone(),
                client_file_time: container.client_file_time,
                size: container.size,
                blobs: Vec::new(),
            };
            let mut bytes = 0;
            for (position, atom) in atoms.iter().enumerate() {
                read.check()?;
                let maximum = atom
                    .size
                    .unwrap_or(limits.blob_bytes.min(container.size.saturating_sub(bytes)));
                let fetched;
                let data = if let Some(c) = previous {
                    &c.blobs[position].1
                } else {
                    fetched = read.get(ReadOperation::Atom(&atom.atom), maximum)?;
                    &fetched
                };
                require(
                    atom.size.is_none_or(|n| n == data.len()) && data.len() <= maximum,
                    "invalid_response",
                )?;
                let filename = format!("{:08x}.bin", files_written.len());
                files_written.push(filename.clone());
                fs::write(&blobs, &filename, data, 0o600)?;
                total += data.len();
                bytes += data.len();
                require(total <= limits.total_bytes, "bounds")?;
                saved.blobs.push(SavedBlob {
                    name: atom.name.clone(),
                    atom: atom.atom.clone(),
                    file: format!("blobs/{filename}"),
                    size: data.len(),
                    sha256: files::sha256(data),
                });
            }
            require(bytes == container.size, "changed")?;
            manifest.containers.push(saved);
        }
        for container in &inventory {
            if let Some(atoms) = before.get(&container.name) {
                require(&read.atoms(container)? == atoms, "changed")?;
            }
        }
        require(read.inventory()? == inventory, "changed")?;
        read.check()?;
        let mut data = serde_json::to_vec(&manifest)?;
        data.push(b'\n');
        fs::write(&stage, "manifest.json", &data, 0o600)?;
        blobs.sync_all()?;
        stage.sync_all()?;
        read.check()?;
        fs::same(parent, &root, true)?;
        fs::linked(&root, &staging, &stage)?;
        // Track publication before parent fsync; an uncertain flush must not
        // erase an already published immutable snapshot.
        rustix::fs::renameat_with(
            &root,
            &staging,
            &root,
            &destination,
            rustix::fs::RenameFlags::NOREPLACE,
        )?;
        published = true;
        root.sync_all()
            .map_err(|_| Failure::new("durability_unknown"))?;
        Ok(Snapshot {
            path: parent.join(&destination),
            scope_binding: binding,
            container_count: inventory.len(),
            blob_count: files_written.len(),
            total_bytes: total,
            consistency: "rechecked-unlocked".into(),
        })
    })();
    if !published {
        for name in files_written {
            let _ = fs::unlink(&blobs, &name, false);
        }
        let _ = fs::unlink(&stage, "manifest.json", false);
        let _ = fs::unlink(&stage, "blobs", true);
        let _ = fs::unlink(&root, &staging, true);
    }
    result
}

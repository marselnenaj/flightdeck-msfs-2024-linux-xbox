// SPDX-License-Identifier: MIT
//! Bounded private log scans; consumers see complete lines, never joined gaps.
use crate::{Result, files};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::Metadata,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
pub fn regex(pattern: &'static str) -> regex::Regex {
    static CACHE: OnceLock<Mutex<BTreeMap<&'static str, regex::Regex>>> = OnceLock::new();
    CACHE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(pattern)
        .or_insert_with(|| regex::Regex::new(pattern).expect("constant diagnostic expression"))
        .clone()
}
struct Lines {
    pending: Vec<u8>,
    discarding: bool,
    oversized: usize,
}
impl Lines {
    fn feed(&mut self, bytes: &[u8], final_chunk: bool, consume: &mut impl FnMut(&str)) {
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(bytes);
        if data.is_empty() {
            return;
        }
        let mut parts: Vec<_> = data.split(|b| *b == b'\n').collect();
        if !final_chunk {
            self.pending = parts.pop().unwrap_or_default().to_vec();
        }
        let mut lines = Vec::new();
        for part in parts {
            if self.discarding {
                self.discarding = false;
            } else if part.len() > 16 * 1024 {
                self.oversized += 1;
            } else {
                lines.extend_from_slice(part);
                lines.push(b'\n');
            }
        }
        if self.pending.len() > 16 * 1024 {
            if !self.discarding {
                self.oversized += 1;
            }
            self.discarding = true;
            self.pending.clear();
        }
        if !lines.is_empty() {
            consume(&String::from_utf8_lossy(&lines));
        }
    }
}
pub fn scan(path: &Path, consume: impl FnMut(&str)) -> Result<(Value, Metadata)> {
    scan_bounded(
        path,
        64 * 1024 * 1024,
        512 * 1024,
        Duration::from_secs(3),
        consume,
    )
}
pub fn scan_bounded(
    path: &Path,
    maximum: u64,
    tail: u64,
    seconds: Duration,
    mut consume: impl FnMut(&str),
) -> Result<(Value, Metadata)> {
    use std::os::unix::fs::MetadataExt;
    let mut file = files::open_at(rustix::fs::CWD, path, false, false)?;
    let info = file.metadata()?;
    let deadline = Instant::now() + seconds;
    let mut read = 0u64;
    let mut lines = Lines {
        pending: Vec::new(),
        discarding: false,
        oversized: 0,
    };
    let end = info.len().min(maximum);
    let mut position = 0u64;
    let mut block = vec![0u8; 256 * 1024];
    while position < end && Instant::now() < deadline {
        let length = (end - position).min(block.len() as u64) as usize;
        let n = file.read(&mut block[..length])?;
        if n == 0 {
            break;
        }
        position += n as u64;
        read += n as u64;
        lines.feed(&block[..n], false, &mut consume);
    }
    let offset = position.max(info.len().saturating_sub(tail));
    if position < info.len() {
        if offset > position {
            lines.pending.clear();
            lines.discarding = true;
        }
        file.seek(SeekFrom::Start(offset))?;
        position = offset;
        while position < info.len() {
            let length = (info.len() - position).min(block.len() as u64) as usize;
            let n = file.read(&mut block[..length])?;
            if n == 0 {
                break;
            }
            position += n as u64;
            read += n as u64;
            lines.feed(&block[..n], false, &mut consume);
        }
    }
    lines.feed(&[], true, &mut consume);
    let after = file.metadata()?;
    let changed = (
        info.len(),
        info.mtime(),
        info.mtime_nsec(),
        info.ctime(),
        info.ctime_nsec(),
    ) != (
        after.len(),
        after.mtime(),
        after.mtime_nsec(),
        after.ctime(),
        after.ctime_nsec(),
    );
    let coverage = json!({"scope":"bounded_scan","bytes_total":info.len(),"bytes_read":read,"omitted_bytes":info.len().saturating_sub(read),"oversized_lines":lines.oversized,"changed_during_read":changed,"complete":read==info.len()&&lines.oversized==0&&!changed});
    Ok((coverage, info))
}

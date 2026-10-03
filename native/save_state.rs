// SPDX-License-Identifier: MIT
//! Binary interchange with the native GameSave provider. No filesystem or cloud writes.
use crate::{Error, Result, error::require, files::sha256};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const QUOTA: usize = 256 * 1024 * 1024;
pub const METADATA_LIMIT: usize = 32 * 1024 * 1024;
pub const CONTAINER_LIMIT: usize = 4096;
pub const BLOB_LIMIT: usize = 65536;
const INVALID: &str = "Ungültiger oder beschädigter Spielstand.";
const MAGIC: &[u8; 8] = b"XDLOCAL1";

// Intentionally no Debug/Serialize implementation: save contents are private.
#[derive(Clone, PartialEq, Eq)]
pub struct Container {
    pub display_name: String,
    pub modified: u64,
    pub blobs: BTreeMap<String, Vec<u8>>,
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct State {
    pub generation: u64,
    pub containers: BTreeMap<String, Container>,
}

pub fn name(value: &str, container: bool) -> Result<()> {
    require(
        !value.is_empty() && value.len() <= 256 && !value.contains("..") && !value.ends_with('.'),
        INVALID,
    )?;
    let mut parts = value.split('/').peekable();
    while let Some(part) = parts.next() {
        require(!part.is_empty(), INVALID)?;
        let parent = parts.peek().is_some();
        require(!parent || container, INVALID)?;
        require(
            part.bytes().all(|b| {
                b.is_ascii_alphanumeric() || b == b'_' || (!parent && (b == b'.' || b == b'-'))
            }),
            INVALID,
        )?;
    }
    Ok(())
}

pub fn namespace_key(title_id: u32, scid: &str, xuid: u64) -> Result<String> {
    require(title_id != 0 && xuid != 0, INVALID)?;
    let identity = uuid::Uuid::parse_str(scid)
        .map_err(|_| Error::Invalid(INVALID))?
        .hyphenated()
        .to_string();
    require(scid.to_ascii_lowercase() == identity, INVALID)?;
    let mut hash = Sha256::new();
    hash.update(b"xodus.localgamesave.namespace.v1\0");
    hash.update(title_id.to_be_bytes());
    hash.update(identity.as_bytes());
    hash.update([1]);
    hash.update(xuid.to_be_bytes());
    Ok(hex::encode(hash.finalize()))
}

struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(size)
            .ok_or(Error::Invalid(INVALID))?;
        let bytes = self
            .data
            .get(self.position..end)
            .ok_or(Error::Invalid(INVALID))?;
        self.position = end;
        Ok(bytes)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| Error::Invalid(INVALID))?,
        ))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| Error::Invalid(INVALID))?,
        ))
    }
    fn text(&mut self, limit: usize) -> Result<String> {
        let size = self.u32()? as usize;
        require(size <= limit, INVALID)?;
        let text = std::str::from_utf8(self.take(size)?).map_err(|_| Error::Invalid(INVALID))?;
        require(!text.contains('\0'), INVALID)?;
        Ok(text.into())
    }
}

pub fn decode(data: &[u8]) -> Result<State> {
    require(
        (56..=QUOTA + METADATA_LIMIT + 32).contains(&data.len()) && data.starts_with(MAGIC),
        INVALID,
    )?;
    let (body, checksum) = data.split_at(data.len() - 32);
    require(
        Sha256::digest(body).as_slice() == checksum,
        "Prüfsumme des Spielstands stimmt nicht.",
    )?;
    let mut reader = Reader {
        data: body,
        position: 8,
    };
    require(reader.u32()? == 1, INVALID)?;
    let mut state = State {
        generation: reader.u64()?,
        containers: BTreeMap::new(),
    };
    let count = reader.u32()? as usize;
    require(count <= CONTAINER_LIMIT, INVALID)?;
    let (mut used, mut blobs) = (0usize, 0usize);
    for _ in 0..count {
        let key = reader.text(256)?;
        name(&key, true)?;
        require(!state.containers.contains_key(&key), INVALID)?;
        let display_name = reader.text(4096)?;
        let modified = reader.u64()?;
        require(modified <= i64::MAX as u64, INVALID)?;
        let count = reader.u32()? as usize;
        require(count <= BLOB_LIMIT - blobs, INVALID)?;
        blobs += count;
        let mut entry = Container {
            display_name,
            modified,
            blobs: BTreeMap::new(),
        };
        for _ in 0..count {
            let key = reader.text(256)?;
            name(&key, false)?;
            require(!entry.blobs.contains_key(&key), INVALID)?;
            let size = reader.u32()? as usize;
            require(size <= QUOTA - used, INVALID)?;
            used += size;
            entry.blobs.insert(key, reader.take(size)?.to_vec());
        }
        state.containers.insert(key, entry);
    }
    require(reader.position == body.len(), INVALID)?;
    Ok(state)
}

fn text(output: &mut Vec<u8>, value: &str, maximum: usize) -> Result<()> {
    require(value.len() <= maximum && !value.contains('\0'), INVALID)?;
    output.extend((value.len() as u32).to_le_bytes());
    output.extend(value.as_bytes());
    Ok(())
}
pub fn encode(state: &State) -> Result<Vec<u8>> {
    require(state.containers.len() <= CONTAINER_LIMIT, INVALID)?;
    let mut output = MAGIC.to_vec();
    output.extend(1u32.to_le_bytes());
    output.extend(state.generation.to_le_bytes());
    output.extend((state.containers.len() as u32).to_le_bytes());
    let (mut used, mut blobs) = (0usize, 0usize);
    for (key, entry) in &state.containers {
        name(key, true)?;
        require(
            entry.modified <= i64::MAX as u64 && entry.blobs.len() <= BLOB_LIMIT - blobs,
            INVALID,
        )?;
        blobs += entry.blobs.len();
        text(&mut output, key, 256)?;
        text(&mut output, &entry.display_name, 4096)?;
        output.extend(entry.modified.to_le_bytes());
        output.extend((entry.blobs.len() as u32).to_le_bytes());
        for (key, bytes) in &entry.blobs {
            name(key, false)?;
            require(bytes.len() <= QUOTA - used, INVALID)?;
            used += bytes.len();
            text(&mut output, key, 256)?;
            output.extend((bytes.len() as u32).to_le_bytes());
            output.extend(bytes);
        }
        require(output.len() <= QUOTA + METADATA_LIMIT, INVALID)?;
    }
    let checksum = Sha256::digest(&output);
    output.extend(checksum);
    Ok(output)
}
pub fn content_digest(state: &State) -> Result<String> {
    let mut canonical = state.clone();
    canonical.generation = 0;
    for entry in canonical.containers.values_mut() {
        entry.modified = 0;
    }
    Ok(sha256(&encode(&canonical)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut body = hex::decode(concat!(
            "58444c4f43414c3101000000070000000000000001000000",
            "0700000070726f66696c650500000050696c6f747b0000000000000001000000",
            "04000000646174610300000000ff42"
        ))
        .expect("fixture");
        body.extend(Sha256::digest(&body).to_vec());
        body
    }
    #[test]
    fn independent_native_wire_fixture() {
        let wire = fixture();
        let state = decode(&wire).expect("native fixture");
        assert_eq!(state.generation, 7);
        assert_eq!(state.containers["profile"].blobs["data"], [0, 255, 66]);
        assert_eq!(encode(&state).expect("encode"), wire);
    }
    #[test]
    fn truncation_checksum_and_trailing_bytes_fail() {
        let wire = fixture();
        for n in 0..wire.len() {
            assert!(decode(&wire[..n]).is_err());
        }
        let mut trailing = wire.clone();
        trailing.push(0);
        assert!(decode(&trailing).is_err());
        let mut forged = wire[..wire.len() - 32].to_vec();
        forged.push(0);
        forged.extend(Sha256::digest(&forged).to_vec());
        assert!(decode(&forged).is_err());
    }
    #[test]
    fn duplicates_fail_even_with_valid_checksum() {
        let wire = fixture();
        let body = &wire[..wire.len() - 32];
        let mut duplicate = body.to_vec();
        duplicate[20..24].copy_from_slice(&2u32.to_le_bytes());
        duplicate.extend(&body[24..]);
        duplicate.extend(Sha256::digest(&duplicate).to_vec());
        assert!(decode(&duplicate).is_err());
    }
    #[test]
    fn logical_digest_ignores_time_and_generation_only() {
        let mut state = decode(&fixture()).expect("fixture");
        let first = content_digest(&state).expect("digest");
        state.generation += 1;
        state
            .containers
            .get_mut("profile")
            .expect("profile")
            .modified += 1;
        assert_eq!(content_digest(&state).expect("digest"), first);
        state
            .containers
            .get_mut("profile")
            .expect("profile")
            .blobs
            .insert("data".into(), vec![1]);
        assert_ne!(content_digest(&state).expect("digest"), first);
    }
    #[test]
    fn invalid_names_never_become_paths() {
        for bad in [
            "",
            "../escape",
            "trailing.",
            "/root",
            "a//b",
            "a.b/child",
            "a\\b",
        ] {
            assert!(name(bad, true).is_err());
        }
        assert!(name("folder/profile", true).is_ok());
        assert!(name("folder/profile", false).is_err());
    }
}

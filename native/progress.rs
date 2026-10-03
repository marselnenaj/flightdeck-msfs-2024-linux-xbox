// SPDX-License-Identifier: MIT
//! Bounded framing for Xodus progress. Never parse stdout or surface malformed input.
use serde::{Deserialize, Serialize};

pub const MAX_FRAME: usize = 1024;
pub const MAX_INTEGER: u64 = (1u64 << 53) - 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transfer {
    pub kind: Kind,
    pub received_bytes: u64,
    pub verified_bytes: u64,
    pub total_bytes: Option<u64>,
    pub completed_files: Option<u64>,
    pub total_files: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Game,
    Components,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    format: u32,
    received_bytes: u64,
    verified_bytes: u64,
    // Unlike missing fields, explicit null is a meaningful unknown total.
    #[serde(deserialize_with = "required_optional")]
    total_bytes: Option<u64>,
    #[serde(deserialize_with = "required_optional")]
    completed_files: Option<u64>,
    #[serde(deserialize_with = "required_optional")]
    total_files: Option<u64>,
}
fn required_optional<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Option::<u64>::deserialize(d)
}

impl Transfer {
    pub fn valid(&self) -> bool {
        [
            Some(self.received_bytes),
            Some(self.verified_bytes),
            self.total_bytes,
            self.completed_files,
            self.total_files,
        ]
        .into_iter()
        .flatten()
        .all(|v| v <= MAX_INTEGER)
            && self.verified_bytes <= self.received_bytes
            && self
                .total_bytes
                .is_none_or(|v| v > 0 && self.received_bytes <= v)
            && match (self.completed_files, self.total_files) {
                (None, None) => true,
                (Some(done), Some(total)) => total > 0 && done <= total,
                _ => false,
            }
    }
}
fn parse(raw: &[u8]) -> Option<Transfer> {
    let value: Frame = serde_json::from_slice(raw).ok()?;
    if value.format != 1 {
        return None;
    }
    let transfer = Transfer {
        kind: Kind::Game,
        received_bytes: value.received_bytes,
        verified_bytes: value.verified_bytes,
        total_bytes: value.total_bytes,
        completed_files: value.completed_files,
        total_files: value.total_files,
    };
    transfer.valid().then_some(transfer)
}
#[derive(Default)]
pub struct Decoder {
    buffer: Vec<u8>,
    discarding: bool,
    pending: Option<Option<Transfer>>,
}
impl Decoder {
    /// The caller bounds bytes per poll; retained memory never exceeds MAX_FRAME.
    pub fn consume(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if !self.discarding {
                if self.buffer.len() == MAX_FRAME {
                    self.buffer.clear();
                    self.discarding = true;
                    self.pending = Some(None);
                } else {
                    self.buffer.push(byte);
                }
            }
            if byte == b'\n' {
                if !self.discarding {
                    self.pending = Some(parse(&self.buffer));
                }
                self.buffer.clear();
                self.discarding = false;
            }
        }
    }
    pub fn take(&mut self) -> Option<Option<Transfer>> {
        self.pending.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const GOOD: &[u8] = b"{\"format\":1,\"received_bytes\":40,\"verified_bytes\":20,\"total_bytes\":100,\"completed_files\":1,\"total_files\":3}\n";
    #[test]
    fn fragments_overflow_and_resynchronization() {
        let mut decoder = Decoder::default();
        decoder.consume(&GOOD[..17]);
        assert!(decoder.take().is_none());
        decoder.consume(&GOOD[17..]);
        assert_eq!(decoder.take().flatten().expect("frame").received_bytes, 40);
        decoder.consume(&vec![b'x'; 100000]);
        assert!(decoder.buffer.len() <= MAX_FRAME);
        assert_eq!(decoder.take(), Some(None));
        decoder.consume(b"\n");
        decoder.consume(GOOD);
        assert!(decoder.take().flatten().is_some());
    }
    #[test]
    fn malformed_duplicates_unknown_fields_and_noninteger_counts_fail() {
        let good = std::str::from_utf8(GOOD).expect("fixture");
        for bad in [
            good.replace("\"format\":1", "\"format\":1,\"format\":1"),
            good.replace("40", "true"),
            good.replace("40", "40.0"),
            good.replace("40", "9007199254740992"),
            good.replace("20", "41"),
            good.replace("100", "0"),
            good.replace(",\"total_files\":3", ""),
            good.replace("\"format\":1", "\"secret\":\"private\",\"format\":1"),
        ] {
            let mut decoder = Decoder::default();
            decoder.consume(bad.as_bytes());
            assert_eq!(decoder.take(), Some(None));
        }
    }
}

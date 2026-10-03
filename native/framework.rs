// SPDX-License-Identifier: MIT
//! Read-only .NET evidence. Repair must run in an exclusively owned staging profile.
use crate::{Error, Result, error::require, files};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Default, PartialEq, Eq, Serialize)]
pub struct Architecture {
    pub release: u32,
    pub clr: bool,
}
#[derive(Clone, Default, PartialEq, Eq, Serialize)]
pub struct Status {
    pub x86: Architecture,
    pub x64: Architecture,
    pub ready: bool,
}

pub fn framework_path(prefix: &Path, architecture: &str, name: &str) -> Result<PathBuf> {
    let mut path = prefix.canonicalize()?;
    for part in [
        "drive_c",
        "windows",
        "Microsoft.NET",
        architecture,
        "v4.0.30319",
        name,
    ] {
        let mut matches = Vec::new();
        for (count, entry) in fs::read_dir(&path)?.enumerate() {
            require(count < 4096, ".NET-Verzeichnis enthält zu viele Dateien.")?;
            let entry = entry?;
            if entry
                .file_name()
                .to_str()
                .is_some_and(|v| v.eq_ignore_ascii_case(part))
            {
                matches.push(entry.path());
            }
        }
        require(matches.len() == 1, ".NET-Datei fehlt oder ist mehrdeutig.")?;
        path = matches.pop().ok_or(Error::Invalid(".NET-Datei fehlt."))?;
        require(
            !fs::symlink_metadata(&path)?.is_symlink(),
            ".NET-Dateipfad enthält einen symbolischen Link.",
        )?;
    }
    Ok(path)
}
pub fn releases(text: &str) -> (u32, u32) {
    let keys = [
        r"software\\microsoft\\net framework setup\\ndp\\v4\\full",
        r"software\\wow6432node\\microsoft\\net framework setup\\ndp\\v4\\full",
    ];
    let mut sections: BTreeMap<&str, Vec<Vec<String>>> = BTreeMap::new();
    let mut current = None;
    for line in text.lines() {
        if let Some(header) = line.strip_prefix('[') {
            let key = header
                .split_once(']')
                .map(|(key, _)| key.to_ascii_lowercase());
            current = keys.iter().copied().find(|k| key.as_deref() == Some(*k));
            if let Some(key) = current {
                sections.entry(key).or_default().push(Vec::new());
            }
        } else if line.to_ascii_lowercase().starts_with("\"release\"=")
            && let Some(values) = current
                .and_then(|key| sections.get_mut(key))
                .and_then(|v| v.last_mut())
        {
            values.push(line.to_ascii_lowercase());
        }
    }
    let release = |key| -> u32 {
        let Some(sections) = sections.get(key) else {
            return 0;
        };
        if sections.len() != 1 || sections[0].len() != 1 {
            return 0;
        }
        let Some(value) = sections[0][0].strip_prefix("\"release\"=dword:") else {
            return 0;
        };
        let value = value.trim_end();
        if value.is_empty() || value.len() > 8 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
            return 0;
        }
        u32::from_str_radix(value, 16).unwrap_or(0)
    };
    (release(keys[0]), release(keys[1]))
}
pub fn status(prefix: &Path) -> Status {
    let (x64, x86) = files::read(&prefix.join("system.reg"), 64 * 1024 * 1024)
        .map(|data| releases(&String::from_utf8_lossy(&data)))
        .unwrap_or((0, 0));
    let clr = |architecture| -> bool {
        use std::io::Read;
        let result = (|| -> Result<bool> {
            let path = framework_path(prefix, architecture, "clr.dll")?;
            let mut file = files::open_at(rustix::fs::CWD, &path, false, false)?;
            require(
                file.metadata()?.len() <= 128 * 1024 * 1024,
                ".NET-Datei ist zu groß.",
            )?;
            let mut magic = [0; 2];
            file.read_exact(&mut magic)?;
            Ok(magic == *b"MZ")
        })();
        result.unwrap_or(false)
    };
    let x64 = Architecture {
        release: x64,
        clr: clr("Framework64"),
    };
    let x86 = Architecture {
        release: x86,
        clr: clr("Framework"),
    };
    let ready = [&x64, &x86].iter().all(|a| a.release >= 528040 && a.clr);
    Status { x86, x64, ready }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_views_case_and_duplicates() {
        let one = "[SOFTWARE\\\\Microsoft\\\\NET Framework Setup\\\\NDP\\\\v4\\\\Full] 123\n\"Release\"=dword:00080eb1\n";
        let two = one.replace("SOFTWARE\\\\", "Software\\\\Wow6432Node\\\\");
        assert_eq!(releases(&format!("{one}{two}")), (528049, 528049));
        assert_eq!(releases(&format!("{one}{one}{two}")), (0, 528049));
        assert_eq!(
            releases(&one.replace("dword:00080eb1", "\"528049\"")),
            (0, 0)
        );
        assert_eq!(releases(&one.replace("Full]", "Full\\\\1033]")), (0, 0));
    }
}

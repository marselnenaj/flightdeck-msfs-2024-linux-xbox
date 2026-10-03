// SPDX-License-Identifier: MIT
use std::fmt;

#[derive(Debug)]
pub enum Error {
    Invalid(&'static str),
    Io(std::io::Error),
    Json(serde_json::Error),
    Cancelled,
    AuthRequired(&'static str),
    Cloud(crate::cloud::Failure),
    Framework(Box<Error>),
    Graphics(Box<Error>),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::Cloud(error) = self {
            return fmt::Display::fmt(error, f);
        }
        if let Self::Framework(error) | Self::Graphics(error) = self {
            return fmt::Display::fmt(error, f);
        }
        // Never put input data, save contents or paths from underlying errors in HTTP responses.
        f.write_str(match self {
            Self::Invalid(message) | Self::AuthRequired(message) => message,
            Self::Io(_) => {
                "Der lokale Vorgang ist fehlgeschlagen. Bitte Pfad und Schreibrechte prüfen."
            }
            Self::Json(_) => "Ungültige oder beschädigte Konfigurationsdaten.",
            Self::Cancelled => "Vorgang abgebrochen.",
            Self::Cloud(_) => unreachable!(),
            Self::Framework(_) | Self::Graphics(_) => unreachable!(),
        })
    }
}

impl std::error::Error for Error {}
impl From<crate::cloud::Failure> for Error {
    fn from(value: crate::cloud::Failure) -> Self {
        Self::Cloud(value)
    }
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<rustix::io::Errno> for Error {
    fn from(value: rustix::io::Errno) -> Self {
        Self::Io(value.into())
    }
}
impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::Invalid(message))
    }
}

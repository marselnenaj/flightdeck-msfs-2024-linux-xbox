// SPDX-License-Identifier: MIT
//! The two exec-only descriptor boundaries. Never call this on an FD owned by Rust.
#![allow(unsafe_code)]
use crate::{Result, error::require};
use std::{fs::File, os::fd::BorrowedFd};
/// Duplicate an inherited descriptor while leaving its original owner intact.
/// /proc validation is done before borrowing; these exec helpers initialize no
/// other threads until every inherited descriptor has been registered.
pub fn duplicate(number: i32) -> Result<File> {
    require(number >= 3, "Invalid inherited descriptor.")?;
    std::fs::metadata(format!("/proc/self/fd/{number}"))?;
    // SAFETY: the caller supplies a live inherited FD, which this function never
    // closes. No Rust object owns it and no thread may close it concurrently.
    let borrowed = unsafe { BorrowedFd::borrow_raw(number) };
    let flags = rustix::io::fcntl_getfd(borrowed)?;
    rustix::io::fcntl_setfd(borrowed, flags | rustix::io::FdFlags::CLOEXEC)?;
    Ok(File::from(rustix::io::fcntl_dupfd_cloexec(borrowed, 3)?))
}

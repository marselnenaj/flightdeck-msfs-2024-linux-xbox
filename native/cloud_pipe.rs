// SPDX-License-Identifier: MIT
//! Bounded private framing for the authenticated Windows helper.
use crate::{
    cloud::{self, Failure, Result, require},
    process,
};
use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    fs::{OFlags, fcntl_getfl, fcntl_setfl},
};
use serde_json::Value;
use std::{
    os::fd::AsFd,
    process::{Child, ChildStdin, ChildStdout},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
const MAX_FRAME: usize = 256 * 1024;
pub fn helper_error(answer: &Value) -> Failure {
    let status = answer["http_status"]
        .as_u64()
        .filter(|v| (100..=599).contains(v))
        .map(|v| v as u16);
    let hr = answer["hresult"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok());
    let code = if matches!(status, Some(401 | 403)) {
        "authentication"
    } else if matches!(hr, Some(0x800705B4 | 0x80072EE2 | 0x8007274C))
        || matches!(status, Some(408 | 504))
    {
        "deadline"
    } else if matches!(
        hr,
        Some(0x800703E3 | 0x80072EE7 | 0x80072EFD | 0x80072EFE | 0x80072F8F)
    ) || status.is_some_and(|v| v == 429 || v >= 500)
    {
        "transport"
    } else if answer["error"] == "authentication" {
        "authentication"
    } else if answer["error"] == "title_binding" {
        "invalid_scope"
    } else {
        "transport"
    };
    Failure {
        http_status: status,
        native_hresult: hr,
        ..Failure::new(code)
    }
}
fn ready(
    fd: &impl AsFd,
    writing: bool,
    cancel: &AtomicBool,
    deadline: Instant,
    cleanup: bool,
) -> Result<()> {
    loop {
        if !cleanup {
            cloud::check(cancel)?;
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|v| !v.is_zero())
            .ok_or_else(|| Failure::new("deadline"))?
            .min(Duration::from_millis(100));
        let mut descriptors = [PollFd::new(
            fd,
            if writing {
                PollFlags::OUT
            } else {
                PollFlags::IN
            },
        )];
        let duration = Timespec {
            tv_sec: 0,
            tv_nsec: remaining.as_nanos() as i64,
        };
        match poll(&mut descriptors, Some(&duration)) {
            Ok(0) => (),
            Ok(_) => return Ok(()),
            Err(rustix::io::Errno::INTR) => (),
            Err(_) => return Err(Failure::new("transport")),
        }
    }
}
fn write(
    fd: &impl AsFd,
    mut bytes: &[u8],
    cancel: &AtomicBool,
    deadline: Instant,
    cleanup: bool,
) -> Result<()> {
    while !bytes.is_empty() {
        ready(fd, true, cancel, deadline, cleanup)?;
        match rustix::io::write(fd, bytes) {
            Ok(0) => return Err(Failure::new("transport")),
            Ok(n) => bytes = &bytes[n..],
            Err(rustix::io::Errno::WOULDBLOCK | rustix::io::Errno::INTR) => (),
            Err(_) => return Err(Failure::new("transport")),
        }
    }
    Ok(())
}
fn read(
    fd: &impl AsFd,
    mut bytes: &mut [u8],
    cancel: &AtomicBool,
    deadline: Instant,
    cleanup: bool,
) -> Result<()> {
    while !bytes.is_empty() {
        ready(fd, false, cancel, deadline, cleanup)?;
        match rustix::io::read(fd, &mut *bytes) {
            Ok(0) => return Err(Failure::new("transport")),
            Ok(n) => bytes = &mut bytes[n..],
            Err(rustix::io::Errno::WOULDBLOCK | rustix::io::Errno::INTR) => (),
            Err(_) => return Err(Failure::new("transport")),
        }
    }
    Ok(())
}
pub struct Pipe {
    child: Child,
    input: Option<ChildStdin>,
    output: Option<ChildStdout>,
    cancel: Arc<AtomicBool>,
    failed: bool,
    closed: bool,
}
impl Pipe {
    /// The child must be a new owned process group, with dedicated piped I/O.
    pub fn new(child: Child, cancel: Arc<AtomicBool>) -> Result<Self> {
        let mut pipe = Self {
            child,
            input: None,
            output: None,
            cancel,
            failed: false,
            closed: false,
        };
        pipe.input = pipe.child.stdin.take();
        pipe.output = pipe.child.stdout.take();
        let input = pipe
            .input
            .as_ref()
            .ok_or_else(|| Failure::new("transport"))?;
        let output = pipe
            .output
            .as_ref()
            .ok_or_else(|| Failure::new("transport"))?;
        fcntl_setfl(input, fcntl_getfl(input)? | OFlags::NONBLOCK)?;
        fcntl_setfl(output, fcntl_getfl(output)? | OFlags::NONBLOCK)?;
        Ok(pipe)
    }
    pub fn fail(&mut self) {
        self.failed = true;
    }
    pub fn exchange(
        &mut self,
        request: &Value,
        input_body: &[u8],
        maximum: usize,
        timeout: Duration,
        cleanup: bool,
    ) -> Result<(Value, Vec<u8>)> {
        require(
            !self.failed && !self.closed && !timeout.is_zero(),
            "transport",
        )?;
        if !cleanup {
            cloud::check(&self.cancel)?;
        }
        let result = (|| {
            let payload = serde_json::to_vec(request)?;
            require(payload.len() <= MAX_FRAME, "bounds")?;
            let deadline = Instant::now() + timeout;
            let input = self
                .input
                .as_ref()
                .ok_or_else(|| Failure::new("transport"))?;
            let output = self
                .output
                .as_ref()
                .ok_or_else(|| Failure::new("transport"))?;
            write(
                input,
                &(payload.len() as u32).to_le_bytes(),
                &self.cancel,
                deadline,
                cleanup,
            )?;
            write(input, &payload, &self.cancel, deadline, cleanup)?;
            write(input, input_body, &self.cancel, deadline, cleanup)?;
            let mut length = [0; 4];
            read(output, &mut length, &self.cancel, deadline, cleanup)?;
            let length = u32::from_le_bytes(length) as usize;
            require(length > 0 && length <= MAX_FRAME, "bounds")?;
            let mut bytes = vec![0; length];
            read(output, &mut bytes, &self.cancel, deadline, cleanup)?;
            let answer = cloud::json(&bytes)?;
            if answer["ok"] != true {
                return Err(helper_error(&answer));
            }
            let size = cloud::number(
                answer.get("body_bytes").unwrap_or(&Value::from(0)),
                maximum as u64,
            )? as usize;
            let mut body = vec![0; size];
            read(output, &mut body, &self.cancel, deadline, cleanup)?;
            Ok((answer, body))
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        drop(self.input.take());
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.child.try_wait()?.is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        process::terminate_group(&mut self.child)?;
        drop(self.output.take());
        Ok(())
    }
}
impl Drop for Pipe {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

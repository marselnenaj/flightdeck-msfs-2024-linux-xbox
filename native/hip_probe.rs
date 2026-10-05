// SPDX-License-Identifier: MIT
//! ROCm metadata and a small round-trip check, only in a bounded child process.
#![allow(unsafe_code)]
use crate::{Error, Result, error::require};
use libloading::Library;
use serde_json::{Value, json};
use std::{
    ffi::{c_char, c_void},
    path::Path,
    ptr,
};

const FAILED: &str =
    "ROCm/HIP 7 could not initialize. Check the installed ROCm runtime and GPU access.";

unsafe fn symbol<T: Copy>(lib: &Library, name: &[u8]) -> Result<T> {
    unsafe { lib.get::<T>(name) }
        .map(|f| *f)
        .map_err(|_| Error::Invalid(FAILED))
}

fn string(bytes: &[u8]) -> Result<String> {
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .ok_or(Error::Invalid(FAILED))?;
    let value = std::str::from_utf8(&bytes[..end]).map_err(|_| Error::Invalid(FAILED))?;
    require(
        !value.is_empty() && !value.chars().any(char::is_control),
        FAILED,
    )?;
    Ok(value.to_owned())
}

pub fn probe(path: &Path) -> Result<Value> {
    require(path.is_absolute(), FAILED)?;
    // SAFETY: HIP's R0600 x86_64 ABI is versioned and fixed: 1472 bytes,
    // gcnArchName at 1160, length 256. The buffer is 8-byte aligned. See
    // ROCm/HIP rocm-7.1.1/include/hip/hip_runtime_api.h. No driver is loaded
    // in the launcher service, and the caller bounds this process's lifetime.
    unsafe {
        let lib = Library::new(path).map_err(|_| Error::Invalid(FAILED))?;
        let init: unsafe extern "C" fn(u32) -> i32 = symbol(&lib, b"hipInit\0")?;
        let version: unsafe extern "C" fn(*mut i32) -> i32 =
            symbol(&lib, b"hipRuntimeGetVersion\0")?;
        let count: unsafe extern "C" fn(*mut i32) -> i32 = symbol(&lib, b"hipGetDeviceCount\0")?;
        let props: unsafe extern "C" fn(*mut c_void, i32) -> i32 =
            symbol(&lib, b"hipGetDevicePropertiesR0600\0")?;
        let pci: unsafe extern "C" fn(*mut c_char, i32, i32) -> i32 =
            symbol(&lib, b"hipDeviceGetPCIBusId\0")?;
        let select: unsafe extern "C" fn(i32) -> i32 = symbol(&lib, b"hipSetDevice\0")?;
        let alloc: unsafe extern "C" fn(*mut *mut c_void, usize) -> i32 =
            symbol(&lib, b"hipMalloc\0")?;
        let free: unsafe extern "C" fn(*mut c_void) -> i32 = symbol(&lib, b"hipFree\0")?;
        let copy: unsafe extern "C" fn(*mut c_void, *const c_void, usize, i32) -> i32 =
            symbol(&lib, b"hipMemcpy\0")?;
        // Keep the library resident until child exit: HIP owns worker threads.
        std::mem::forget(lib);
        let mut runtime_version = 0;
        require(
            version(&mut runtime_version) == 0
                && (70_000_000..80_000_000).contains(&runtime_version),
            FAILED,
        )?;
        require(init(0) == 0, FAILED)?;
        let mut total = 0;
        require(count(&mut total) == 0 && (1..=16).contains(&total), FAILED)?;
        let mut devices = Vec::new();
        for index in 0..total {
            let mut data = [0_u64; 184];
            let mut address = [0_u8; 64];
            require(
                props(data.as_mut_ptr().cast(), index) == 0
                    && pci(address.as_mut_ptr().cast(), 64, index) == 0,
                FAILED,
            )?;
            let data: Vec<_> = data.iter().flat_map(|n| n.to_ne_bytes()).collect();
            let name = string(&data[..256])?;
            let arch = string(&data[1160..1416])?
                .split(':')
                .next()
                .unwrap_or("")
                .to_owned();
            let supported = ["gfx1030", "gfx1200", "gfx1201"].contains(&arch.as_str());
            let mut memory_roundtrip = false;
            if supported {
                require(select(index) == 0, FAILED)?;
                let mut gpu = ptr::null_mut();
                require(alloc(&mut gpu, 256) == 0 && !gpu.is_null(), FAILED)?;
                let input: [u8; 256] = std::array::from_fn(|i| i as u8);
                let mut output = [0_u8; 256];
                let result = copy(gpu, input.as_ptr().cast(), 256, 1) == 0
                    && copy(output.as_mut_ptr().cast(), gpu, 256, 2) == 0
                    && input == output;
                let released = free(gpu) == 0;
                require(result && released, FAILED)?;
                memory_roundtrip = true;
            }
            devices.push(json!({"index":index,"name":name,"arch":arch,"pci_bus_id":string(&address)?,"supported":supported,"memory_roundtrip":memory_roundtrip}));
        }
        Ok(
            json!({"status":"ready","runtime_version":runtime_version,"devices":devices,"neural_rendering_verified":false}),
        )
    }
}

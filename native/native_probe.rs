// SPDX-License-Identifier: MIT
//! Linux Vulkan/OpenXR ABI boundary, invoked only by a bounded child process.
//! The caller never loads a driver in the HTTP service. All structures contain
//! C integers/pointers only. Output arrays are fixed-size or explicitly bounded.
#![allow(unsafe_code)]
use crate::{Error, Result, error::require};
use libloading::Library;
use serde_json::{Value, json};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
};
type Handle = *mut c_void;
const FAILURE: &str = "Der native Treiber konnte nicht initialisiert werden.";
#[repr(C)]
struct VkApp {
    kind: u32,
    next: *const c_void,
    name: *const c_char,
    version: u32,
    engine: *const c_char,
    engine_version: u32,
    api: u32,
}
#[repr(C)]
struct VkCreate {
    kind: u32,
    next: *const c_void,
    flags: u32,
    app: *const VkApp,
    layer_count: u32,
    layers: *const *const c_char,
    extension_count: u32,
    extensions: *const *const c_char,
}
#[repr(C)]
struct DeviceId {
    kind: u32,
    next: *mut c_void,
    uuid: [u8; 16],
    driver_uuid: [u8; 16],
    luid: [u8; 8],
    node: u32,
    valid: u32,
}
#[repr(C)]
struct VkProperties {
    kind: u32,
    next: *mut c_void,
    data: [u64; 512],
}
fn vk_app(api: u32) -> VkApp {
    VkApp {
        kind: 0,
        next: ptr::null(),
        name: c"Flightdeck graphics check".as_ptr(),
        version: 1,
        engine: ptr::null(),
        engine_version: 0,
        api,
    }
}
fn vk_info(app: &VkApp, extensions: &[*const c_char]) -> VkCreate {
    VkCreate {
        kind: 1,
        next: ptr::null(),
        flags: 0,
        app,
        layer_count: 0,
        layers: ptr::null(),
        extension_count: extensions.len() as u32,
        extensions: extensions.as_ptr(),
    }
}
type VkCreateFn = unsafe extern "C" fn(*const VkCreate, *const c_void, *mut Handle) -> i32;
type VkDestroyFn = unsafe extern "C" fn(Handle, *const c_void);
type VkPropertiesFn = unsafe extern "C" fn(Handle, *mut VkProperties);
fn library(name: &str) -> Result<Library> {
    unsafe { Library::new(name) }.map_err(|_| Error::Invalid(FAILURE))
}
unsafe fn symbol<T: Copy>(lib: &Library, name: &[u8]) -> Result<T> {
    unsafe { lib.get::<T>(name) }
        .map(|s| *s)
        .map_err(|_| Error::Invalid(FAILURE))
}
fn numbers(data: &[u64; 512]) -> [u32; 5] {
    let bytes = data
        .iter()
        .take(3)
        .flat_map(|n| n.to_ne_bytes())
        .collect::<Vec<_>>();
    std::array::from_fn(|i| {
        u32::from_ne_bytes(
            bytes[i * 4..i * 4 + 4]
                .try_into()
                .expect("fixed Vulkan word"),
        )
    })
}
fn device_id() -> DeviceId {
    DeviceId {
        kind: 1000071004,
        next: ptr::null_mut(),
        uuid: [0; 16],
        driver_uuid: [0; 16],
        luid: [0; 8],
        node: 0,
        valid: 0,
    }
}
pub fn graphics() -> Result<Value> {
    // SAFETY: signatures and repr(C) layouts follow Vulkan 1.0/1.1; the library
    // remains loaded until every handle is destroyed. Drivers only run here.
    unsafe {
        let vk = library("libvulkan.so.1")?;
        let create: VkCreateFn = symbol(&vk, b"vkCreateInstance\0")?;
        let destroy: VkDestroyFn = symbol(&vk, b"vkDestroyInstance\0")?;
        let enumerate: unsafe extern "C" fn(Handle, *mut u32, *mut Handle) -> i32 =
            symbol(&vk, b"vkEnumeratePhysicalDevices\0")?;
        let properties: unsafe extern "C" fn(Handle, *mut c_void) =
            symbol(&vk, b"vkGetPhysicalDeviceProperties\0")?;
        let properties2: Option<VkPropertiesFn> =
            symbol(&vk, b"vkGetPhysicalDeviceProperties2\0").ok();
        let mut app = vk_app((1 << 22) | (1 << 12));
        let mut instance = ptr::null_mut();
        if create(&vk_info(&app, &[]), ptr::null(), &mut instance) != 0 {
            app.api = 1 << 22;
            require(
                create(&vk_info(&app, &[]), ptr::null(), &mut instance) == 0,
                FAILURE,
            )?;
        }
        let result = (|| {
            let mut count = 0;
            require(
                enumerate(instance, &mut count, ptr::null_mut()) == 0 && (1..=16).contains(&count),
                FAILURE,
            )?;
            let mut devices = vec![ptr::null_mut(); count as usize];
            let capacity = count;
            require(
                enumerate(instance, &mut count, devices.as_mut_ptr()) == 0 && count <= capacity,
                FAILURE,
            )?;
            let mut result = Vec::new();
            for device in devices.into_iter().take(count as usize) {
                let mut id = device_id();
                let mut props = VkProperties {
                    kind: 1000059001,
                    next: (&mut id as *mut DeviceId).cast(),
                    data: [0; 512],
                };
                if app.api > 1 << 22
                    && let Some(call) = properties2
                {
                    call(device, &mut props);
                } else {
                    properties(device, props.data.as_mut_ptr().cast());
                }
                let [api, driver, vendor, _, kind] = numbers(&props.data);
                let bytes = props
                    .data
                    .iter()
                    .flat_map(|n| n.to_ne_bytes())
                    .collect::<Vec<_>>();
                let name = String::from_utf8_lossy(
                    bytes[20..276].split(|b| *b == 0).next().unwrap_or(&[]),
                )
                .into_owned();
                if name.is_empty() || !name.bytes().all(|c| (32..127).contains(&c)) {
                    continue;
                }
                let version = if vendor == 0x10de {
                    format!(
                        "{}.{}.{}.{}",
                        driver >> 22,
                        (driver >> 14) & 255,
                        (driver >> 6) & 255,
                        driver & 63
                    )
                } else {
                    format!(
                        "{}.{}.{}",
                        driver >> 22,
                        (driver >> 12) & 1023,
                        driver & 4095
                    )
                };
                let mut value = json!({"name":name,"vendor_id":vendor,"type":kind,"api_version":format!("{}.{}.{}",(api>>22)&127,(api>>12)&1023,api&4095),"driver_version":version});
                if id.uuid.iter().any(|v| *v != 0) {
                    value["device_uuid"] = json!(hex::encode(id.uuid));
                }
                result.push(value);
            }
            Ok(json!({"status":if result.is_empty(){"failed"}else{"ready"},"devices":result}))
        })();
        destroy(instance, ptr::null());
        result
    }
}
pub fn nvidia_directory() -> Result<Value> {
    #[repr(C)]
    struct LinkMap {
        address: usize,
        name: *const c_char,
    }
    // SAFETY: glibc dlinfo RTLD_DI_LINKMAP returns the first two ABI fields of
    // link_map. Keep the original dlopen handle alive while copying its name.
    unsafe {
        let glx = libloading::os::unix::Library::open(
            Some("libGLX_nvidia.so.0"),
            libloading::os::unix::RTLD_LAZY,
        )
        .map_err(|_| Error::Invalid(FAILURE))?;
        let handle = glx.into_raw();
        let _owned = libloading::os::unix::Library::from_raw(handle);
        let dl = library("libdl.so.2")?;
        let info: unsafe extern "C" fn(Handle, i32, *mut *mut LinkMap) -> i32 =
            symbol(&dl, b"dlinfo\0")?;
        let mut map = ptr::null_mut();
        require(
            info(handle, 2, &mut map) == 0 && !map.is_null() && !(*map).name.is_null(),
            FAILURE,
        )?;
        let path =
            std::path::PathBuf::from(CStr::from_ptr((*map).name).to_string_lossy().into_owned())
                .canonicalize()?;
        let folder = path
            .parent()
            .ok_or(Error::Invalid(FAILURE))?
            .join("nvidia/wine");
        Ok(if folder.join("nvngx.dll").is_file() {
            json!(folder)
        } else {
            Value::Null
        })
    }
}
#[repr(C)]
struct XrApp {
    name: [c_char; 128],
    version: u32,
    engine: [c_char; 128],
    engine_version: u32,
    api: u64,
}
#[repr(C)]
struct XrCreate {
    kind: i32,
    next: *const c_void,
    flags: u64,
    app: XrApp,
    layer_count: u32,
    layers: *const *const c_char,
    extension_count: u32,
    extensions: *const *const c_char,
}
#[repr(C)]
struct XrSystem {
    kind: i32,
    next: *const c_void,
    form_factor: i32,
}
#[repr(C)]
struct XrRequirements {
    kind: i32,
    next: *const c_void,
    minimum: u64,
    maximum: u64,
}
type GetXr = unsafe extern "C" fn(Handle, *const c_char, *mut Handle) -> i32;
fn name128(value: &[u8]) -> [c_char; 128] {
    let mut result = [0; 128];
    for (i, b) in value.iter().take(127).enumerate() {
        result[i] = *b as c_char;
    }
    result
}
pub fn vr() -> Value {
    match vr_inner() {
        Ok(value) => value,
        Err(Error::Invalid("headset_missing")) => json!({"state":"headset_missing"}),
        Err(Error::Invalid("loader_missing")) => json!({"state":"loader_missing"}),
        Err(_) => json!({"state":"failed"}),
    }
}
fn xr_checked(code: i32) -> Result<()> {
    if code == -35 {
        Err(Error::Invalid("headset_missing"))
    } else {
        require(code == 0, FAILURE)
    }
}
fn vr_inner() -> Result<Value> {
    // SAFETY: Khronos OpenXR 1.0 and Vulkan C ABI. Function addresses are checked
    // for null; extension buffers and device properties have bounded storage.
    unsafe {
        let xr = library("libopenxr_loader.so.1").map_err(|_| Error::Invalid("loader_missing"))?;
        let get: GetXr = symbol(&xr, b"xrGetInstanceProcAddr\0")?;
        let mut instance: Handle = ptr::null_mut();
        macro_rules! function {
            ($name:literal,$ty:ty) => {{
                let mut pointer = ptr::null_mut();
                xr_checked(get(
                    instance,
                    concat!($name, "\0").as_ptr().cast(),
                    &mut pointer,
                ))?;
                require(!pointer.is_null(), FAILURE)?;
                std::mem::transmute::<Handle, $ty>(pointer)
            }};
        }
        let create = function!(
            "xrCreateInstance",
            unsafe extern "C" fn(*const XrCreate, *mut Handle) -> i32
        );
        let extensions = [c"XR_KHR_vulkan_enable".as_ptr()];
        let info = XrCreate {
            kind: 3,
            next: ptr::null(),
            flags: 0,
            app: XrApp {
                name: name128(b"Flightdeck VR check"),
                version: 1,
                engine: name128(b"Flightdeck"),
                engine_version: 1,
                api: 1 << 48,
            },
            layer_count: 0,
            layers: ptr::null(),
            extension_count: 1,
            extensions: extensions.as_ptr(),
        };
        xr_checked(create(&info, &mut instance))?;
        let destroy = function!("xrDestroyInstance", unsafe extern "C" fn(Handle) -> i32);
        let result = (|| {
            let system_call = function!(
                "xrGetSystem",
                unsafe extern "C" fn(Handle, *const XrSystem, *mut u64) -> i32
            );
            let mut system = 0;
            xr_checked(system_call(
                instance,
                &XrSystem {
                    kind: 4,
                    next: ptr::null(),
                    form_factor: 1,
                },
                &mut system,
            ))?;
            let requirements_call = function!(
                "xrGetVulkanGraphicsRequirementsKHR",
                unsafe extern "C" fn(Handle, u64, *mut XrRequirements) -> i32
            );
            let mut req = XrRequirements {
                kind: 1000025002,
                next: ptr::null(),
                minimum: 0,
                maximum: 0,
            };
            xr_checked(requirements_call(instance, system, &mut req))?;
            type Extensions = unsafe extern "C" fn(Handle, u64, u32, *mut u32, *mut c_char) -> i32;
            let instance_call: Extensions =
                function!("xrGetVulkanInstanceExtensionsKHR", Extensions);
            let device_call: Extensions = function!("xrGetVulkanDeviceExtensionsKHR", Extensions);
            let extension_list = |call: Extensions| -> Result<Vec<String>> {
                let mut count = 0;
                xr_checked(call(instance, system, 0, &mut count, ptr::null_mut()))?;
                require(count > 0 && count <= 32768, FAILURE)?;
                let mut buffer = vec![0_u8; count as usize];
                let capacity = count;
                xr_checked(call(
                    instance,
                    system,
                    capacity,
                    &mut count,
                    buffer.as_mut_ptr().cast(),
                ))?;
                require(count <= capacity, FAILURE)?;
                let text = String::from_utf8_lossy(buffer.split(|v| *v == 0).next().unwrap_or(&[]));
                let mut names = Vec::new();
                for name in text.split_whitespace() {
                    require(
                        name.starts_with("VK_")
                            && name.len() <= 203
                            && name.bytes().all(|v| v.is_ascii_alphanumeric() || v == b'_'),
                        FAILURE,
                    )?;
                    if !names.iter().any(|v| v == name) {
                        names.push(name.to_string());
                    }
                }
                require(names.len() <= 256, FAILURE)?;
                Ok(names)
            };
            let instance_extensions = extension_list(instance_call)?;
            let device_extensions = extension_list(device_call)?;
            let vk = library("libvulkan.so.1")?;
            let create: VkCreateFn = symbol(&vk, b"vkCreateInstance\0")?;
            let destroy_vk: VkDestroyFn = symbol(&vk, b"vkDestroyInstance\0")?;
            let properties: VkPropertiesFn = symbol(&vk, b"vkGetPhysicalDeviceProperties2\0")?;
            let extensions = instance_extensions
                .iter()
                .map(|v| CString::new(v.as_str()).map_err(|_| Error::Invalid(FAILURE)))
                .collect::<Result<Vec<_>>>()?;
            let pointers = extensions.iter().map(|s| s.as_ptr()).collect::<Vec<_>>();
            let minimum = (((req.minimum >> 48) as u32) << 22)
                | ((((req.minimum >> 32) & 0xffff) as u32) << 12)
                | (req.minimum as u32 & 0xfff);
            let app = vk_app(minimum.max((1 << 22) | (1 << 12)));
            let mut vk_instance = ptr::null_mut();
            xr_checked(create(
                &vk_info(&app, &pointers),
                ptr::null(),
                &mut vk_instance,
            ))?;
            let result = (|| {
                let device_call = function!(
                    "xrGetVulkanGraphicsDeviceKHR",
                    unsafe extern "C" fn(Handle, u64, Handle, *mut Handle) -> i32
                );
                let mut device = ptr::null_mut();
                xr_checked(device_call(instance, system, vk_instance, &mut device))?;
                require(!device.is_null(), FAILURE)?;
                let mut id = device_id();
                let mut props = VkProperties {
                    kind: 1000059001,
                    next: (&mut id as *mut DeviceId).cast(),
                    data: [0; 512],
                };
                properties(device, &mut props);
                let [_, _, vendor, product, _] = numbers(&props.data);
                Ok(
                    json!({"state":"ready","vendor_id":vendor,"device_id":product,"device_uuid":hex::encode(id.uuid),"instance_extensions":instance_extensions,"device_extensions":device_extensions}),
                )
            })();
            destroy_vk(vk_instance, ptr::null());
            result
        })();
        let _ = destroy(instance);
        result
    }
}

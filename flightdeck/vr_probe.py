# SPDX-License-Identifier: MIT
"""OpenXR/Vulkan ABI probe. Only load native drivers in a bounded child process."""
from __future__ import annotations

import ctypes as c
import json
import re
import struct


class ProbeError(Exception):
    def __init__(self, stage, result):
        self.stage, self.result = stage, result


def checked(result, stage):
    if result != 0:
        raise ProbeError(stage, result)


def native_probe():
    class App(c.Structure):
        _fields_ = [("name", c.c_char * 128), ("version", c.c_uint32),
                    ("engine", c.c_char * 128), ("engine_version", c.c_uint32), ("api", c.c_uint64)]
    class Create(c.Structure):
        _fields_ = [("type", c.c_int32), ("next", c.c_void_p), ("flags", c.c_uint64),
                    ("app", App), ("layer_count", c.c_uint32), ("layers", c.c_void_p),
                    ("extension_count", c.c_uint32), ("extensions", c.POINTER(c.c_char_p))]
    class System(c.Structure):
        _fields_ = [("type", c.c_int32), ("next", c.c_void_p), ("form_factor", c.c_int32)]
    class Requirements(c.Structure):
        _fields_ = [("type", c.c_int32), ("next", c.c_void_p), ("minimum", c.c_uint64), ("maximum", c.c_uint64)]
    class Properties(c.Structure):
        _fields_ = [("type", c.c_int32), ("next", c.c_void_p), ("version", c.c_uint64), ("name", c.c_char * 128)]
    class VkApp(c.Structure):
        _fields_ = [("type", c.c_uint32), ("next", c.c_void_p), ("name", c.c_char_p),
                    ("version", c.c_uint32), ("engine", c.c_char_p), ("engine_version", c.c_uint32), ("api", c.c_uint32)]
    class VkCreate(c.Structure):
        _fields_ = [("type", c.c_uint32), ("next", c.c_void_p), ("flags", c.c_uint32),
                    ("app", c.POINTER(VkApp)), ("layer_count", c.c_uint32), ("layers", c.c_void_p),
                    ("extension_count", c.c_uint32), ("extensions", c.POINTER(c.c_char_p))]
    class DeviceID(c.Structure):
        _fields_ = [("type", c.c_uint32), ("next", c.c_void_p), ("uuid", c.c_ubyte * 16),
                    ("driver_uuid", c.c_ubyte * 16), ("luid", c.c_ubyte * 8), ("node", c.c_uint32), ("valid", c.c_uint32)]
    class VkProperties(c.Structure):
        _fields_ = [("type", c.c_uint32), ("next", c.c_void_p), ("data", c.c_uint64 * 512)]

    xr = c.CDLL("libopenxr_loader.so.1")
    xr.xrGetInstanceProcAddr.argtypes = [c.c_void_p, c.c_char_p, c.POINTER(c.c_void_p)]
    xr.xrGetInstanceProcAddr.restype = c.c_int32
    instance, vk_instance = c.c_void_p(), c.c_void_p()

    def function(name, args, handle=None):
        address = c.c_void_p()
        checked(xr.xrGetInstanceProcAddr(handle if handle is not None else instance, name.encode(), c.byref(address)), name)
        if not address.value:
            raise ProbeError(name, -7)
        return c.CFUNCTYPE(c.c_int32, *args)(address.value)

    create = function("xrCreateInstance", [c.POINTER(Create), c.POINTER(c.c_void_p)], c.c_void_p())
    extensions = (c.c_char_p * 1)(b"XR_KHR_vulkan_enable")
    info = Create(3, None, 0, App(b"Flightdeck VR check", 1, b"Flightdeck", 1, 1 << 48), 0, None, 1, extensions)
    checked(create(c.byref(info), c.byref(instance)), "xrCreateInstance")
    destroy = function("xrDestroyInstance", [c.c_void_p])
    try:
        properties = Properties(32, None)
        checked(function("xrGetInstanceProperties", [c.c_void_p, c.POINTER(Properties)])(instance, c.byref(properties)), "xrGetInstanceProperties")
        system, system_info = c.c_uint64(), System(4, None, 1)
        checked(function("xrGetSystem", [c.c_void_p, c.POINTER(System), c.POINTER(c.c_uint64)])(instance, c.byref(system_info), c.byref(system)), "xrGetSystem")
        requirements = Requirements(1000025002, None)
        checked(function("xrGetVulkanGraphicsRequirementsKHR", [c.c_void_p, c.c_uint64, c.POINTER(Requirements)])(instance, system, c.byref(requirements)), "xrGetVulkanGraphicsRequirementsKHR")

        def extension_list(name):
            call = function(name, [c.c_void_p, c.c_uint64, c.c_uint32, c.POINTER(c.c_uint32), c.c_void_p])
            count = c.c_uint32()
            checked(call(instance, system, 0, c.byref(count), None), name)
            if not 0 < count.value <= 32768:
                raise ProbeError(name, -1)
            buffer = c.create_string_buffer(count.value)
            checked(call(instance, system, count.value, c.byref(count), buffer), name)
            names = buffer.value.decode("ascii").split()
            if len(names) > 256 or any(not re.fullmatch(r"VK_[A-Za-z0-9_]{1,200}", n) for n in names):
                raise ProbeError(name, -1)
            return list(dict.fromkeys(names))

        instance_ext = extension_list("xrGetVulkanInstanceExtensionsKHR")
        device_ext = extension_list("xrGetVulkanDeviceExtensionsKHR")
        vk = c.CDLL("libvulkan.so.1")
        vk.vkCreateInstance.argtypes = [c.POINTER(VkCreate), c.c_void_p, c.POINTER(c.c_void_p)]
        vk.vkCreateInstance.restype = c.c_int32
        vk.vkDestroyInstance.argtypes = [c.c_void_p, c.c_void_p]
        vk.vkDestroyInstance.restype = None
        vk.vkGetPhysicalDeviceProperties2.argtypes = [c.c_void_p, c.POINTER(VkProperties)]
        vk.vkGetPhysicalDeviceProperties2.restype = None
        # OpenXR encodes API versions differently from Vulkan.
        minimum = ((requirements.minimum >> 48) << 22) | (((requirements.minimum >> 32) & 0xffff) << 12) | (requirements.minimum & 0xfff)
        app = VkApp(0, None, b"Flightdeck VR check", 1, None, 0, max((1 << 22) | (1 << 12), minimum))
        enabled = (c.c_char_p * len(instance_ext))(*(n.encode() for n in instance_ext))
        vk_info = VkCreate(1, None, 0, c.pointer(app), 0, None, len(instance_ext), enabled)
        checked(vk.vkCreateInstance(c.byref(vk_info), None, c.byref(vk_instance)), "vkCreateInstance")
        try:
            device = c.c_void_p()
            checked(function("xrGetVulkanGraphicsDeviceKHR", [c.c_void_p, c.c_uint64, c.c_void_p, c.POINTER(c.c_void_p)])(instance, system, vk_instance, c.byref(device)), "xrGetVulkanGraphicsDeviceKHR")
            if not device.value:
                raise ProbeError("xrGetVulkanGraphicsDeviceKHR", -1)
            identity = DeviceID(1000071004, None)
            props = VkProperties(1000059001, c.addressof(identity))
            vk.vkGetPhysicalDeviceProperties2(device, c.byref(props))
            _, _, vendor, product, _ = struct.unpack_from("=5I", bytes(props.data))
            return {"state": "ready", "runtime_name": properties.name.decode("utf-8", "replace")[:128],
                    "vendor_id": vendor, "device_id": product, "device_uuid": bytes(identity.uuid).hex(),
                    "instance_extensions": instance_ext, "device_extensions": device_ext}
        finally:
            vk.vkDestroyInstance(vk_instance, None)
    finally:
        destroy(instance)


if __name__ == "__main__":
    try:
        result = native_probe()
    except ProbeError as error:
        result = {"state": "headset_missing" if error.result == -35 else "failed", "stage": error.stage, "result": error.result}
    except (OSError, ValueError, AttributeError):
        result = {"state": "loader_missing"}
    print("FLIGHTDECK_XR_RESULT=" + json.dumps(result), flush=True)

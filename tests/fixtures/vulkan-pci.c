// SPDX-License-Identifier: MIT
// Minimal C Vulkan ABI fixture: optional PCI metadata must not break discovery.
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

struct app {
    uint32_t kind;
    const void *next;
    const char *name;
    uint32_t version;
    const char *engine;
    uint32_t engine_version, api;
};
struct create {
    uint32_t kind;
    const void *next;
    uint32_t flags;
    const struct app *app;
};
struct chain {
    uint32_t kind;
    void *next;
};
struct id {
    uint32_t kind;
    void *next;
    uint8_t uuid[16], driver_uuid[16], luid[8];
    uint32_t node, valid;
};
struct pci {
    uint32_t kind;
    void *next;
    uint32_t domain, bus, device, function;
};
struct props2 {
    uint32_t kind;
    void *next;
    uint64_t data[512];
};
struct extension {
    char name[256];
    uint32_t version;
};

static int mode(const char *value) {
    const char *selected = getenv("TEST_VULKAN_MODE");
    return selected && !strcmp(selected, value);
}
int vkCreateInstance(const struct create *info, const void *allocator, void **out) {
    if (allocator || info->kind != 1 || info->app->kind)
        abort();
    if (mode("vulkan10") && info->app->api > (1u << 22))
        return -9;
    *out = (void *)1;
    return 0;
}
void vkDestroyInstance(void *instance, const void *allocator) {
    if (instance != (void *)1 || allocator)
        abort();
}
int vkEnumeratePhysicalDevices(void *instance, uint32_t *count, void **devices) {
    if (instance != (void *)1)
        abort();
    if (devices) {
        if (*count < 1)
            abort();
        devices[0] = (void *)2;
    }
    *count = 1;
    return 0;
}
void vkGetPhysicalDeviceProperties(void *device, void *out) {
    if (device != (void *)2)
        abort();
    uint32_t words[] = {(1u << 22) | (3u << 12), 0, 0x1002, 0x73bf, 2};
    memcpy(out, words, sizeof(words));
    strcpy((char *)out + 20, "Radeon fixture with different driver name");
}
#ifndef NO_PCI_ENUMERATOR
int vkEnumerateDeviceExtensionProperties(void *device, const char *layer, uint32_t *count,
                                         struct extension *out) {
    if (device != (void *)2 || layer)
        abort();
    if (mode("enumeration-error"))
        return -1;
    if (mode("oversized-count")) {
        *count = 1025;
        return 0;
    }
    if (out) {
        if (*count != 1)
            abort();
        if (mode("growing-count")) {
            *count = 2;
            return 0;
        }
        memset(out, 0, sizeof(*out));
        strcpy(out->name, mode("unadvertised") ? "VK_EXT_other" : "VK_EXT_pci_bus_info");
        out->version = 2;
    }
    *count = 1;
    return 0;
}
#endif
#ifndef NO_PROPERTIES2
void vkGetPhysicalDeviceProperties2(void *device, struct props2 *props) {
    if (props->kind != 1000059001 || mode("vulkan10"))
        abort();
    vkGetPhysicalDeviceProperties(device, props->data);
    struct id *id = props->next;
    if (!id || id->kind != 1000071004)
        abort();
    for (unsigned i = 0; i < 16; ++i)
        id->uuid[i] = (uint8_t)(i + 1);
    struct pci *pci = id->next;
    if (!pci)
        return;
#ifdef NO_PCI_ENUMERATOR
    abort();
#endif
    if (pci->kind != 1000212000 || pci->next || mode("unadvertised") || mode("oversized-count") ||
        mode("growing-count") || mode("enumeration-error"))
        abort();
    if (mode("unfilled"))
        return;
    pci->domain = 0;
    pci->bus = 3;
    pci->device = 0;
    pci->function = 0;
    if (mode("invalid-domain"))
        pci->domain = 0x10000;
    if (mode("invalid-bus"))
        pci->bus = 0x100;
    if (mode("invalid-device"))
        pci->device = 0x20;
    if (mode("invalid-function"))
        pci->function = 8;
}
#endif

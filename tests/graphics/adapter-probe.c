/* SPDX-License-Identifier: MIT
 * Check real DXGI selection and explicit/default D3D12 adapter handoff.
 * No game, account, network request or driver modification is involved.
 */
#define COBJMACROS
#define WIDL_C_INLINE_WRAPPERS
#include <windows.h>
#include <dxgi1_6.h>
#include <d3d12.h>
#include <stdio.h>
#include <string.h>

int main(void)
{
    IDXGIFactory1 *factory = NULL;
    IDXGIAdapter1 *adapter = NULL, *extra = NULL;
    ID3D12Device *explicit_device = NULL, *default_device = NULL;
    ID3D12CommandQueue *queue = NULL;
    DXGI_ADAPTER_DESC1 desc;
    D3D12_COMMAND_QUEUE_DESC queue_desc = {0};
    LUID explicit_luid, default_luid;
    HRESULT hr;
    int result = 1;

    hr = CreateDXGIFactory1(&IID_IDXGIFactory1, (void **)&factory);
    printf("CreateDXGIFactory1: 0x%08lx\n", (unsigned long)hr);
    if (FAILED(hr)) goto done;
    hr = IDXGIFactory1_EnumAdapters1(factory, 0, &adapter);
    if (FAILED(hr)) goto done;
    hr = IDXGIFactory1_EnumAdapters1(factory, 1, &extra);
    if (hr != DXGI_ERROR_NOT_FOUND) {
        puts("Expected exactly one selected adapter");
        goto done;
    }
    hr = IDXGIAdapter1_GetDesc1(adapter, &desc);
    if (FAILED(hr) || (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) goto done;
    char name[512];
    WideCharToMultiByte(CP_UTF8, 0, desc.Description, -1, name, sizeof(name), NULL, NULL);
    printf("GPU: %s; vendor=0x%04x\n", name, desc.VendorId);

    hr = D3D12CreateDevice((IUnknown *)adapter, D3D_FEATURE_LEVEL_12_0,
                         &IID_ID3D12Device, (void **)&explicit_device);
    printf("D3D12 explicit adapter: 0x%08lx\n", (unsigned long)hr);
    if (FAILED(hr)) goto done;
    hr = D3D12CreateDevice(NULL, D3D_FEATURE_LEVEL_12_0,
                         &IID_ID3D12Device, (void **)&default_device);
    printf("D3D12 default adapter: 0x%08lx\n", (unsigned long)hr);
    if (FAILED(hr)) goto done;
    explicit_luid = ID3D12Device_GetAdapterLuid(explicit_device);
    default_luid = ID3D12Device_GetAdapterLuid(default_device);
    if (memcmp(&desc.AdapterLuid, &explicit_luid, sizeof(LUID)) ||
        memcmp(&desc.AdapterLuid, &default_luid, sizeof(LUID))) {
        puts("DXGI and D3D12 selected different adapters");
        goto done;
    }
    queue_desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    hr = ID3D12Device_CreateCommandQueue(default_device, &queue_desc,
                                        &IID_ID3D12CommandQueue, (void **)&queue);
    printf("D3D12 command queue: 0x%08lx\n", (unsigned long)hr);
    if (FAILED(hr)) goto done;
    puts("PASS: one hardware adapter shared by DXGI and both D3D12 creation paths");
    result = 0;
done:
    if (queue) ID3D12CommandQueue_Release(queue);
    if (default_device) ID3D12Device_Release(default_device);
    if (explicit_device) ID3D12Device_Release(explicit_device);
    if (extra) IDXGIAdapter1_Release(extra);
    if (adapter) IDXGIAdapter1_Release(adapter);
    if (factory) IDXGIFactory1_Release(factory);
    return result;
}

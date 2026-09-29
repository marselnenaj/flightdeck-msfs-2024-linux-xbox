/* SPDX-License-Identifier: MIT
 * Real D3D12 rendering and readback across multiple DXGI swapchain lifetimes.
 * Run only in an isolated prefix. No simulator or account is used.
 */
#define COBJMACROS
#define WIDL_C_INLINE_WRAPPERS
#include <windows.h>
#include <dxgi1_6.h>
#include <d3d12.h>
#include <d3d11.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define CHECK(call) do { HRESULT hr_ = (call); if (FAILED(hr_)) { \
    fprintf(stderr, "%s failed: 0x%08lx at %d\n", #call, (unsigned long)hr_, __LINE__); exit(1); } } while (0)

static ID3D12Device *device;
static ID3D12CommandQueue *queue;
static IDXGIFactory2 *factory;
static ID3D12CommandAllocator *allocator;
static ID3D12GraphicsCommandList *commands;
static ID3D12DescriptorHeap *rtvs;
static ID3D12Fence *fence;
static HANDLE event;
static UINT64 completed;
static unsigned frames;
static ID3D11Device *device11;
static ID3D11DeviceContext *context11;
static unsigned frames11;

static void render11(IDXGISwapChain1 *chain)
{
    ID3D11Texture2D *buffer, *readback;
    ID3D11RenderTargetView *rtv;
    D3D11_TEXTURE2D_DESC desc;
    D3D11_MAPPED_SUBRESOURCE mapped;
    const float color[4] = {1.f, 0.f, 0.f, 1.f};
    CHECK(IDXGISwapChain1_GetBuffer(chain, 0, &IID_ID3D11Texture2D, (void **)&buffer));
    CHECK(ID3D11Device_CreateRenderTargetView(device11, (ID3D11Resource *)buffer, NULL, &rtv));
    ID3D11DeviceContext_ClearRenderTargetView(context11, rtv, color);
    ID3D11Texture2D_GetDesc(buffer, &desc);
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    desc.MiscFlags = 0;
    CHECK(ID3D11Device_CreateTexture2D(device11, &desc, NULL, &readback));
    ID3D11DeviceContext_CopyResource(context11, (ID3D11Resource *)readback, (ID3D11Resource *)buffer);
    CHECK(ID3D11DeviceContext_Map(context11, (ID3D11Resource *)readback, 0, D3D11_MAP_READ, 0, &mapped));
    for (UINT y = 0; y < desc.Height; ++y) {
        for (UINT x = 0; x < desc.Width; ++x) {
            const unsigned char *p = (const unsigned char *)mapped.pData + y * mapped.RowPitch + x * 4;
            if (p[0] != 255 || p[1] != 0 || p[2] != 0 || p[3] != 255) {
                fprintf(stderr, "Wrong D3D11 pixel in frame %u at %u,%u\n", frames11, x, y);
                exit(1);
            }
        }
    }
    ID3D11DeviceContext_Unmap(context11, (ID3D11Resource *)readback, 0);
    ID3D11Texture2D_Release(readback);
    ID3D11RenderTargetView_Release(rtv);
    ID3D11Texture2D_Release(buffer);
    CHECK(IDXGISwapChain1_Present(chain, 1, 0));
    ++frames11;
}

static void pump(void)
{
    MSG msg;
    while (PeekMessageW(&msg, NULL, 0, 0, PM_REMOVE)) {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

static IDXGISwapChain3 *create_chain(const WCHAR *title, int x, HWND *window)
{
    DXGI_SWAP_CHAIN_DESC1 desc = {0};
    IDXGISwapChain1 *chain;
    IDXGISwapChain3 *result;
    *window = CreateWindowW(L"FlightdeckRendererTest", title, WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                           x, 100, 420, 280, NULL, NULL, GetModuleHandleW(NULL), NULL);
    if (!*window) exit(1);
    desc.Width = 400;
    desc.Height = 240;
    desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.BufferCount = 2;
    desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    CHECK(IDXGIFactory2_CreateSwapChainForHwnd(factory, (IUnknown *)queue, *window, &desc, NULL, NULL, &chain));
    CHECK(IDXGISwapChain1_QueryInterface(chain, &IID_IDXGISwapChain3, (void **)&result));
    IDXGISwapChain1_Release(chain);
    pump();
    return result;
}

static void render(IDXGISwapChain3 *chain, int green)
{
    ID3D12Resource *buffer, *readback;
    D3D12_RESOURCE_DESC desc, download = {0};
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint;
    D3D12_HEAP_PROPERTIES heap = {0};
    D3D12_RESOURCE_BARRIER barrier = {0};
    D3D12_TEXTURE_COPY_LOCATION from = {0}, to = {0};
    D3D12_CPU_DESCRIPTOR_HANDLE handle = ID3D12DescriptorHeap_GetCPUDescriptorHandleForHeapStart(rtvs);
    const float color[4] = {0, green ? 1.f : 0.f, green ? 0.f : 1.f, 1.f};
    UINT64 size;
    unsigned char *pixels;
    UINT index = IDXGISwapChain3_GetCurrentBackBufferIndex(chain);
    CHECK(IDXGISwapChain3_GetBuffer(chain, index, &IID_ID3D12Resource, (void **)&buffer));
    desc = ID3D12Resource_GetDesc(buffer);
    ID3D12Device_GetCopyableFootprints(device, &desc, 0, 1, 0, &footprint, NULL, NULL, &size);
    download.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    download.Width = size;
    download.Height = 1;
    download.DepthOrArraySize = 1;
    download.MipLevels = 1;
    download.SampleDesc.Count = 1;
    download.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    heap.Type = D3D12_HEAP_TYPE_READBACK;
    CHECK(ID3D12Device_CreateCommittedResource(device, &heap, D3D12_HEAP_FLAG_NONE, &download,
          D3D12_RESOURCE_STATE_COPY_DEST, NULL, &IID_ID3D12Resource, (void **)&readback));
    CHECK(ID3D12CommandAllocator_Reset(allocator));
    CHECK(ID3D12GraphicsCommandList_Reset(commands, allocator, NULL));
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = buffer;
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
    barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
    ID3D12GraphicsCommandList_ResourceBarrier(commands, 1, &barrier);
    ID3D12Device_CreateRenderTargetView(device, buffer, NULL, handle);
    ID3D12GraphicsCommandList_ClearRenderTargetView(commands, handle, color, 0, NULL);
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
    barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    ID3D12GraphicsCommandList_ResourceBarrier(commands, 1, &barrier);
    from.pResource = buffer;
    from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    to.pResource = readback;
    to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    to.PlacedFootprint = footprint;
    ID3D12GraphicsCommandList_CopyTextureRegion(commands, &to, 0, 0, 0, &from, NULL);
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
    ID3D12GraphicsCommandList_ResourceBarrier(commands, 1, &barrier);
    CHECK(ID3D12GraphicsCommandList_Close(commands));
    ID3D12CommandQueue_ExecuteCommandLists(queue, 1, (ID3D12CommandList **)&commands);
    CHECK(ID3D12CommandQueue_Signal(queue, fence, ++completed));
    CHECK(ID3D12Fence_SetEventOnCompletion(fence, completed, event));
    if (WaitForSingleObject(event, 10000) != WAIT_OBJECT_0) exit(1);
    CHECK(ID3D12Resource_Map(readback, 0, NULL, (void **)&pixels));
    for (UINT y = 0; y < desc.Height; ++y) {
        for (UINT x = 0; x < desc.Width; ++x) {
            const unsigned char *p = pixels + footprint.Offset + y * footprint.Footprint.RowPitch + x * 4;
            if (p[0] != 0 || p[1] != (green ? 255 : 0) || p[2] != (green ? 0 : 255) || p[3] != 255) {
                fprintf(stderr, "Wrong pixel in %s frame %u buffer %u at %u,%u\n", green ? "primary" : "secondary", frames, index, x, y);
                exit(1);
            }
        }
    }
    ID3D12Resource_Unmap(readback, 0, NULL);
    ID3D12Resource_Release(readback);
    ID3D12Resource_Release(buffer);
    CHECK(IDXGISwapChain3_Present(chain, 1, 0));
    ++frames;
    pump();
}

int main(int argc, char **argv)
{
    IDXGIAdapter1 *adapter;
    DXGI_ADAPTER_DESC1 info;
    D3D12_COMMAND_QUEUE_DESC qdesc = {0};
    D3D12_DESCRIPTOR_HEAP_DESC hdesc = {0};
    WNDCLASSW wc = {0};
    HWND primary_window, secondary_window;
    HWND window11;
    IDXGISwapChain1 *chain11;
    DXGI_SWAP_CHAIN_DESC1 desc11 = {0};
    IDXGISwapChain3 *primary, *secondary;
    (void)argv;
    setvbuf(stdout, NULL, _IONBF, 0);
    wc.lpfnWndProc = DefWindowProcW;
    wc.hInstance = GetModuleHandleW(NULL);
    wc.lpszClassName = L"FlightdeckRendererTest";
    if (!RegisterClassW(&wc)) return 1;
    CHECK(CreateDXGIFactory1(&IID_IDXGIFactory2, (void **)&factory));
    CHECK(IDXGIFactory2_EnumAdapters1(factory, 0, &adapter));
    CHECK(IDXGIAdapter1_GetDesc1(adapter, &info));
    if (info.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) return 1;
    printf("Adapter vendor: %04x\n", info.VendorId);
    CHECK(D3D12CreateDevice((IUnknown *)adapter, D3D_FEATURE_LEVEL_12_0, &IID_ID3D12Device, (void **)&device));
    CHECK(ID3D12Device_CreateCommandQueue(device, &qdesc, &IID_ID3D12CommandQueue, (void **)&queue));
    CHECK(ID3D12Device_CreateCommandAllocator(device, D3D12_COMMAND_LIST_TYPE_DIRECT, &IID_ID3D12CommandAllocator, (void **)&allocator));
    CHECK(ID3D12Device_CreateCommandList(device, 0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator, NULL,
                                      &IID_ID3D12GraphicsCommandList, (void **)&commands));
    CHECK(ID3D12GraphicsCommandList_Close(commands));
    hdesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    hdesc.NumDescriptors = 1;
    CHECK(ID3D12Device_CreateDescriptorHeap(device, &hdesc, &IID_ID3D12DescriptorHeap, (void **)&rtvs));
    CHECK(ID3D12Device_CreateFence(device, 0, D3D12_FENCE_FLAG_NONE, &IID_ID3D12Fence, (void **)&fence));
    event = CreateEventW(NULL, FALSE, FALSE, NULL);
    if (!event) return 1;
    primary = create_chain(L"Flightdeck test - primary green", 100, &primary_window);
    for (unsigned i = 0; i < 8; ++i) render(primary, 1);
    /* MSFS also creates a DXVK D3D11 device while D3D12 is presenting. */
    CHECK(D3D11CreateDevice((IDXGIAdapter *)adapter, D3D_DRIVER_TYPE_UNKNOWN, NULL, 0,
                           NULL, 0, D3D11_SDK_VERSION, &device11, NULL, &context11));
    window11 = CreateWindowW(wc.lpszClassName, L"Flightdeck test - D3D11 red", WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                             100, 450, 420, 280, NULL, NULL, wc.hInstance, NULL);
    if (!window11) return 1;
    desc11.Width = 400; desc11.Height = 240;
    desc11.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc11.SampleDesc.Count = 1;
    desc11.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc11.BufferCount = 2;
    desc11.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    CHECK(IDXGIFactory2_CreateSwapChainForHwnd(factory, (IUnknown *)device11, window11, &desc11, NULL, NULL, &chain11));
    for (unsigned cycle = 0; cycle < 3; ++cycle) {
        secondary = create_chain(L"Flightdeck test - secondary blue", 550, &secondary_window);
        /* Creating an unused second chain must not disturb the first. */
        for (unsigned i = 0; i < 4; ++i) render(primary, 1);
        for (unsigned i = 0; i < 8; ++i) { render(primary, 1); render(secondary, 0); render11(chain11); }
        CHECK(IDXGISwapChain3_ResizeBuffers(secondary, 3, 320, 200, DXGI_FORMAT_UNKNOWN, 0));
        for (unsigned i = 0; i < 8; ++i) { render(primary, 1); render(secondary, 0); render11(chain11); }
        if (argc > 1 && cycle == 0) {
            SetForegroundWindow(primary_window);
            puts("VISIBLE_PRIMARY");
            for (unsigned i = 0; i < 200; ++i) { render(primary, 1); render(secondary, 0); Sleep(20); }
            SetForegroundWindow(secondary_window);
            puts("VISIBLE_SECONDARY");
            for (unsigned i = 0; i < 200; ++i) { render(primary, 1); render(secondary, 0); Sleep(20); }
        }
        IDXGISwapChain3_Release(secondary);
        DestroyWindow(secondary_window);
        for (unsigned i = 0; i < 8; ++i) render(primary, 1);
        CHECK(IDXGISwapChain3_ResizeBuffers(primary, 2, 400 + cycle * 16, 240, DXGI_FORMAT_UNKNOWN, 0));
        CHECK(IDXGISwapChain1_ResizeBuffers(chain11, 2, 320 + cycle * 16, 200, DXGI_FORMAT_UNKNOWN, 0));
    }
    CHECK(ID3D11Device_GetDeviceRemovedReason(device11));
    IDXGISwapChain1_Release(chain11);
    ID3D11DeviceContext_Release(context11);
    ID3D11Device_Release(device11);
    DestroyWindow(window11);
    for (unsigned i = 0; i < 8; ++i) render(primary, 1);
    printf("PASS: %u D3D12 + %u D3D11 rendered/read-back/presented frames across creation, resize and destruction\n", frames, frames11);
    CHECK(ID3D12Device_GetDeviceRemovedReason(device));
    IDXGISwapChain3_Release(primary);
    DestroyWindow(primary_window);
    CloseHandle(event);
    ID3D12Fence_Release(fence);
    ID3D12DescriptorHeap_Release(rtvs);
    ID3D12GraphicsCommandList_Release(commands);
    ID3D12CommandAllocator_Release(allocator);
    ID3D12CommandQueue_Release(queue);
    ID3D12Device_Release(device);
    IDXGIAdapter1_Release(adapter);
    IDXGIFactory2_Release(factory);
    return 0;
}

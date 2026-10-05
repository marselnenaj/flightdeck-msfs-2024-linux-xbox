// SPDX-License-Identifier: MIT
// Adapted from the pinned guentra/dlss5-amd-hip-linux live texture probe.
// Copyright (c) 2026 Kien; see compat/neural-rdna2/UPSTREAM-LICENSE.
// Real D3D12 texture -> complete HIP network -> same-list consumer.
// Isolated test program; does not launch or modify a game.
#include "native_hip_live.h"
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <vector>
static void ck(HRESULT h, const char *s) {
    if (FAILED(h)) {
        fprintf(stderr, "FAIL %s hr=%08lx\n", s, (unsigned long)h);
        ExitProcess(1);
    }
}
static void require(bool b, const char *s) {
    if (!b) {
        fprintf(stderr, "FAIL %s\n", s);
        ExitProcess(1);
    }
}
static void transition(ID3D12GraphicsCommandList *c, ID3D12Resource *r, D3D12_RESOURCE_STATES a,
                       D3D12_RESOURCE_STATES b) {
    D3D12_RESOURCE_BARRIER v{};
    v.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    v.Transition = {r, 0, a, b};
    c->ResourceBarrier(1, &v);
}
static ID3D12Resource *buffer(ID3D12Device *d, UINT64 bytes, D3D12_HEAP_TYPE type) {
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    desc.Width = bytes;
    desc.Height = 1;
    desc.DepthOrArraySize = desc.MipLevels = desc.SampleDesc.Count = 1;
    desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = type;
    ID3D12Resource *r = nullptr;
    ck(d->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
                                  type == D3D12_HEAP_TYPE_READBACK
                                      ? D3D12_RESOURCE_STATE_COPY_DEST
                                      : D3D12_RESOURCE_STATE_GENERIC_READ,
                                  nullptr, IID_PPV_ARGS(&r)),
       "buffer");
    return r;
}
int main() {
    setvbuf(stdout, nullptr, _IONBF, 0);
    SetEnvironmentVariableW(L"DLSS5_CODEC_SRGB", L"1");
    _wputenv_s(L"DLSS5_CODEC_SRGB", L"1");
    IDXGIFactory4 *factory = nullptr;
    ck(CreateDXGIFactory1(IID_PPV_ARGS(&factory)), "factory");
    IDXGIAdapter1 *adapter = nullptr;
    for (UINT i = 0;; ++i) {
        IDXGIAdapter1 *p = nullptr;
        const HRESULT enumeration = factory->EnumAdapters1(i, &p);
        if (enumeration == DXGI_ERROR_NOT_FOUND)
            break;
        ck(enumeration, "enumerate adapter");
        DXGI_ADAPTER_DESC1 d{};
        ck(p->GetDesc1(&d), "adapter description");
        if (wcsstr(d.Description, L"AMD Radeon RX 6900 XT")) {
            adapter = p;
            printf("ADAPTER %ls\n", d.Description);
            break;
        }
        p->Release();
    }
    require(adapter, "AMD Radeon RX 6900 XT present");
    ID3D12Device *device = nullptr;
    ck(D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_12_0, IID_PPV_ARGS(&device)), "device");
    adapter->Release();
    factory->Release();
    require(GetModuleHandleW(L"dlss5-amd.addon64") != nullptr, "ReShade neural add-on loaded");
    printf("PASS ReShade neural add-on loaded\n");
    ID3D12CommandQueue *queue = nullptr;
    D3D12_COMMAND_QUEUE_DESC q{};
    ck(device->CreateCommandQueue(&q, IID_PPV_ARGS(&queue)), "queue");
    ID3D12CommandAllocator *allocator = nullptr;
    ID3D12GraphicsCommandList *list = nullptr;
    ck(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator)),
       "allocator");
    ck(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator, nullptr,
                                 IID_PPV_ARGS(&list)),
       "list");
    const UINT width = 1920, height = 1080;
    D3D12_RESOURCE_DESC td{};
    td.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    td.Width = width;
    td.Height = height;
    td.DepthOrArraySize = td.MipLevels = td.SampleDesc.Count = 1;
    td.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    D3D12_HEAP_PROPERTIES hp{};
    hp.Type = D3D12_HEAP_TYPE_DEFAULT;
    ID3D12Resource *texture = nullptr;
    ck(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &td,
                                       D3D12_RESOURCE_STATE_COPY_DEST, nullptr,
                                       IID_PPV_ARGS(&texture)),
       "texture");
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp{};
    UINT64 size = 0;
    device->GetCopyableFootprints(&td, 0, 1, 0, &fp, nullptr, nullptr, &size);
    auto *upload = buffer(device, size, D3D12_HEAP_TYPE_UPLOAD);
    auto *readback = buffer(device, size, D3D12_HEAP_TYPE_READBACK);
    void *mapped = nullptr;
    ck(upload->Map(0, nullptr, &mapped), "upload map");
    for (UINT y = 0; y < height; ++y)
        for (UINT x = 0; x < width; ++x) {
            auto *p = static_cast<unsigned char *>(mapped) + fp.Offset +
                      size_t(y) * fp.Footprint.RowPitch + x * 4;
            p[0] = 40 + x % 53;
            p[1] = 30 + y % 31;
            p[2] = 20 + (x + y) % 13;
            p[3] = 100 + (x + y) % 151;
        }
    upload->Unmap(0, nullptr);
    D3D12_TEXTURE_COPY_LOCATION tex{}, up{}, rb{};
    tex.pResource = texture;
    tex.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    up.pResource = upload;
    up.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    up.PlacedFootprint = fp;
    rb.pResource = readback;
    rb.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    rb.PlacedFootprint = fp;
    list->CopyTextureRegion(&tex, 0, 0, 0, &up, nullptr);
    transition(list, texture, D3D12_RESOURCE_STATE_COPY_DEST,
               D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    ID3D12Fence *fence = nullptr;
    ck(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)), "fence");
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    require(event != nullptr, "completion event handle");
    UINT64 value = 0;
    auto submit = [&] {
        ck(list->Close(), "close");
        ID3D12CommandList *lists[] = {list};
        queue->ExecuteCommandLists(1, lists);
        ck(queue->Signal(fence, ++value), "signal");
        ck(fence->SetEventOnCompletion(value, event), "completion event");
        require(WaitForSingleObject(event, 10000) == WAIT_OBJECT_0,
                "10-second consumer completion");
        ck(device->GetDeviceRemovedReason(), "device status");
    };
    submit();
    std::vector<unsigned char> original(size_t(width) * height * 4), golden(original.size());
    for (UINT y = 0; y < height; ++y)
        for (UINT x = 0; x < width; ++x) {
            auto *p = original.data() + (size_t(y) * width + x) * 4;
            p[0] = 40 + x % 53;
            p[1] = 30 + y % 31;
            p[2] = 20 + (x + y) % 13;
            p[3] = 100 + (x + y) % 151;
        }
    {
        NativeHipClient direct;
        direct.Create("AMD Radeon RX 6900 XT");
        require(direct.RunFrameRaw(original.data(), golden.data(), width, height,
                                   unsigned(td.Format), 7, true, true, 1, 0) == 0,
                "direct packed reference");
    }
    NativeHipLive live;
    std::vector<unsigned char> first;
    for (unsigned run = 0; run < 3; ++run) {
        ck(allocator->Reset(), "reset allocator");
        ck(list->Reset(allocator, nullptr), "reset list");
        // Re-upload identical original before EACH run, not the prior network output.
        transition(list, texture, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                   D3D12_RESOURCE_STATE_COPY_DEST);
        list->CopyTextureRegion(&tex, 0, 0, 0, &up, nullptr);
        transition(list, texture, D3D12_RESOURCE_STATE_COPY_DEST,
                   D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        bool accepted =
            live.Record(list, texture, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, 7);
        const ULONGLONG deadline = GetTickCount64() + 15000;
        while (!accepted && GetTickCount64() < deadline) {
            Sleep(20);
            accepted =
                live.Record(list, texture, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, 7);
        }
        require(accepted, "live marker accepted after initialization");
        transition(list, texture, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                   D3D12_RESOURCE_STATE_COPY_SOURCE);
        list->CopyTextureRegion(&rb, 0, 0, 0, &tex, nullptr);
        transition(list, texture, D3D12_RESOURCE_STATE_COPY_SOURCE,
                   D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        auto start = std::chrono::steady_clock::now();
        submit();
        double ms =
            std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - start)
                .count();
        ck(readback->Map(0, nullptr, &mapped), "readback map");
        std::vector<unsigned char> packed(size_t(width) * height * 4);
        size_t changed = 0;
        for (UINT y = 0; y < height; ++y)
            for (UINT x = 0; x < width; ++x) {
                auto *p = static_cast<unsigned char *>(mapped) + fp.Offset +
                          size_t(y) * fp.Footprint.RowPitch + x * 4;
                auto *o = packed.data() + (size_t(y) * width + x) * 4;
                memcpy(o, p, 4);
                unsigned char expected[4] = {static_cast<unsigned char>(40 + x % 53),
                                             static_cast<unsigned char>(30 + y % 31),
                                             static_cast<unsigned char>(20 + (x + y) % 13),
                                             static_cast<unsigned char>(100 + (x + y) % 151)};
                require(o[3] == expected[3], "alpha exact");
                for (unsigned c = 0; c < 3; ++c)
                    changed += o[c] != expected[c];
            }
        readback->Unmap(0, nullptr);
        require(packed == golden, "all D3D12 output bytes match direct HIP reference");
        require(changed > 0, "actual network changed RGB");
        if (run)
            require(packed == first, "exact same seed replay through D3D");
        else
            first = packed;
        printf("PASS live texture run=%u changed=%zu alpha_exact=1 replay=1 "
               "reference_bytes=8294400 exact=1 wall_ms=%.3f\n",
               run, changed, ms);
        require(ms < 2000.0, "staging regression: frame must complete within 2 seconds");
    }
    list->Release();
    allocator->Release();
    readback->Release();
    upload->Release();
    texture->Release();
    fence->Release();
    queue->Release();
    device->Release();
    CloseHandle(event);
    return 0;
}

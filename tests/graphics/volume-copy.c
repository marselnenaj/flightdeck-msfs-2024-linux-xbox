/* SPDX-License-Identifier: MIT */
/* Full-volume copies of a 2D-array-compatible 3D texture. With maintenance9,
 * a Vulkan barrier with layerCount=1 covers only the first depth slice.
 * The launcher test also requires Vulkan validation: correct readback alone
 * can hide this layout violation on drivers that tolerate it. */
#define COBJMACROS
#define WIDL_C_INLINE_WRAPPERS
#include <windows.h>
#include <dxgi1_6.h>
#include <d3d12.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#define CHECK(call) do { HRESULT h = (call); if (FAILED(h)) { \
    fprintf(stderr, "%s: %#lx at %d\n", #call, (unsigned long)h, __LINE__); exit(1); } } while (0)

int main(void)
{
    ID3D12Device *device;
    ID3D12CommandQueue *queue;
    ID3D12CommandAllocator *allocator;
    ID3D12GraphicsCommandList *commands;
    ID3D12Resource *source, *destination, *upload, *readback;
    ID3D12Fence *fence;
    D3D12_COMMAND_QUEUE_DESC qdesc = {0};
    D3D12_RESOURCE_DESC texture = {0}, buffer = {0};
    D3D12_HEAP_PROPERTIES heap = {0};
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint;
    D3D12_TEXTURE_COPY_LOCATION from = {0}, to = {0};
    D3D12_RESOURCE_BARRIER barriers[1] = {{0}};
    UINT64 total;
    unsigned char *mapped;
    unsigned errors = 0;
    HANDLE event;
    setvbuf(stdout, NULL, _IONBF, 0);
    CHECK(D3D12CreateDevice(NULL, D3D_FEATURE_LEVEL_12_0, &IID_ID3D12Device, (void **)&device));
    CHECK(ID3D12Device_CreateCommandQueue(device, &qdesc, &IID_ID3D12CommandQueue, (void **)&queue));
    CHECK(ID3D12Device_CreateCommandAllocator(device, D3D12_COMMAND_LIST_TYPE_DIRECT, &IID_ID3D12CommandAllocator, (void **)&allocator));
    CHECK(ID3D12Device_CreateCommandList(device, 0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator, NULL, &IID_ID3D12GraphicsCommandList, (void **)&commands));
    texture.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE3D;
    texture.Width = 32;
    texture.Height = 16;
    texture.DepthOrArraySize = 4;
    texture.MipLevels = 1;
    texture.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    texture.SampleDesc.Count = 1;
    texture.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    CHECK(ID3D12Device_CreateCommittedResource(device, &heap, D3D12_HEAP_FLAG_NONE, &texture, D3D12_RESOURCE_STATE_COPY_DEST, NULL, &IID_ID3D12Resource, (void **)&source));
    CHECK(ID3D12Device_CreateCommittedResource(device, &heap, D3D12_HEAP_FLAG_NONE, &texture, D3D12_RESOURCE_STATE_COPY_DEST, NULL, &IID_ID3D12Resource, (void **)&destination));
    ID3D12Device_GetCopyableFootprints(device, &texture, 0, 1, 0, &footprint, NULL, NULL, &total);
    buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    buffer.Width = total;
    buffer.Height = 1;
    buffer.DepthOrArraySize = 1;
    buffer.MipLevels = 1;
    buffer.SampleDesc.Count = 1;
    buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    heap.Type = D3D12_HEAP_TYPE_UPLOAD;
    CHECK(ID3D12Device_CreateCommittedResource(device, &heap, D3D12_HEAP_FLAG_NONE, &buffer, D3D12_RESOURCE_STATE_GENERIC_READ, NULL, &IID_ID3D12Resource, (void **)&upload));
    heap.Type = D3D12_HEAP_TYPE_READBACK;
    CHECK(ID3D12Device_CreateCommittedResource(device, &heap, D3D12_HEAP_FLAG_NONE, &buffer, D3D12_RESOURCE_STATE_COPY_DEST, NULL, &IID_ID3D12Resource, (void **)&readback));
    CHECK(ID3D12Resource_Map(upload, 0, NULL, (void **)&mapped));
    for (UINT z = 0; z < 4; ++z)
        for (UINT y = 0; y < 16; ++y)
            for (UINT x = 0; x < 32; ++x) {
                unsigned char *pixel = mapped + footprint.Offset + (z * 16 + y) * footprint.Footprint.RowPitch + x * 4;
                pixel[0] = 20 + z * 50;
                pixel[1] = x * 7;
                pixel[2] = y * 13;
                pixel[3] = 255;
            }
    ID3D12Resource_Unmap(upload, 0, NULL);
    from.pResource = upload;
    from.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    from.PlacedFootprint = footprint;
    to.pResource = source;
    to.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    ID3D12GraphicsCommandList_CopyTextureRegion(commands, &to, 0, 0, 0, &from, NULL);
    barriers[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[0].Transition.pResource = source;
    barriers[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barriers[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    barriers[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    ID3D12GraphicsCommandList_ResourceBarrier(commands, 1, barriers);
    from.pResource = source;
    from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    from.SubresourceIndex = 0;
    to.pResource = destination;
    ID3D12GraphicsCommandList_CopyTextureRegion(commands, &to, 0, 0, 0, &from, NULL);
    barriers[0].Transition.pResource = destination;
    ID3D12GraphicsCommandList_ResourceBarrier(commands, 1, barriers);
    from.pResource = destination;
    to.pResource = readback;
    to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    to.PlacedFootprint = footprint;
    ID3D12GraphicsCommandList_CopyTextureRegion(commands, &to, 0, 0, 0, &from, NULL);
    CHECK(ID3D12GraphicsCommandList_Close(commands));
    ID3D12CommandQueue_ExecuteCommandLists(queue, 1, (ID3D12CommandList **)&commands);
    CHECK(ID3D12Device_CreateFence(device, 0, D3D12_FENCE_FLAG_NONE, &IID_ID3D12Fence, (void **)&fence));
    event = CreateEventW(NULL, FALSE, FALSE, NULL);
    if (!event) return 1;
    CHECK(ID3D12CommandQueue_Signal(queue, fence, 1));
    CHECK(ID3D12Fence_SetEventOnCompletion(fence, 1, event));
    if (WaitForSingleObject(event, 10000) != WAIT_OBJECT_0) return 1;
    CHECK(ID3D12Resource_Map(readback, 0, NULL, (void **)&mapped));
    for (UINT z = 0; z < 4; ++z)
        for (UINT y = 0; y < 16; ++y)
            for (UINT x = 0; x < 32; ++x) {
                const unsigned char *pixel = mapped + footprint.Offset + (z * 16 + y) * footprint.Footprint.RowPitch + x * 4;
                if (pixel[0] != 20 + z * 50 || pixel[1] != x * 7 || pixel[2] != y * 13 || pixel[3] != 255) {
                    if (errors < 8) printf("Wrong volume pixel at %u,%u,%u: %u,%u,%u,%u\n", x,y,z,pixel[0],pixel[1],pixel[2],pixel[3]);
                    ++errors;
                }
            }
    ID3D12Resource_Unmap(readback, 0, NULL);
    printf("VOLUME_COPY: pixels=2048 mismatches=%u\n", errors);
    CHECK(ID3D12Device_GetDeviceRemovedReason(device));
    CloseHandle(event);
    ID3D12Fence_Release(fence);
    ID3D12Resource_Release(source);
    ID3D12Resource_Release(destination);
    ID3D12Resource_Release(upload);
    ID3D12Resource_Release(readback);
    ID3D12GraphicsCommandList_Release(commands);
    ID3D12CommandAllocator_Release(allocator);
    ID3D12CommandQueue_Release(queue);
    ID3D12Device_Release(device);
    return errors != 0;
}

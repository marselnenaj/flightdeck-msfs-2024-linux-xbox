/* SPDX-License-Identifier: MIT
 * Real Wine OpenXR -> native runtime stereo smoke test, D3D11 and D3D12.
 * Use only in an isolated test prefix. See docs/vr-validation.md.
 */
#define COBJMACROS
#define WIDL_C_INLINE_WRAPPERS
#define XR_USE_PLATFORM_WIN32
#define XR_USE_GRAPHICS_API_D3D11
#define XR_USE_GRAPHICS_API_D3D12
#define XR_NO_PROTOTYPES
#include <windows.h>
#include <d3d11.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <openxr/openxr.h>
#include <openxr/openxr_platform.h>
#include <openxr/openxr_loader_negotiation.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define FUNCTIONS(X) X(xrCreateInstance) X(xrDestroyInstance) X(xrGetSystem) \
 X(xrCreateSession) X(xrDestroySession) X(xrPollEvent) X(xrBeginSession) \
 X(xrCreateReferenceSpace) X(xrDestroySpace) X(xrEnumerateSwapchainFormats) \
 X(xrCreateSwapchain) X(xrDestroySwapchain) X(xrEnumerateSwapchainImages) \
 X(xrAcquireSwapchainImage) X(xrWaitSwapchainImage) X(xrReleaseSwapchainImage) \
 X(xrWaitFrame) X(xrBeginFrame) X(xrEndFrame) X(xrLocateViews)
#define DECLARE(name) static PFN_##name p_##name;
FUNCTIONS(DECLARE)
#define XR(call) do { XrResult r = (call); if (XR_FAILED(r)) { \
 fprintf(stderr, "%s: %d\n", #call, (int)r); exit(2); } } while (0)
#define HR(call) do { HRESULT r = (call); if (FAILED(r)) { \
 fprintf(stderr, "%s: %#lx\n", #call, (unsigned long)r); exit(3); } } while (0)
#define REQUIRE(value) do { if (!(value)) { fprintf(stderr, "Failed: %s\n", #value); exit(4); } } while (0)

int main(int argc, char **argv)
{
    const int use12 = argc > 1 && !strcmp(argv[1], "d3d12");
    char manifest[4096];
    DWORD length = GetEnvironmentVariableA("XR_RUNTIME_JSON", manifest, sizeof(manifest));
    REQUIRE(length > 3 && length < sizeof(manifest));
    REQUIRE(manifest[1] == ':' && GetFileAttributesA(manifest) != INVALID_FILE_ATTRIBUTES);
    puts("Windows OpenXR manifest is reachable inside the isolated prefix");
    HMODULE library = LoadLibraryA("wineopenxr.dll");
    REQUIRE(library);
    PFN_xrNegotiateLoaderRuntimeInterface negotiate = (void *)GetProcAddress(library, "xrNegotiateLoaderRuntimeInterface");
    REQUIRE(negotiate);
    XrNegotiateLoaderInfo loader = {XR_LOADER_INTERFACE_STRUCT_LOADER_INFO, XR_LOADER_INFO_STRUCT_VERSION,
        sizeof(loader), 1, 1, XR_MAKE_VERSION(1, 0, 0), XR_MAKE_VERSION(1, 1, 0)};
    XrNegotiateRuntimeRequest runtime = {XR_LOADER_INTERFACE_STRUCT_RUNTIME_REQUEST, XR_RUNTIME_INFO_STRUCT_VERSION, sizeof(runtime)};
    XR(negotiate(&loader, &runtime));
    PFN_xrGetInstanceProcAddr get = runtime.getInstanceProcAddr;
    XR(get(XR_NULL_HANDLE, "xrCreateInstance", (PFN_xrVoidFunction *)&p_xrCreateInstance));
    const char *extension = use12 ? XR_KHR_D3D12_ENABLE_EXTENSION_NAME : XR_KHR_D3D11_ENABLE_EXTENSION_NAME;
    XrInstanceCreateInfo create = {XR_TYPE_INSTANCE_CREATE_INFO};
    strcpy(create.applicationInfo.applicationName, "Flightdeck OpenXR stereo validation");
    create.applicationInfo.apiVersion = XR_MAKE_VERSION(1, 0, 0);
    create.enabledExtensionCount = 1;
    create.enabledExtensionNames = &extension;
    XrInstance instance;
    XR(p_xrCreateInstance(&create, &instance));
#define LOAD(name) XR(get(instance, #name, (PFN_xrVoidFunction *)&p_##name));
    FUNCTIONS(LOAD)
    XrSystemGetInfo system_info = {XR_TYPE_SYSTEM_GET_INFO, NULL, XR_FORM_FACTOR_HEAD_MOUNTED_DISPLAY};
    XrSystemId system;
    XR(p_xrGetSystem(instance, &system_info, &system));

    LUID luid;
    if (use12) {
        PFN_xrGetD3D12GraphicsRequirementsKHR requirements;
        XR(get(instance, "xrGetD3D12GraphicsRequirementsKHR", (PFN_xrVoidFunction *)&requirements));
        XrGraphicsRequirementsD3D12KHR needed = {XR_TYPE_GRAPHICS_REQUIREMENTS_D3D12_KHR};
        XR(requirements(instance, system, &needed));
        luid = needed.adapterLuid;
    } else {
        PFN_xrGetD3D11GraphicsRequirementsKHR requirements;
        XR(get(instance, "xrGetD3D11GraphicsRequirementsKHR", (PFN_xrVoidFunction *)&requirements));
        XrGraphicsRequirementsD3D11KHR needed = {XR_TYPE_GRAPHICS_REQUIREMENTS_D3D11_KHR};
        XR(requirements(instance, system, &needed));
        luid = needed.adapterLuid;
    }
    IDXGIFactory4 *factory;
    IDXGIAdapter1 *adapter;
    HR(CreateDXGIFactory1(&IID_IDXGIFactory4, (void **)&factory));
    HR(IDXGIFactory4_EnumAdapterByLuid(factory, luid, &IID_IDXGIAdapter1, (void **)&adapter));
    DXGI_ADAPTER_DESC1 description;
    HR(IDXGIAdapter1_GetDesc1(adapter, &description));
    REQUIRE(!(description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE));
    printf("D3D%d adapter vendor=%04x device=%04x\n", use12 ? 12 : 11, description.VendorId, description.DeviceId);

    ID3D11Device *device11 = NULL;
    ID3D11DeviceContext *context11 = NULL;
    ID3D12Device *device12 = NULL;
    ID3D12CommandQueue *queue = NULL;
    ID3D12CommandAllocator *allocator = NULL;
    ID3D12GraphicsCommandList *commands = NULL;
    ID3D12DescriptorHeap *heap = NULL;
    ID3D12Fence *fence = NULL;
    HANDLE complete = NULL;
    XrGraphicsBindingD3D11KHR binding11 = {XR_TYPE_GRAPHICS_BINDING_D3D11_KHR};
    XrGraphicsBindingD3D12KHR binding12 = {XR_TYPE_GRAPHICS_BINDING_D3D12_KHR};
    if (use12) {
        HR(D3D12CreateDevice((IUnknown *)adapter, D3D_FEATURE_LEVEL_12_0, &IID_ID3D12Device, (void **)&device12));
        D3D12_COMMAND_QUEUE_DESC desc = {0};
        HR(ID3D12Device_CreateCommandQueue(device12, &desc, &IID_ID3D12CommandQueue, (void **)&queue));
        HR(ID3D12Device_CreateCommandAllocator(device12, D3D12_COMMAND_LIST_TYPE_DIRECT, &IID_ID3D12CommandAllocator, (void **)&allocator));
        HR(ID3D12Device_CreateCommandList(device12, 0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator, NULL, &IID_ID3D12GraphicsCommandList, (void **)&commands));
        HR(ID3D12GraphicsCommandList_Close(commands));
        D3D12_DESCRIPTOR_HEAP_DESC heap_info = {D3D12_DESCRIPTOR_HEAP_TYPE_RTV, 1, 0, 0};
        HR(ID3D12Device_CreateDescriptorHeap(device12, &heap_info, &IID_ID3D12DescriptorHeap, (void **)&heap));
        HR(ID3D12Device_CreateFence(device12, 0, D3D12_FENCE_FLAG_NONE, &IID_ID3D12Fence, (void **)&fence));
        complete = CreateEventW(NULL, FALSE, FALSE, NULL);
        REQUIRE(complete);
        binding12.device = device12;
        binding12.queue = queue;
    } else {
        D3D_FEATURE_LEVEL level = D3D_FEATURE_LEVEL_11_0;
        HR(D3D11CreateDevice((IDXGIAdapter *)adapter, D3D_DRIVER_TYPE_UNKNOWN, NULL, 0, &level, 1,
                            D3D11_SDK_VERSION, &device11, NULL, &context11));
        binding11.device = device11;
    }
    XrSessionCreateInfo session_info = {XR_TYPE_SESSION_CREATE_INFO, use12 ? (void *)&binding12 : (void *)&binding11, 0, system};
    XrSession session;
    XR(p_xrCreateSession(instance, &session_info, &session));
    puts("OpenXR session created through Wine");
    XrReferenceSpaceCreateInfo space_info = {XR_TYPE_REFERENCE_SPACE_CREATE_INFO, NULL, XR_REFERENCE_SPACE_TYPE_LOCAL, {{0,0,0,1},{0,0,0}}};
    XrSpace space;
    XR(p_xrCreateReferenceSpace(session, &space_info, &space));
    uint32_t count;
    int64_t formats[128];
    XR(p_xrEnumerateSwapchainFormats(session, 128, &count, formats));
    int64_t format = 0;
    for (uint32_t i = 0; i < count; i++) if (formats[i] == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB) format = formats[i];
    REQUIRE(format);
    XrSwapchainCreateInfo chain_info = {XR_TYPE_SWAPCHAIN_CREATE_INFO, NULL, 0,
        XR_SWAPCHAIN_USAGE_COLOR_ATTACHMENT_BIT | XR_SWAPCHAIN_USAGE_SAMPLED_BIT, format, 1, 128, 128, 1, 2, 1};
    XrSwapchain chain;
    XR(p_xrCreateSwapchain(session, &chain_info, &chain));
    XR(p_xrEnumerateSwapchainImages(chain, 0, &count, NULL));
    REQUIRE(count && count < 32);
    XrSwapchainImageD3D11KHR images11[32] = {{0}};
    XrSwapchainImageD3D12KHR images12[32] = {{0}};
    for (uint32_t i = 0; i < count; i++) {
        images11[i].type = XR_TYPE_SWAPCHAIN_IMAGE_D3D11_KHR;
        images12[i].type = XR_TYPE_SWAPCHAIN_IMAGE_D3D12_KHR;
    }
    XR(p_xrEnumerateSwapchainImages(chain, count, &count, (XrSwapchainImageBaseHeader *)(use12 ? (void *)images12 : (void *)images11)));
    DWORD deadline = GetTickCount() + 10000;
    BOOL ready = FALSE;
    while (!ready && (LONG)(deadline - GetTickCount()) > 0) {
        XrEventDataBuffer event = {XR_TYPE_EVENT_DATA_BUFFER};
        XrResult result = p_xrPollEvent(instance, &event);
        REQUIRE(XR_SUCCEEDED(result));
        if (result == XR_SUCCESS && event.type == XR_TYPE_EVENT_DATA_SESSION_STATE_CHANGED)
            ready = ((XrEventDataSessionStateChanged *)&event)->state == XR_SESSION_STATE_READY;
        if (!ready) Sleep(10);
    }
    REQUIRE(ready);
    XrSessionBeginInfo begin_session = {XR_TYPE_SESSION_BEGIN_INFO, NULL, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO};
    XR(p_xrBeginSession(session, &begin_session));
    for (UINT64 frame = 1; frame <= 8; frame++) {
        XrFrameWaitInfo wait = {XR_TYPE_FRAME_WAIT_INFO};
        XrFrameState state = {XR_TYPE_FRAME_STATE};
        XR(p_xrWaitFrame(session, &wait, &state));
        XrFrameBeginInfo begin = {XR_TYPE_FRAME_BEGIN_INFO};
        XR(p_xrBeginFrame(session, &begin));
        XrViewLocateInfo locate = {XR_TYPE_VIEW_LOCATE_INFO, NULL, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO, state.predictedDisplayTime, space};
        XrViewState view_state = {XR_TYPE_VIEW_STATE};
        XrView views[2] = {{XR_TYPE_VIEW}, {XR_TYPE_VIEW}};
        XR(p_xrLocateViews(session, &locate, &view_state, 2, &count, views));
        REQUIRE(count == 2 && (view_state.viewStateFlags & XR_VIEW_STATE_ORIENTATION_VALID_BIT));
        uint32_t index;
        XrSwapchainImageAcquireInfo acquire = {XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
        XrSwapchainImageWaitInfo wait_image = {XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO, NULL, XR_INFINITE_DURATION};
        XR(p_xrAcquireSwapchainImage(chain, &acquire, &index));
        XR(p_xrWaitSwapchainImage(chain, &wait_image));
        const float color[4] = {0.1f, 0.3f, 0.8f, 1.0f};
        if (use12) {
            HR(ID3D12CommandAllocator_Reset(allocator));
            HR(ID3D12GraphicsCommandList_Reset(commands, allocator, NULL));
            D3D12_RESOURCE_BARRIER barrier = {0};
            barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            barrier.Transition.pResource = images12[index].texture;
            barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
            barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
            barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
            ID3D12GraphicsCommandList_ResourceBarrier(commands, 1, &barrier);
            D3D12_CPU_DESCRIPTOR_HANDLE handle = ID3D12DescriptorHeap_GetCPUDescriptorHandleForHeapStart(heap);
            ID3D12Device_CreateRenderTargetView(device12, images12[index].texture, NULL, handle);
            ID3D12GraphicsCommandList_ClearRenderTargetView(commands, handle, color, 0, NULL);
            barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
            barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
            ID3D12GraphicsCommandList_ResourceBarrier(commands, 1, &barrier);
            HR(ID3D12GraphicsCommandList_Close(commands));
            ID3D12CommandList *list = (ID3D12CommandList *)commands;
            ID3D12CommandQueue_ExecuteCommandLists(queue, 1, &list);
            HR(ID3D12CommandQueue_Signal(queue, fence, frame));
            HR(ID3D12Fence_SetEventOnCompletion(fence, frame, complete));
            REQUIRE(WaitForSingleObject(complete, 10000) == WAIT_OBJECT_0);
        } else {
            ID3D11RenderTargetView *target;
            HR(ID3D11Device_CreateRenderTargetView(device11, (ID3D11Resource *)images11[index].texture, NULL, &target));
            ID3D11DeviceContext_ClearRenderTargetView(context11, target, color);
            ID3D11DeviceContext_Flush(context11);
            ID3D11RenderTargetView_Release(target);
        }
        XrSwapchainImageReleaseInfo release = {XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
        XR(p_xrReleaseSwapchainImage(chain, &release));
        XrCompositionLayerProjectionView projection[2] = {{XR_TYPE_COMPOSITION_LAYER_PROJECTION_VIEW}, {XR_TYPE_COMPOSITION_LAYER_PROJECTION_VIEW}};
        for (uint32_t eye = 0; eye < 2; eye++) {
            projection[eye].pose = views[eye].pose;
            projection[eye].fov = views[eye].fov;
            projection[eye].subImage = (XrSwapchainSubImage){chain, {{0,0},{128,128}}, eye};
        }
        XrCompositionLayerProjection layer = {XR_TYPE_COMPOSITION_LAYER_PROJECTION, NULL, 0, space, 2, projection};
        const XrCompositionLayerBaseHeader *layers[] = {(XrCompositionLayerBaseHeader *)&layer};
        XrFrameEndInfo end = {XR_TYPE_FRAME_END_INFO, NULL, state.predictedDisplayTime, XR_ENVIRONMENT_BLEND_MODE_OPAQUE, 1, layers};
        XR(p_xrEndFrame(session, &end));
    }
    XR(p_xrDestroySwapchain(chain));
    XR(p_xrDestroySpace(space));
    XR(p_xrDestroySession(session));
    XR(p_xrDestroyInstance(instance));
    if (use12) {
        CloseHandle(complete);
        ID3D12Fence_Release(fence);
        ID3D12DescriptorHeap_Release(heap);
        ID3D12GraphicsCommandList_Release(commands);
        ID3D12CommandAllocator_Release(allocator);
        ID3D12CommandQueue_Release(queue);
        ID3D12Device_Release(device12);
    } else {
        ID3D11DeviceContext_Release(context11);
        ID3D11Device_Release(device11);
    }
    IDXGIAdapter1_Release(adapter);
    IDXGIFactory4_Release(factory);
    FreeLibrary(library);
    printf("PASS: D3D%d submitted 8 stereo frames with tracked views through Wine OpenXR\n", use12 ? 12 : 11);
    return 0;
}

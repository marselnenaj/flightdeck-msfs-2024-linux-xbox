/* SPDX-License-Identifier: LGPL-2.1-or-later */
// Narrow bridge to the separately built, genuine Xodus-backed user runtime.
// LGPL-2.1-or-later, like the surrounding runtime sources.
#include "UserBridge.h"
#include <mutex>

namespace {
using BindFn = HRESULT (WINAPI *)(IXThreadingImpl *);
using InitFn = HRESULT (WINAPI *)(ULONG, ULONG, char, const void *);
using QueryFn = HRESULT (WINAPI *)(const GUID *, REFIID, void **);
using ShutdownFn = HRESULT (WINAPI *)();
struct UserState {
    std::mutex lock;
    HMODULE module = nullptr;
    BindFn bind = nullptr;
    InitFn initialize = nullptr;
    QueryFn query = nullptr;
    ShutdownFn shutdown = nullptr;
    bool active = false;
    bool stopping = false;
    bool needs_cleanup = false;
    HRESULT last_error = CO_E_NOTINITIALIZED;
};
UserState &state() {
    // The library and its interface vtables stay loaded for process lifetime.
    // Explicit uninitialization shuts down resources, not the DLL's code pages.
    static UserState *value = new UserState;
    return *value;
}
HRESULT load(UserState &user) {
    if (user.module) return S_OK;
    HMODULE module = LoadLibraryW(L"xodus_store_test.dll");
    if (!module) return HRESULT_FROM_WIN32(GetLastError());
    auto bind = reinterpret_cast<BindFn>(GetProcAddress(module, "XodusUserSetThreading"));
    auto initialize = reinterpret_cast<InitFn>(GetProcAddress(module, "InitializeApiImplEx2"));
    auto query = reinterpret_cast<QueryFn>(GetProcAddress(module, "QueryApiImpl"));
    auto shutdown = reinterpret_cast<ShutdownFn>(GetProcAddress(module, "UninitializeApiImpl"));
    if (!bind || !initialize || !query || !shutdown) {
        FreeLibrary(module);
        return HRESULT_FROM_WIN32(ERROR_PROC_NOT_FOUND);
    }
    user.module = module;
    user.bind = bind;
    user.initialize = initialize;
    user.query = query;
    user.shutdown = shutdown;
    return S_OK;
}
}

bool UserRuntimeEnabled() {
    static const bool enabled = [] {
        char value[4] = {};
        return GetEnvironmentVariableA("XODUS_USER_RUNTIME", value, sizeof(value)) == 1 && value[0] == '1';
    }();
    return enabled;
}

bool IsUserRuntimeClass(const GUID *clsid) {
    // CLSIDs from the pinned PR60 xuser.idl and xgame.idl, respectively.
    static const GUID user = {0x01acd177,0x91f9,0x4763,{0xa3,0x8e,0xcc,0xbb,0x55,0xce,0x32,0xe0}};
    static const GUID game = {0x973a344e,0x24bf,0x4d0f,{0x84,0x57,0x56,0xc5,0x34,0x89,0x2b,0x29}};
    return clsid && (*clsid == user || *clsid == game);
}

HRESULT InitializeUserRuntime(ULONG gdk, ULONG services, char mode, const void *options) {
    if (!UserRuntimeEnabled()) return S_OK;
    auto &user = state();
    std::lock_guard<std::mutex> guard(user.lock);
    if (user.stopping) return E_ILLEGAL_METHOD_CALL;
    if (user.active) return S_OK;
    HRESULT hr = load(user);
    if (SUCCEEDED(hr)) {
        hr = user.bind(x_threading_impl);
        if (SUCCEEDED(hr)) user.needs_cleanup = true;
    }
    if (SUCCEEDED(hr)) hr = user.initialize(gdk, services, mode, options);
    user.last_error = hr;
    user.active = SUCCEEDED(hr);
    std::fprintf(stderr, "xodus-user-bridge: initialize result=%08lx\n", static_cast<ULONG>(hr));
    return hr;
}

HRESULT QueryUserRuntime(const GUID *clsid, REFIID iid, void **out) {
    if (!out || !clsid) return E_POINTER;
    *out = nullptr;
    auto &user = state();
    std::lock_guard<std::mutex> guard(user.lock);
    if (!user.active) return user.last_error;
    HRESULT hr = user.query(clsid, iid, out);
    std::fprintf(stderr, "xodus-user-bridge: query result=%08lx\n", static_cast<ULONG>(hr));
    return hr;
}

HRESULT ShutdownUserRuntime() {
    if (!UserRuntimeEnabled()) return S_OK;
    auto &user = state();
    ShutdownFn shutdown = nullptr;
    {
        std::lock_guard<std::mutex> guard(user.lock);
        if (user.stopping) return S_FALSE;
        if (!user.module || !user.needs_cleanup) return S_OK;
        user.stopping = true;
        user.needs_cleanup = false;
        user.active = false;
        user.last_error = CO_E_NOTINITIALIZED;
        shutdown = user.shutdown;
    }
    // Do not hold the state mutex while draining callbacks/IPC threads.
    HRESULT hr = shutdown();
    {
        std::lock_guard<std::mutex> guard(user.lock);
        user.stopping = false;
    }
    std::fprintf(stderr, "xodus-user-bridge: shutdown result=%08lx\n", static_cast<ULONG>(hr));
    return hr;
}

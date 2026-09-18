/* SPDX-License-Identifier: LGPL-2.1-or-later */
#include <initguid.h>
#include "compat.h"
#include <string>
#include <atomic>
#include <mutex>
#include <limits>
#include "NetworkingState.h"
#include "UserBridge.h"
#include "StoreBridge.h"
#include "GameSaveBridge.h"
#include "RuntimeDiagnostics.h"

static INIT_ONCE base_once = INIT_ONCE_STATIC_INIT;
static HMODULE base_module;
static DWORD base_error;
namespace {
struct RuntimeLifetime {
    std::mutex lock;
    unsigned long long clients = 0;
    bool terminal = false;
};
RuntimeLifetime &lifetime() {
    static RuntimeLifetime *state = new RuntimeLifetime;
    return *state;
}
template<typename Initialize>
HRESULT initialize_client(ULONG gdk, ULONG services, char mode, const void *options,
                          unsigned argc, Initialize initialize) {
    auto &state = lifetime();
    std::lock_guard<std::mutex> lock(state.lock);
    HRESULT hr;
    if (state.terminal) hr = E_UNEXPECTED;
    else if (state.clients == std::numeric_limits<unsigned long long>::max()) hr = E_OUTOFMEMORY;
    else {
        hr = initialize();
        if (SUCCEEDED(hr)) hr = InitializeUserRuntime(gdk, services, mode, options);
        if (SUCCEEDED(hr)) ++state.clients;
    }
    static std::atomic<unsigned> logs{0};
    if (logs.fetch_add(1) < 64)
        std::fprintf(stderr,"xodus-runtime-lifetime: initialize argc=%u clients=%llu result=%08lx\n",
            argc,state.clients,static_cast<ULONG>(hr));
    return hr;
}
}
static BOOL CALLBACK load_base(PINIT_ONCE, PVOID, PVOID*) {
    HMODULE self;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                           reinterpret_cast<LPCWSTR>(&load_base), &self)) {
        base_error = GetLastError(); return TRUE;
    }
    wchar_t path[32768];
    DWORD count = GetModuleFileNameW(self, path, 32768);
    if (!count || count >= 32768) { base_error = ERROR_FILENAME_EXCED_RANGE; return TRUE; }
    std::wstring full(path, count);
    auto slash = full.find_last_of(L"\\/");
    if (slash == std::wstring::npos) { base_error = ERROR_BAD_PATHNAME; return TRUE; }
    full.resize(slash + 1);
    full += L"xgameruntime_original.dll";
    base_module = LoadLibraryExW(full.c_str(), nullptr, LOAD_WITH_ALTERED_SEARCH_PATH);
    if (!base_module) base_error = GetLastError();
    return TRUE;
}
static FARPROC base_proc(const char *name) {
    InitOnceExecuteOnce(&base_once, load_base, nullptr, nullptr);
    if (!base_module) { SetLastError(base_error ? base_error : ERROR_MOD_NOT_FOUND); return nullptr; }
    return GetProcAddress(base_module, name);
}
extern "C" HRESULT WINAPI QueryApiImpl(const GUID *clsid, REFIID iid, void **out) {
    if (!clsid || !out) return E_POINTER;
    *out = nullptr;
    if (*clsid == __uuidof(IXThreadingImpl)) {
        static std::atomic<bool> logged{false};
        if(!logged.exchange(true))std::fprintf(stderr, "xodus-taskqueue: using real PR60 threading implementation\n");
        return x_threading_impl->QueryInterface(iid, out);
    }
    if (XodusGameSaveLocalEnabled() && IsGameSaveRuntimeClass(clsid))
        return QueryGameSaveRuntime(clsid,iid,out);
    if (IsStoreRuntimeClass(clsid)) return QueryStoreRuntime(clsid,iid,out);
    if (*clsid == __uuidof(IXNetworkingImpl)) return x_networking_impl->QueryInterface(iid,out);
    if (IsRuntimeDiagnosticsClass(clsid)) return QueryRuntimeDiagnostics(clsid,iid,out);
    if (UserRuntimeEnabled() && IsUserRuntimeClass(clsid)) return QueryUserRuntime(clsid,iid,out);
    return QueryOriginalApi(clsid,iid,out);
}
HRESULT QueryOriginalApi(const GUID *clsid, REFIID iid, void **out) {
    std::fprintf(stderr, "xodus-taskqueue: forward QueryApi clsid=%08lx-%04x-%04x-%02x%02x-%02x%02x%02x%02x%02x%02x\n", clsid->Data1, clsid->Data2, clsid->Data3, clsid->Data4[0], clsid->Data4[1], clsid->Data4[2], clsid->Data4[3], clsid->Data4[4], clsid->Data4[5], clsid->Data4[6], clsid->Data4[7]);
    using Fn = HRESULT(WINAPI *)(const GUID *, REFIID, void **);
    auto fn = reinterpret_cast<Fn>(base_proc("QueryApiImpl"));
    HRESULT hr=fn ? fn(clsid, iid, out) : HRESULT_FROM_WIN32(GetLastError());
    std::fprintf(stderr,"xodus-taskqueue: forwarded QueryApi result=%08lx\n",static_cast<ULONG>(hr));
    return hr;
}
extern "C" HRESULT WINAPI InitializeApiImpl(ULONG a, ULONG b) {
    return initialize_client(a,b,0,nullptr,2,[&] {
        using Fn=HRESULT(WINAPI *)(ULONG,ULONG); auto fn=reinterpret_cast<Fn>(base_proc("InitializeApiImpl"));
        return fn ? fn(a,b) : HRESULT_FROM_WIN32(GetLastError());
    });
}
extern "C" HRESULT WINAPI InitializeApiImplEx(ULONG a, ULONG b, char c) {
    return initialize_client(a,b,c,nullptr,3,[&] {
        using Fn=HRESULT(WINAPI *)(ULONG,ULONG,char); auto fn=reinterpret_cast<Fn>(base_proc("InitializeApiImplEx"));
        return fn ? fn(a,b,c) : HRESULT_FROM_WIN32(GetLastError());
    });
}
extern "C" HRESULT WINAPI InitializeApiImplEx2(ULONG a, ULONG b, char c, const void *d) {
    return initialize_client(a,b,c,d,4,[&] {
        using Fn=HRESULT(WINAPI *)(ULONG,ULONG,char,const void*); auto fn=reinterpret_cast<Fn>(base_proc("InitializeApiImplEx2"));
        return fn ? fn(a,b,c,d) : HRESULT_FROM_WIN32(GetLastError());
    });
}
extern "C" HRESULT WINAPI DllCanUnloadNow() {
    // Process-lifetime singleton interfaces and the networking timer pin code.
    return S_FALSE;
}
extern "C" HRESULT WINAPI UninitializeApiImpl() {
    auto &state = lifetime();
    {
        std::lock_guard<std::mutex> lock(state.lock);
        if (!state.clients) return S_OK;
        --state.clients;
        static std::atomic<unsigned> logs{0};
        if(logs.fetch_add(1)<64)
            std::fprintf(stderr,"xodus-runtime-lifetime: uninitialize clients=%llu shutdown=%u\n",state.clients,state.clients==0);
        if (state.clients) return S_OK;
        // This private runtime currently has terminal component teardown.
        // Do not claim successful in-process reinitialization of closed state.
        state.terminal = true;
    }
    // Never hold the lifetime mutex while waiting for user callbacks or IPC.
    ShutdownGameSaveRuntime();
    ShutdownStoreRuntime();
    HRESULT hr = ShutdownUserRuntime();
    NetworkRuntimeShutdown();
    // The original snapshot has no initialization resources to release;
    // its export is a Wine raising stub and must not be called.
    return FAILED(hr) ? hr : S_OK;
}
extern "C" void WINAPI XErrorReport(ULONG a, const char *b) {
    using Fn=void(WINAPI *)(ULONG,const char*); auto fn=reinterpret_cast<Fn>(base_proc("XErrorReport"));
    if (fn) fn(a,b);
}

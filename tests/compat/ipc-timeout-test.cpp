/* SPDX-License-Identifier: LGPL-2.1-or-later */
// The actual Wine builtin talks only to the synthetic Unix-socket broker.
#include <initguid.h>
#include "compat.h"
#include <xuser.h>
#include <xstore.h>
#include "StoreCollectionsTypes.h"

struct Options { ULONG unused; BOOLEAN inline_config; const char *config; };
using Init = HRESULT(WINAPI *)(ULONG, ULONG, char, const Options *);
using Bind = HRESULT(WINAPI *)(IXThreadingImpl *);
using Query = HRESULT(WINAPI *)(const GUID *, REFIID, void **);
using Stop = HRESULT(WINAPI *)();
using Account = HRESULT(WINAPI *)(void **);
using ReleaseAccount = void(WINAPI *)(void *);
using Inventory = HRESULT(WINAPI *)(void *, UINT32, UINT32, const char *, const char *, volatile LONG *, XodusStoreCollectionSnapshot **);
using ReleaseInventory = void(WINAPI *)(XodusStoreCollectionSnapshot *);
using License = HRESULT(WINAPI *)(void *, volatile LONG *, XStoreGameLicense *);
using Updates = HRESULT(WINAPI *)(void *, volatile LONG *);

static unsigned failures;
static void check(const char *name, bool passed, HRESULT hr = S_OK) {
    std::printf("%s %s hr=%08lx\n", passed ? "PASS" : "FAIL", name, static_cast<ULONG>(hr));
    if (!passed) ++failures;
}

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    const char *scenario = argv[1];
    auto module = LoadLibraryW(L"xodus_store_test.dll");
    if (!module) { check("load-builtin", false, HRESULT_FROM_WIN32(GetLastError())); return 1; }
    auto bind = reinterpret_cast<Bind>(GetProcAddress(module, "XodusUserSetThreading"));
    auto init = reinterpret_cast<Init>(GetProcAddress(module, "InitializeApiImplEx2"));
    auto query = reinterpret_cast<Query>(GetProcAddress(module, "QueryApiImpl"));
    auto stop = reinterpret_cast<Stop>(GetProcAddress(module, "UninitializeApiImpl"));
    auto account = reinterpret_cast<Account>(GetProcAddress(module, "XodusStoreAcquireAccount"));
    auto release = reinterpret_cast<ReleaseAccount>(GetProcAddress(module, "XodusStoreReleaseAccount"));
    auto inventory = reinterpret_cast<Inventory>(GetProcAddress(module, "XodusStoreQueryInventory"));
    auto release_inventory = reinterpret_cast<ReleaseInventory>(GetProcAddress(module, "XodusStoreReleaseCollections"));
    auto license = reinterpret_cast<License>(GetProcAddress(module, "XodusStoreQueryGameLicense"));
    auto updates = reinterpret_cast<Updates>(GetProcAddress(module, "XodusStoreCheckPackageUpdates"));
    if (!bind || !init || !query || !stop || !account || !release || !inventory || !release_inventory || !license || !updates) return 2;
    Options options{0, TRUE,
        "<Game><Identity Name=\"Test.Game\" Publisher=\"CN=Test\" Version=\"1.0.0.0\"/>"
        "<StoreId>GAME1234EFGH</StoreId><TitleId>1</TitleId>"
        "<MSAAppId>00000000-1111-2222-3333-444444444444</MSAAppId></Game>"};
    HRESULT hr = bind(x_threading_impl);
    if (SUCCEEDED(hr)) hr = init(250600, 0, 0, &options);
    check("initialize", hr == S_OK, hr);
    if (FAILED(hr)) return 1;

    if (!strcmp(scenario, "auth")) {
        IXUserImpl6 *user = nullptr;
        hr = query(&CLSID_XUserImpl, IID_IXUserImpl6, reinterpret_cast<void **>(&user));
        check("user-interface", hr == S_OK, hr);
        if (SUCCEEDED(hr)) {
            XTaskQueueHandle queue = nullptr;
            hr = x_threading_impl->XTaskQueueCreate(XTaskQueueDispatchMode::ThreadPool, XTaskQueueDispatchMode::ThreadPool, &queue);
            XAsyncBlock async{}; async.queue = queue;
            const auto start = GetTickCount64();
            if (SUCCEEDED(hr)) hr = user->XUserAddAsync(XUserAddOptions::AddDefaultUserSilently, &async);
            if (SUCCEEDED(hr)) hr = x_threading_impl->XAsyncGetStatus(&async, TRUE);
            // Deliberately invalid response: parsed after the delay, without
            // making any Xbox HTTP request or loading a real account.
            check("delayed-auth-response-parsed", hr == E_INVALIDARG && GetTickCount64() - start >= 5500, hr);
            if (queue) x_threading_impl->XTaskQueueCloseHandle(queue);
            user->Release();
        }
    }

    void *owned = nullptr;
    hr = account(&owned);
    if (!strcmp(scenario, "account-timeout")) {
        check("account-renewal-still-bounded", hr == HRESULT_FROM_WIN32(ERROR_TIMEOUT) && !owned, hr);
        hr = account(&owned);
        check("late-response-cannot-satisfy-next-request", hr == HRESULT_FROM_WIN32(ERROR_OPERATION_ABORTED) && !owned, hr);
    } else {
        check("account", hr == S_OK && owned, hr);
        if (owned) {
            volatile LONG cancelled = 0;
            if (!strncmp(scenario, "inventory-", 10)) {
                XodusStoreCollectionSnapshot *snapshot = nullptr;
                const auto start = GetTickCount64();
                hr = inventory(owned, 2, 100, "AT", "", &cancelled, &snapshot);
                if (!strcmp(scenario, "inventory-ok"))
                    check("delayed-inventory", hr == S_OK && snapshot && snapshot->item_count == 0, hr);
                else
                    check("broker-timeout-returned", hr == HRESULT_FROM_WIN32(ERROR_TIMEOUT) && !snapshot, hr);
                check("waited-for-broker", GetTickCount64() - start >= 5500);
                if (snapshot) release_inventory(snapshot);
            } else if (!strcmp(scenario, "updates")) {
                const auto start = GetTickCount64();
                hr = updates(owned, &cancelled);
                check("delayed-package-updates", hr == S_OK && GetTickCount64() - start >= 5500, hr);
            }
            XStoreGameLicense result{};
            hr = license(owned, &cancelled, &result);
            check("subsequent-license", hr == S_OK && result.isActive, hr);
            release(owned);
        }
    }
    hr = stop();
    check("shutdown", hr == S_OK, hr);
    std::printf("SUMMARY failures=%u\n", failures);
    return failures ? 1 : 0;
}

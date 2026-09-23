/* SPDX-License-Identifier: LGPL-2.1-or-later */
#include "compat.h"
#include "StoreContext.h"
#include "StoreLicenseEvents.h"
#include "StoreQueries.h"
#include "StoreDurableLicense.h"
#include <atomic>
#include <cstring>
#include <xstore.h>
#include <memory>
#include <mutex>
#include <new>
#include <unordered_map>

class XodusStoreContextState
{
public:
    XodusStoreAccountProvider provider;
    void *account = nullptr;
    std::atomic<bool> closed{false};
    volatile LONG cancelled=0;
    explicit XodusStoreContextState(const XodusStoreAccountProvider &binding) : provider(binding) {}
    ~XodusStoreContextState() { if (account) provider.release_account(provider.state, account); }
};
namespace
{
using Contexts = std::unordered_map<void *, XodusStoreContextRef>;
std::mutex contexts_mutex;
Contexts contexts;
bool stopped;
}

HRESULT XodusStoreContextCreate(const XodusStoreAccountProvider *provider,
                               const void *ignored_pc_user, void **out)
{
    (void)ignored_pc_user; /* Public PC API uses the Store account instead. */
    if (!out) return E_POINTER;
    *out = nullptr;
    if (!provider || !provider->acquire_account || !provider->release_account) return E_INVALIDARG;
    {
        std::lock_guard<std::mutex> lock(contexts_mutex);
        if (stopped) return E_ABORT;
    }
    XodusStoreContextRef context;
    try { context = std::make_shared<XodusStoreContextState>(*provider); }
    catch (const std::bad_alloc &) { return E_OUTOFMEMORY; }
    HRESULT hr = provider->acquire_account(provider->state, &context->account);
    if (FAILED(hr)) return hr;
    if (!context->account) return E_UNEXPECTED;
    try
    {
        std::lock_guard<std::mutex> lock(contexts_mutex);
        if (stopped) return E_ABORT;
        auto handle = context.get();
        /* Retain the local reference even if map allocation/rehashing throws,
         * so the provider release callback cannot run under this lock. */
        contexts.emplace(handle, context);
        *out = handle;
    }
    catch (const std::bad_alloc &) { return E_OUTOFMEMORY; }
    return S_OK;
}

void XodusStoreContextClose(void *handle)
{
    XodusStoreContextRef detached;
    {
        std::lock_guard<std::mutex> lock(contexts_mutex);
        auto found = contexts.find(handle);
        if (found == contexts.end()) return;
        detached = std::move(found->second);
        detached->closed = true;
        InterlockedExchange(&detached->cancelled,1);
        contexts.erase(found);
    }
    XodusStoreQueriesContextClosed(handle);
    XodusStoreDurableContextClosed(handle);
    XodusStoreLicenseEventsContextClosed(handle);
    /* The account release callback runs after dropping the registry lock. */
}

void XodusStoreContextShutdown()
{
    Contexts detached;
    {
        std::lock_guard<std::mutex> lock(contexts_mutex);
        stopped = true;
        contexts.swap(detached);
        for (auto &entry : detached) {
            entry.second->closed = true;
            InterlockedExchange(&entry.second->cancelled,1);
        }
    }
    for(auto &entry:detached)XodusStoreQueriesContextClosed(entry.first);
    XodusStoreDurableShutdown();
    XodusStoreLicenseEventsShutdown();
    /* Provider callbacks run outside the lock. In-flight creates see stopped
     * before publishing and release their acquired reference on return. */
}

HRESULT XodusStoreContextRetain(void *handle, XodusStoreContextRef *reference)
{
    if (!reference) return E_POINTER;
    reference->reset();
    std::lock_guard<std::mutex> lock(contexts_mutex);
    if (stopped) return E_ABORT;
    auto found = contexts.find(handle);
    if (found == contexts.end()) return E_HANDLE;
    *reference = found->second;
    return S_OK;
}

bool XodusStoreContextIsOpen(const XodusStoreContextRef &reference)
{
    return reference && !reference->closed.load();
}

const XodusStoreAccountProvider *XodusStoreContextProvider(const XodusStoreContextRef &reference)
{
    return reference ? &reference->provider : nullptr;
}

void *XodusStoreContextAccount(const XodusStoreContextRef &reference)
{
    return reference ? reference->account : nullptr;
}

HRESULT XodusStoreReadGameLicenseObservation(const XodusStoreContextRef &reference,
                                           XStoreGameLicense *license)
{
    if (!license) return E_POINTER;
    std::memset(license, 0, sizeof(*license));
    if (!XodusStoreContextIsOpen(reference)) return E_ABORT;
    const auto &provider = reference->provider;
    if (!provider.query_game_license) return E_NOTIMPL;
    XStoreGameLicense observed{};
    HRESULT hr = provider.query_game_license(provider.state, reference->account, &reference->cancelled, &observed);
    if (!XodusStoreContextIsOpen(reference)) return E_ABORT;
    if (FAILED(hr)) return hr;
    if (!std::memchr(observed.skuStoreId, 0, sizeof(observed.skuStoreId)) ||
        !std::memchr(observed.trialUniqueId, 0, sizeof(observed.trialUniqueId)) ||
        observed.isActive > 1 || observed.isTrialOwnedByThisUser > 1 ||
        observed.isDiscLicense > 1 || observed.isTrial > 1) return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
    *license = observed;
    return S_OK;
}

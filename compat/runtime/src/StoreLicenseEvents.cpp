/* SPDX-License-Identifier: LGPL-2.1-or-later */
// Private compatibility implementation; LGPL-2.1-or-later.
#include "StoreLicenseEvents.h"
#include "StoreContext.h"
#include <atomic>
#include <map>
#include <memory>
#include <mutex>
#include <vector>
#include <limits>

namespace {
struct Registration {
    std::mutex lock;
    std::condition_variable idle;
    bool active = true;
    bool polling = false;
    unsigned running = 0;
    UINT64 token = 0;
    void *store = nullptr;
    XodusStoreContextRef owner;
    XTaskQueueHandle queue = nullptr;
    void *context = nullptr;
    XStoreGameLicenseChangedCallback *callback = nullptr;
    XStoreGameLicense last{};
    ~Registration() { if (queue) XTaskQueueCloseHandle(queue); }
};
struct Invocation { Registration *registration; Invocation *previous; };
thread_local Invocation *invocation;
thread_local PTP_CALLBACK_INSTANCE timer_instance;
thread_local bool timer_disassociated;
struct Manager {
    std::mutex lock;
    std::map<UINT64, std::shared_ptr<Registration>> registrations;
    UINT64 next = 1;
    PTP_TIMER timer = nullptr;
    HRESULT initialized = E_UNEXPECTED;
    bool stopped = false;
};
void CALLBACK timer_callback(PTP_CALLBACK_INSTANCE, void *, PTP_TIMER);
Manager &manager() {
    // Timer jobs can outlive client shutdown or a forgotten registration. Pin
    // the module and retain this small manager until process termination.
    static Manager *m = [] {
        auto value = new Manager;
        HMODULE module = nullptr;
        if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                reinterpret_cast<LPCWSTR>(&timer_callback), &module)) {
            value->initialized = HRESULT_FROM_WIN32(GetLastError());
            return value;
        }
        value->timer = CreateThreadpoolTimer(timer_callback, value, nullptr);
        value->initialized = value->timer ? S_OK : HRESULT_FROM_WIN32(GetLastError());
        return value;
    }();
    return *m;
}
unsigned local_depth(Registration *r) {
    unsigned depth = 0;
    for (auto i = invocation; i; i = i->previous) if (i->registration == r) ++depth;
    return depth;
}
bool same_license(const XStoreGameLicense &a, const XStoreGameLicense &b) {
    // Strings have been validated by the provider boundary. Padding and the
    // natural trial countdown are not license changes. Crossing zero is.
    return !std::strcmp(a.skuStoreId, b.skuStoreId) &&
        !std::strcmp(a.trialUniqueId, b.trialUniqueId) &&
        a.isActive == b.isActive && a.isTrial == b.isTrial &&
        a.isTrialOwnedByThisUser == b.isTrialOwnedByThisUser &&
        a.isDiscLicense == b.isDiscLicense && a.expirationDate == b.expirationDate &&
        (!a.isTrial || ((!a.trialTimeRemainingInSeconds) == (!b.trialTimeRemainingInSeconds)));
}
void erase_if_idle(const std::shared_ptr<Registration> &reg) {
    auto &m = manager();
    std::lock_guard<std::mutex> manager_lock(m.lock);
    std::lock_guard<std::mutex> registration_lock(reg->lock);
    if (!reg->active && !reg->running) {
        auto found = m.registrations.find(reg->token);
        if (found != m.registrations.end() && found->second == reg) m.registrations.erase(found);
    }
}
void CALLBACK notification(void *value, BOOLEAN cancelled) {
    std::unique_ptr<std::shared_ptr<Registration>> job(static_cast<std::shared_ptr<Registration> *>(value));
    auto reg = *job;
    {
        std::lock_guard<std::mutex> lock(reg->lock);
        if (cancelled || !reg->active || !XodusStoreContextIsOpen(reg->owner)) return;
        ++reg->running;
    }
    Invocation frame{reg.get(), invocation};
    invocation = &frame;
    // Game callbacks must not throw across the native C ABI.
    reg->callback(reg->context);
    invocation = frame.previous;
    {
        std::lock_guard<std::mutex> lock(reg->lock);
        --reg->running;
    }
    reg->idle.notify_all();
    erase_if_idle(reg);
}
void poll(Manager &m) {
    std::vector<std::shared_ptr<Registration>> snapshot;
    {
        std::lock_guard<std::mutex> lock(m.lock);
        if (m.stopped) return;
        for (const auto &entry : m.registrations) snapshot.push_back(entry.second);
    }
    for (auto &reg : snapshot) {
        {
            std::lock_guard<std::mutex> lock(reg->lock);
            if (!reg->active || reg->polling || !XodusStoreContextIsOpen(reg->owner)) continue;
            reg->polling = true;
        }
        XStoreGameLicense next{};
        HRESULT hr = XodusStoreReadGameLicenseObservation(reg->owner, &next);
        bool changed = false;
        {
            std::lock_guard<std::mutex> lock(reg->lock);
            changed = SUCCEEDED(hr) && reg->active && XodusStoreContextIsOpen(reg->owner) && !same_license(reg->last, next);
        }
        // Failed reads, including reauthentication, never manufacture an
        // inactive license or erase the last established license state.
        if (changed) {
            auto job = new(std::nothrow) std::shared_ptr<Registration>(reg);
            hr = job ? XTaskQueueSubmitCallback(reg->queue, XTaskQueuePort::Completion, job, notification) : E_OUTOFMEMORY;
            if (FAILED(hr)) delete job;
        }
        {
            std::lock_guard<std::mutex> lock(reg->lock);
            if (changed && SUCCEEDED(hr)) reg->last = next;
            reg->polling = false;
        }
    }
}
void CALLBACK timer_callback(PTP_CALLBACK_INSTANCE instance, void *context, PTP_TIMER) {
    timer_instance = instance;
    timer_disassociated = false;
    try { poll(*static_cast<Manager *>(context)); }
    catch (const std::bad_alloc &) { /* Retry snapshot allocation next tick. */ }
    timer_instance = nullptr;
}
bool deactivate(const std::shared_ptr<Registration> &reg, bool wait) {
    const auto own = local_depth(reg.get());
    std::unique_lock<std::mutex> lock(reg->lock);
    reg->active = false;
    // A callback can unregister itself. It cannot wait for its own stack to
    // return; report pending until that invocation returns, avoiding deadlock.
    // Two concurrently running callbacks may both unregister themselves. Do
    // not have either wait for the other's stack in that reentrant case.
    if (wait && !own) reg->idle.wait(lock, [&] { return reg->running == 0; });
    return reg->running == 0;
}
}

HRESULT XodusStoreRegisterGameLicenseChanged(void *store, XTaskQueueHandle queue,
    void *context, XStoreGameLicenseChangedCallback *callback, XTaskQueueRegistrationToken *token) {
    if (token) token->token = 0;
    if (!token || !callback) return E_POINTER;
    try {
        auto reg = std::make_shared<Registration>();
        HRESULT hr = XodusStoreContextRetain(store, &reg->owner);
        if (FAILED(hr)) return hr;
        reg->store = store; reg->context = context; reg->callback = callback;
        if (queue) hr = XTaskQueueDuplicateHandle(queue, &reg->queue);
        else hr = XTaskQueueGetCurrentProcessTaskQueue(&reg->queue) ? S_OK : HRESULT_FROM_WIN32(ERROR_NO_TASK_QUEUE);
        if (FAILED(hr)) return hr;
        // No supported real license observation -> no pretend event support.
        hr = XodusStoreReadGameLicenseObservation(reg->owner, &reg->last);
        if (FAILED(hr)) return hr;
        auto &m = manager();
        if (FAILED(m.initialized)) return m.initialized;
        std::lock_guard<std::mutex> lock(m.lock);
        if (m.stopped || !XodusStoreContextIsOpen(reg->owner)) return E_ABORT;
        if (m.next == std::numeric_limits<UINT64>::max()) return E_OUTOFMEMORY;
        reg->token = m.next++;
        m.registrations.emplace(reg->token, reg);
        token->token = reg->token;
#ifndef STORE_EVENTS_TESTING
        LARGE_INTEGER due; due.QuadPart = -50000000LL;
        FILETIME time{due.LowPart, static_cast<DWORD>(due.HighPart)};
        if (m.registrations.size() == 1) SetThreadpoolTimer(m.timer, &time, 5000, 100);
#endif
        return S_OK; // Deliberately no initial notification.
    } catch (const std::bad_alloc &) { return E_OUTOFMEMORY; }
}

BOOLEAN XodusStoreUnregisterGameLicenseChanged(void *store, XTaskQueueRegistrationToken token, BOOLEAN wait) {
    auto &m = manager();
    std::shared_ptr<Registration> reg;
    {
        std::lock_guard<std::mutex> lock(m.lock);
        auto found = m.registrations.find(token.token);
        if (found == m.registrations.end()) return TRUE;
        reg = found->second;
        if (reg->store != store) return FALSE;
    }
    bool idle = deactivate(reg, wait != FALSE);
    if (idle) erase_if_idle(reg);
    return idle ? TRUE : FALSE;
}

void XodusStoreLicenseEventsContextClosed(void *store) {
    auto &m = manager();
    UINT64 cursor = 0;
    // Keep inactive/draining entries discoverable by concurrent Unregister.
    // Otherwise it could return TRUE while its user context is still in use.
    for (;;) {
        std::shared_ptr<Registration> reg;
        {
            std::lock_guard<std::mutex> lock(m.lock);
            for (auto it = m.registrations.upper_bound(cursor); it != m.registrations.end(); ++it) {
                if (it->second->store == store) {
                    reg = it->second;
                    cursor = it->first;
                    std::lock_guard<std::mutex> registration_lock(reg->lock);
                    reg->active = false;
                    break;
                }
            }
        }
        if (!reg) break;
        deactivate(reg, invocation == nullptr);
        erase_if_idle(reg);
    }
}

void XodusStoreLicenseEventsShutdown() {
    auto &m = manager();
    {
        std::lock_guard<std::mutex> lock(m.lock);
        m.stopped = true;
        for (auto &entry : m.registrations) {
            std::lock_guard<std::mutex> registration_lock(entry.second->lock);
            entry.second->active = false;
        }
        if (m.timer) SetThreadpoolTimer(m.timer, nullptr, 0, 0);
    }
    UINT64 cursor = 0;
    for (;;) {
        std::shared_ptr<Registration> reg;
        {
            std::lock_guard<std::mutex> lock(m.lock);
            auto found = m.registrations.upper_bound(cursor);
            if (found == m.registrations.end()) break;
            cursor = found->first;
            reg = found->second;
        }
        deactivate(reg, invocation == nullptr);
        erase_if_idle(reg);
    }
    if (m.timer) {
        // Immediate Completion mode can invoke shutdown inside this timer.
        if (timer_instance && !timer_disassociated) {
            DisassociateCurrentThreadFromCallback(timer_instance);
            timer_disassociated = true;
        }
        // A game callback may be waiting on another callback which is itself
        // executing on the timer. External shutdown drains the timer; callback
        // shutdown only disables it, leaving retained jobs to finish safely.
        if (!invocation) WaitForThreadpoolTimerCallbacks(m.timer, TRUE);
    }
}

#ifdef STORE_EVENTS_TESTING
void XodusStoreLicenseEventsTestPoll() { poll(manager()); }
void XodusStoreLicenseEventsTestArmTimer() {
    LARGE_INTEGER due; due.QuadPart = -10000LL;
    FILETIME time{due.LowPart, static_cast<DWORD>(due.HighPart)};
    SetThreadpoolTimer(manager().timer, &time, 0, 0);
}
#endif

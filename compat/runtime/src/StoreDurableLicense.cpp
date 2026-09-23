/* SPDX-License-Identifier: LGPL-2.1-or-later */
// Online Durable licenses. Every handle starts with an exact Microsoft-signed
// grant, renews while held, and permanently expires if that proof becomes stale.
#include "StoreDurableLicense.h"
#include <algorithm>
#include <atomic>
#include <condition_variable>
#include <cstdint>
#include <cstring>
#include <limits>
#include <map>
#include <mutex>
#include <new>
#include <string>
#include <vector>

namespace {
#ifdef STORE_DURABLES_TESTING
std::atomic<UINT64> time_offset{0};
#endif
UINT64 ticks() {
    return GetTickCount64()
#ifdef STORE_DURABLES_TESTING
        + time_offset.load()
#endif
        ;
}
INT64 utc() {
    FILETIME ft; GetSystemTimeAsFileTime(&ft);
    return static_cast<INT64>(((static_cast<UINT64>(ft.dwHighDateTime)<<32)|ft.dwLowDateTime)/10000000)-11644473600LL
#ifdef STORE_DURABLES_TESTING
        + static_cast<INT64>(time_offset.load()/1000)
#endif
        ;
}
struct License {
    std::mutex lock;
    XodusStoreContextRef owner;
    std::string product;
    XStoreLicenseHandle handle=nullptr;
    XStoreGameLicense proof{};
    UINT64 deadline=0, renew_at=0;
    bool closed=false, lost=false, polling=false;
    volatile LONG cancelled=0;
};
struct Registration {
    std::mutex lock;
    std::condition_variable idle;
    XStoreLicenseHandle handle=nullptr;
    XTaskQueueHandle queue=nullptr;
    void *context=nullptr;
    XStorePackageLicenseLostCallback *callback=nullptr;
    UINT64 token=0;
    bool active=true, queued=false, delivered=false;
    unsigned running=0;
    ~Registration(){if(queue)XTaskQueueCloseHandle(queue);}
};
struct Invocation {Registration *registration;Invocation *previous;};
thread_local Invocation *invocation=nullptr;
thread_local PTP_CALLBACK_INSTANCE timer_instance=nullptr;
struct Manager {
    std::mutex lock;
    std::map<void*,std::shared_ptr<License>> licenses;
    std::map<UINT64,std::shared_ptr<Registration>> registrations;
    UINT64 next_handle=1,next_token=1;
    PTP_TIMER timer=nullptr;
    HRESULT initialized=E_UNEXPECTED;
    bool stopped=false;
};
void CALLBACK timer_callback(PTP_CALLBACK_INSTANCE,void*,PTP_TIMER);
Manager &manager() {
    static Manager *m=[] {
        auto value=new Manager;
        HMODULE module=nullptr;
        if(!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_PIN,
            reinterpret_cast<LPCWSTR>(&timer_callback),&module)) {
            value->initialized=HRESULT_FROM_WIN32(GetLastError());return value;
        }
        value->timer=CreateThreadpoolTimer(timer_callback,value,nullptr);
        value->initialized=value->timer?S_OK:HRESULT_FROM_WIN32(GetLastError());
        return value;
    }();
    return *m;
}
bool valid_locked(License &license) {
    if(license.closed||license.lost)return false;
    if(!XodusStoreContextIsOpen(license.owner)||utc()>=license.proof.expirationDate||ticks()>=license.deadline) {
        license.lost=true;InterlockedExchange(&license.cancelled,1);return false;
    }
    return true;
}
bool set_proof(License &license,const XStoreGameLicense &proof) {
    const INT64 seconds=proof.expirationDate-utc();
    if(seconds<=0||seconds>60)return false;
    const UINT64 remaining=static_cast<UINT64>(seconds)*1000,now=ticks();
    license.proof=proof;license.deadline=now+remaining;
    license.renew_at=now+std::min<UINT64>(20000,remaining/3);
    return true;
}
std::shared_ptr<License> retain(XStoreLicenseHandle handle) {
    auto &m=manager();std::lock_guard<std::mutex> lock(m.lock);
    auto it=m.licenses.find(handle);
    return it==m.licenses.end()?nullptr:it->second;
}
HRESULT observe(const XodusStoreContextRef &owner,const char *product,volatile LONG *cancelled,XStoreGameLicense *proof) {
    if(!XodusStoreContextIsOpen(owner)||InterlockedCompareExchange(cancelled,0,0))return E_ABORT;
    auto provider=XodusStoreContextProvider(owner);
    if(!provider->query_durable_license)return E_NOTIMPL;
    *proof={};
    HRESULT hr=provider->query_durable_license(provider->state,XodusStoreContextAccount(owner),product,cancelled,proof);
    if(!XodusStoreContextIsOpen(owner)||InterlockedCompareExchange(cancelled,0,0))return E_ABORT;
    if(FAILED(hr))return hr;
    const INT64 now=utc();
    if(strnlen(proof->skuStoreId,sizeof(proof->skuStoreId))!=17||std::strncmp(proof->skuStoreId,product,12)||
        proof->skuStoreId[12]!='/'||!std::all_of(proof->skuStoreId+13,proof->skuStoreId+17,[](char c){return(c>='A'&&c<='Z')||(c>='0'&&c<='9');})||
        proof->isActive!=TRUE||proof->isTrial||proof->isTrialOwnedByThisUser||proof->isDiscLicense||
        proof->trialTimeRemainingInSeconds||proof->trialUniqueId[0]||proof->expirationDate<=now||proof->expirationDate>now+60)
        return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
    return S_OK;
}
bool own_callback(Registration *reg) {
    for(auto frame=invocation;frame;frame=frame->previous)if(frame->registration==reg)return true;
    return false;
}
bool deactivate(const std::shared_ptr<Registration> &reg,bool wait) {
    std::unique_lock<std::mutex> lock(reg->lock);reg->active=false;
    if(wait&&!own_callback(reg.get()))reg->idle.wait(lock,[&]{return reg->running==0;});
    return !reg->running;
}
void erase_if_idle(const std::shared_ptr<Registration> &reg) {
    auto &m=manager();std::lock_guard<std::mutex> lock(m.lock);
    std::lock_guard<std::mutex> local(reg->lock);
    if(!reg->active&&!reg->running) {
        auto it=m.registrations.find(reg->token);
        if(it!=m.registrations.end()&&it->second==reg)m.registrations.erase(it);
    }
}
void CALLBACK notification(void *opaque,BOOLEAN cancelled) {
    std::unique_ptr<std::shared_ptr<Registration>> job(static_cast<std::shared_ptr<Registration>*>(opaque));
    auto reg=*job;
    {
        std::lock_guard<std::mutex> lock(reg->lock);reg->queued=false;
        if(cancelled||!reg->active||reg->delivered)return;
        reg->delivered=true;++reg->running;
    }
    Invocation frame{reg.get(),invocation};invocation=&frame;
    reg->callback(reg->context);
    invocation=frame.previous;
    {std::lock_guard<std::mutex> lock(reg->lock);--reg->running;}
    reg->idle.notify_all();erase_if_idle(reg);
}
void notify_lost(XStoreLicenseHandle handle) {
    auto &m=manager();std::vector<std::shared_ptr<Registration>> registrations;
    {
        std::lock_guard<std::mutex> lock(m.lock);
        if(m.stopped)return;
        for(auto &entry:m.registrations)if(entry.second->handle==handle)registrations.push_back(entry.second);
    }
    for(auto &reg:registrations) {
        {
            std::lock_guard<std::mutex> lock(reg->lock);
            if(!reg->active||reg->queued||reg->delivered)continue;
            reg->queued=true;
        }
        auto job=new(std::nothrow)std::shared_ptr<Registration>(reg);
        HRESULT hr=job?XTaskQueueSubmitCallback(reg->queue,XTaskQueuePort::Completion,job,notification):E_OUTOFMEMORY;
        if(FAILED(hr)) {
            delete job;std::lock_guard<std::mutex> lock(reg->lock);reg->queued=false;
        }
    }
}
void poll(Manager &m) {
    std::vector<std::shared_ptr<License>> licenses;
    {
        std::lock_guard<std::mutex> lock(m.lock);if(m.stopped)return;
        for(auto &entry:m.licenses)licenses.push_back(entry.second);
    }
    for(auto &license:licenses) {
        bool refresh=false,lost=false;
        {
            std::lock_guard<std::mutex> lock(license->lock);
            lost=!valid_locked(*license);
            if(!lost&&!license->polling&&ticks()>=license->renew_at) {license->polling=true;refresh=true;}
        }
        if(refresh) {
            XStoreGameLicense proof{};HRESULT hr;
            try{hr=observe(license->owner,license->product.c_str(),&license->cancelled,&proof);}
            catch(const std::bad_alloc&){hr=E_OUTOFMEMORY;}
            catch(...){hr=E_FAIL;}
            std::lock_guard<std::mutex> lock(license->lock);
            // Never revive a handle that expired or was lost during the read.
            if(valid_locked(*license)&&SUCCEEDED(hr)) {
                if(!set_proof(*license,proof))license->renew_at=ticks()+1000;
            } else if(FAILED(hr)) license->renew_at=ticks()+5000;
            license->polling=false;lost=!valid_locked(*license);
        }
        if(lost)notify_lost(license->handle);
    }
}
void CALLBACK timer_callback(PTP_CALLBACK_INSTANCE instance,void *value,PTP_TIMER) {
    timer_instance=instance;
    try{poll(*static_cast<Manager*>(value));}catch(const std::bad_alloc&){}
    timer_instance=nullptr;
}
}

HRESULT XodusStoreDurableAcquire(const XodusStoreContextRef &owner,const char *product,
    volatile LONG *cancelled,XStoreLicenseHandle *out) {
    if(out)*out=nullptr;
    if(!out||!product||!cancelled)return E_POINTER;
    if(strnlen(product,13)!=12||!std::all_of(product,product+12,[](char c){return(c>='A'&&c<='Z')||(c>='0'&&c<='9');}))return E_INVALIDARG;
    try {
        auto license=std::make_shared<License>();license->owner=owner;license->product=product;
        HRESULT hr=observe(owner,product,cancelled,&license->proof);if(FAILED(hr))return hr;
        if(!set_proof(*license,license->proof))return HRESULT_FROM_WIN32(ERROR_LOGON_FAILURE);
        auto &m=manager();if(FAILED(m.initialized))return m.initialized;
        std::lock_guard<std::mutex> lock(m.lock);
        if(m.stopped||!XodusStoreContextIsOpen(owner)||InterlockedCompareExchange(cancelled,0,0))return E_ABORT;
        if(m.licenses.size()>=128||m.next_handle>=std::numeric_limits<uintptr_t>::max())return E_OUTOFMEMORY;
        license->handle=reinterpret_cast<void*>(static_cast<uintptr_t>(m.next_handle++));
        m.licenses.emplace(license->handle,license);*out=license->handle;
#ifndef STORE_DURABLES_TESTING
        if(m.licenses.size()==1){LARGE_INTEGER due;due.QuadPart=-10000000LL;FILETIME ft{due.LowPart,static_cast<DWORD>(due.HighPart)};SetThreadpoolTimer(m.timer,&ft,1000,100);}
#endif
        return S_OK;
    }catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
}
BOOLEAN XodusStoreIsLicenseValid(XStoreLicenseHandle handle) {
    auto license=retain(handle);if(!license)return FALSE;
    std::lock_guard<std::mutex> lock(license->lock);return valid_locked(*license)?TRUE:FALSE;
}
void XodusStoreCloseLicenseHandle(XStoreLicenseHandle handle) {
    auto &m=manager();std::shared_ptr<License> license;
    {
        std::lock_guard<std::mutex> lock(m.lock);
        auto it=m.licenses.find(handle);if(it==m.licenses.end())return;
        license=it->second;m.licenses.erase(it);
        std::lock_guard<std::mutex> local(license->lock);license->closed=true;InterlockedExchange(&license->cancelled,1);
    }
    UINT64 cursor=0;
    for(;;) {
        std::shared_ptr<Registration> reg;
        {
            std::lock_guard<std::mutex> lock(m.lock);
            for(auto it=m.registrations.upper_bound(cursor);it!=m.registrations.end();++it)if(it->second->handle==handle){cursor=it->first;reg=it->second;break;}
        }
        if(!reg)break;
        deactivate(reg,invocation==nullptr);erase_if_idle(reg);
    }
}
HRESULT XodusStoreRegisterPackageLicenseLost(XStoreLicenseHandle handle,XTaskQueueHandle queue,
    void *context,XStorePackageLicenseLostCallback *callback,XTaskQueueRegistrationToken *token) {
    if(token)token->token=0;
    if(!token||!callback)return E_POINTER;
    try {
        auto license=retain(handle);if(!license)return E_HANDLE;
        auto reg=std::make_shared<Registration>();reg->handle=handle;reg->context=context;reg->callback=callback;
        HRESULT hr=queue?XTaskQueueDuplicateHandle(queue,&reg->queue):XTaskQueueGetCurrentProcessTaskQueue(&reg->queue)?S_OK:HRESULT_FROM_WIN32(ERROR_NO_TASK_QUEUE);
        if(FAILED(hr))return hr;
        auto &m=manager();std::lock_guard<std::mutex> lock(m.lock);
        if(m.stopped||!m.licenses.count(handle))return E_HANDLE;
        if(m.next_token==std::numeric_limits<UINT64>::max()||m.registrations.size()>=1024)return E_OUTOFMEMORY;
        reg->token=m.next_token++;m.registrations.emplace(reg->token,reg);token->token=reg->token;
        return S_OK;
    }catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
}
BOOLEAN XodusStoreUnregisterPackageLicenseLost(XStoreLicenseHandle handle,XTaskQueueRegistrationToken token,BOOLEAN wait) {
    auto &m=manager();std::shared_ptr<Registration> reg;
    {std::lock_guard<std::mutex> lock(m.lock);auto it=m.registrations.find(token.token);if(it==m.registrations.end())return TRUE;reg=it->second;if(reg->handle!=handle)return FALSE;}
    bool idle=deactivate(reg,wait!=FALSE);if(idle)erase_if_idle(reg);return idle?TRUE:FALSE;
}
void XodusStoreDurableContextClosed(void *context) {
    auto &m=manager();std::lock_guard<std::mutex> lock(m.lock);
    for(auto &entry:m.licenses)if(entry.second->owner.get()==context) {
        std::lock_guard<std::mutex> local(entry.second->lock);entry.second->lost=true;InterlockedExchange(&entry.second->cancelled,1);
    }
}
void XodusStoreDurableShutdown() {
    auto &m=manager();
    {
        std::lock_guard<std::mutex> lock(m.lock);m.stopped=true;
        if(m.timer)SetThreadpoolTimer(m.timer,nullptr,0,0);
    }
    for(;;) {
        XStoreLicenseHandle handle=nullptr;
        {std::lock_guard<std::mutex> lock(m.lock);if(m.licenses.empty())break;handle=m.licenses.begin()->first;}
        XodusStoreCloseLicenseHandle(handle);
    }
    if(m.timer&&!invocation)WaitForThreadpoolTimerCallbacks(m.timer,TRUE);
}
#ifdef STORE_DURABLES_TESTING
void XodusStoreDurableTestAdvance(UINT64 milliseconds){time_offset.fetch_add(milliseconds);}
void XodusStoreDurableTestPoll(){poll(manager());}
SIZE_T XodusStoreDurableTestCount(){auto &m=manager();std::lock_guard<std::mutex> lock(m.lock);return m.licenses.size();}
#endif

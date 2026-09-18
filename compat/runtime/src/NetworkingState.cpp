/* SPDX-License-Identifier: LGPL-2.1-or-later */
// Personal IXNetworking adaptation. LGPL-2.1-or-later, like the surrounding Wine code.
#include <winsock2.h>
#include <ws2tcpip.h>
#include <iphlpapi.h>
#include <netioapi.h>
#include "NetworkingState.h"
#include "NetworkSecurity.h"
#include <atomic>
#include <memory>
#include <mutex>
#include <condition_variable>
#include <map>
#include <vector>
#include <limits>

// Only local adapter/route state is queried. No external endpoint or fabricated
// online result is used. InternetAccess is a routing hint, not a reachability test.
static HRESULT read_platform_hint(XNetworkingConnectivityHint *hint) {
    if (!hint) return E_POINTER;
    *hint={};
    hint->connectivityCost=XNetworkingConnectivityCostHint::Unknown;
    ULONG bytes=16384;
    std::vector<unsigned char> buffer(bytes);
    ULONG error=ERROR_BUFFER_OVERFLOW;
    for(unsigned attempt=0;attempt<4 && error==ERROR_BUFFER_OVERFLOW;++attempt) {
        buffer.resize(bytes);
        error=GetAdaptersAddresses(AF_UNSPEC,GAA_FLAG_SKIP_ANYCAST|GAA_FLAG_SKIP_MULTICAST|GAA_FLAG_SKIP_DNS_SERVER,
            nullptr,reinterpret_cast<IP_ADAPTER_ADDRESSES*>(buffer.data()),&bytes);
    }
    if(error!=NO_ERROR && error!=ERROR_NO_DATA) return HRESULT_FROM_WIN32(error);
    hint->networkInitialized=TRUE;
    hint->connectivityLevel=XNetworkingConnectivityLevelHint::None;
    if(error==ERROR_NO_DATA) return S_OK;
    PMIB_IPFORWARD_TABLE2 routes=nullptr;
    DWORD route_error=GetIpForwardTable2(AF_UNSPEC,&routes);
    uint64_t best_metric=std::numeric_limits<uint64_t>::max();
    for(auto adapter=reinterpret_cast<IP_ADAPTER_ADDRESSES*>(buffer.data());adapter;adapter=adapter->Next) {
        if(adapter->IfType==IF_TYPE_SOFTWARE_LOOPBACK || adapter->OperStatus!=IfOperStatusUp) continue;
        bool ipv4=false,ipv6=false;
        for(auto address=adapter->FirstUnicastAddress;address;address=address->Next) {
            if(!address->Address.lpSockaddr) continue;
            const auto family=address->Address.lpSockaddr->sa_family;
            if(family==AF_INET) {
                const auto *ip=reinterpret_cast<const sockaddr_in*>(address->Address.lpSockaddr);
                const ULONG value=ntohl(ip->sin_addr.s_addr);
                if(value && (value>>24)!=127) ipv4=true;
            } else if(family==AF_INET6) {
                const auto *ip=reinterpret_cast<const sockaddr_in6*>(address->Address.lpSockaddr);
                if(!IN6_IS_ADDR_UNSPECIFIED(&ip->sin6_addr)&&!IN6_IS_ADDR_LOOPBACK(&ip->sin6_addr)) ipv6=true;
            }
        }
        if(!ipv4&&!ipv6) continue;
        if(hint->connectivityLevel==XNetworkingConnectivityLevelHint::None) {
            hint->connectivityLevel=XNetworkingConnectivityLevelHint::LocalAccess;
            hint->ianaInterfaceType=adapter->IfType;
        }
        if(route_error!=NO_ERROR || !routes) continue;
        for(ULONG i=0;i<routes->NumEntries;++i) {
            const auto &route=routes->Table[i];
            if(route.DestinationPrefix.PrefixLength!=0 || route.Loopback || route.ValidLifetime==0) continue;
            if(route.InterfaceLuid.Value!=adapter->Luid.Value) continue;
            const auto family=route.DestinationPrefix.Prefix.si_family;
            if((family==AF_INET&&!ipv4)||(family==AF_INET6&&!ipv6)) continue;
            if(family!=AF_INET&&family!=AF_INET6) continue;
            const uint64_t metric=static_cast<uint64_t>(route.Metric)+(family==AF_INET?adapter->Ipv4Metric:adapter->Ipv6Metric);
            if(metric<best_metric) {
                best_metric=metric;
                hint->connectivityLevel=XNetworkingConnectivityLevelHint::InternetAccess;
                hint->ianaInterfaceType=adapter->IfType;
            }
        }
    }
    if(routes) FreeMibTable(routes);
    return S_OK;
}
#ifdef NETWORKING_TESTING
static ConnectivityReader test_reader=nullptr;
void NetworkTestSetReader(ConnectivityReader reader) { test_reader=reader; }
#endif
static HRESULT read_hint(XNetworkingConnectivityHint *hint) {
    try {
#ifdef NETWORKING_TESTING
        if(test_reader) return test_reader(hint);
#endif
        return read_platform_hint(hint);
    } catch(const std::bad_alloc&) { return E_OUTOFMEMORY; }
}
static bool same_hint(const XNetworkingConnectivityHint&a,const XNetworkingConnectivityHint&b) {
    return a.connectivityLevel==b.connectivityLevel&&a.connectivityCost==b.connectivityCost&&a.ianaInterfaceType==b.ianaInterfaceType&&
        a.networkInitialized==b.networkInitialized&&a.approachingDataLimit==b.approachingDataLimit&&a.overDataLimit==b.overDataLimit&&a.roaming==b.roaming;
}
struct Registration {
    std::mutex lock;
    std::condition_variable idle;
    bool active=true;
    bool initial_enqueued=false;
    unsigned running=0;
    XTaskQueueHandle queue=nullptr;
    void *context=nullptr;
    XNetworkingConnectivityHintChangedCallback *callback=nullptr;
    XNetworkingConnectivityHint last{};
    ~Registration() { if(queue) XTaskQueueCloseHandle(queue); }
};
struct InvocationFrame { Registration *registration; InvocationFrame *previous; };
static thread_local InvocationFrame *current_invocation;
static thread_local PTP_CALLBACK_INSTANCE current_timer_instance;
static thread_local bool timer_disassociated;
struct CallbackJob { std::shared_ptr<Registration> registration; XNetworkingConnectivityHint hint; };
static void CALLBACK notification_callback(void *context,BOOLEAN canceled) {
    std::unique_ptr<CallbackJob> job(static_cast<CallbackJob*>(context));
    auto reg=job->registration;
    XNetworkingConnectivityHintChangedCallback *callback=nullptr;
    void *user_context=nullptr;
    {
        std::lock_guard<std::mutex> lock(reg->lock);
        if(canceled||!reg->active) return;
        ++reg->running;callback=reg->callback;user_context=reg->context;
    }
    InvocationFrame frame{reg.get(),current_invocation};
    current_invocation=&frame;
    callback(user_context,&job->hint);
    current_invocation=frame.previous;
    { std::lock_guard<std::mutex> lock(reg->lock);--reg->running; }
    reg->idle.notify_all();
}
static HRESULT submit_hint(const std::shared_ptr<Registration>&reg,const XNetworkingConnectivityHint &hint) {
    { std::lock_guard<std::mutex> lock(reg->lock);if(!reg->active) return S_FALSE; }
    auto job=new(std::nothrow) CallbackJob{reg,hint};
    if(!job) return E_OUTOFMEMORY;
    HRESULT hr=XTaskQueueSubmitCallback(reg->queue,XTaskQueuePort::Completion,job,notification_callback);
    if(FAILED(hr)) delete job;
    return hr;
}
struct Manager {
    std::mutex lock;
    std::map<UINT64,std::shared_ptr<Registration>> registrations;
    UINT64 next_token=1;
    PTP_TIMER timer=nullptr;
    HRESULT init_result=E_UNEXPECTED;
    bool stopped=true;
    bool stopping=false;
    std::condition_variable lifecycle_idle;
};
static void poll_registrations(Manager *manager) {
    XNetworkingConnectivityHint hint{};
    if(FAILED(read_hint(&hint))) return;
    std::vector<std::shared_ptr<Registration>> snapshot;
    {
        std::lock_guard<std::mutex> lock(manager->lock);
        for(auto it=manager->registrations.begin();it!=manager->registrations.end();) {
            auto reg=it->second;
            std::lock_guard<std::mutex> reglock(reg->lock);
            if(!reg->active&&reg->running==0) it=manager->registrations.erase(it);
            else {snapshot.push_back(reg);++it;}
        }
    }
    for(auto &reg:snapshot) {
        bool changed=false;
        { std::lock_guard<std::mutex> lock(reg->lock); if(reg->active&&reg->initial_enqueued&&!same_hint(reg->last,hint)) {reg->last=hint;changed=true;} }
        if(changed) submit_hint(reg,hint);
    }
}
static void CALLBACK timer_callback(PTP_CALLBACK_INSTANCE instance,void *context,PTP_TIMER) {
    current_timer_instance=instance;
    timer_disassociated=false;
    try { poll_registrations(static_cast<Manager*>(context)); }
    catch(const std::bad_alloc&) { /* A later timer tick retries without losing registrations. */ }
    current_timer_instance=nullptr;
}
static Manager& manager() {
    // Process lifetime and a pinned module are intentional: a host may forget to
    // unregister; no timer may outlive its DLL code or run through static teardown.
    static Manager *instance=[] {
        auto m=new Manager;
        HMODULE module=nullptr;
        if(!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_PIN,
            reinterpret_cast<LPCWSTR>(&timer_callback),&module)) {m->init_result=HRESULT_FROM_WIN32(GetLastError());return m;}
        m->timer=CreateThreadpoolTimer(timer_callback,m,nullptr);
        if(!m->timer) {m->init_result=HRESULT_FROM_WIN32(GetLastError());return m;}
        m->init_result=S_OK;return m;
    }();
    return *instance;
}
#ifdef NETWORKING_TESTING
void NetworkTestPoll() { poll_registrations(&manager()); }
void NetworkTestArmTimer() {
    LARGE_INTEGER due;due.QuadPart=-1000000LL;
    FILETIME time{due.LowPart,static_cast<DWORD>(due.HighPart)};
    SetThreadpoolTimer(manager().timer,&time,0,0);
}
#endif
static HRESULT register_hint(XTaskQueueHandle queue,void *context,XNetworkingConnectivityHintChangedCallback *callback,XTaskQueueRegistrationToken *token) {
    if(!callback||!token) return E_POINTER;
    token->token=0;
    try {
        XNetworkingConnectivityHint hint{};
        HRESULT hr=read_hint(&hint);if(FAILED(hr)) return hr;
        auto &mgr=manager();if(FAILED(mgr.init_result)) return mgr.init_result;
        auto reg=std::make_shared<Registration>();reg->context=context;reg->callback=callback;reg->last=hint;
        if(queue) hr=XTaskQueueDuplicateHandle(queue,&reg->queue);
        else hr=XTaskQueueGetCurrentProcessTaskQueue(&reg->queue)?S_OK:HRESULT_FROM_WIN32(ERROR_NO_TASK_QUEUE);
        if(FAILED(hr)) return hr;
        {
            std::lock_guard<std::mutex> lock(mgr.lock);
            if(mgr.stopping) return E_ABORT;
            if(mgr.stopped) {
#ifndef NETWORKING_TESTING
                LARGE_INTEGER due;due.QuadPart=-10000000LL;
                FILETIME time{due.LowPart,static_cast<DWORD>(due.HighPart)};
                SetThreadpoolTimer(mgr.timer,&time,1000,100);
#endif
                mgr.stopped=false;
            }
            token->token=mgr.next_token++;
            mgr.registrations.emplace(token->token,reg);
        }
        hr=submit_hint(reg,hint);
        {std::lock_guard<std::mutex> lock(reg->lock);reg->initial_enqueued=SUCCEEDED(hr);}
        if(FAILED(hr)) {
            {std::lock_guard<std::mutex> lock(reg->lock);reg->active=false;}
            {std::lock_guard<std::mutex> lock(mgr.lock);mgr.registrations.erase(token->token);}
            token->token=0;
        }
        std::fprintf(stderr,"xodus-network: register token=%llu result=%08lx\n",token->token,static_cast<ULONG>(hr));
        return hr;
    } catch(const std::bad_alloc&) {token->token=0;return E_OUTOFMEMORY;}
}
static BOOLEAN unregister_hint(XTaskQueueRegistrationToken token,BOOLEAN wait) {
    auto &mgr=manager();
    std::shared_ptr<Registration> reg;
    {std::lock_guard<std::mutex> lock(mgr.lock);auto it=mgr.registrations.find(token.token);if(it==mgr.registrations.end()) return TRUE;reg=it->second;}
    bool idle;
    {std::unique_lock<std::mutex> lock(reg->lock);reg->active=false;if(wait)reg->idle.wait(lock,[&]{return reg->running==0;});idle=reg->running==0;}
    if(idle) {std::lock_guard<std::mutex> lock(mgr.lock);mgr.registrations.erase(token.token);}
    return idle;
}

void NetworkRuntimeShutdown() {
    NetworkSecurityShutdown();
    auto &mgr=manager();
    std::map<UINT64,std::shared_ptr<Registration>> retired;
    {
        std::unique_lock<std::mutex> lock(mgr.lock);
        if(mgr.stopping) {
            // The other shutdown may be waiting for this very callback.
            if(current_invocation||current_timer_instance) return;
            mgr.lifecycle_idle.wait(lock,[&]{return !mgr.stopping;});
            return;
        }
        if(mgr.stopped) return;
        mgr.stopping=true;
        retired.swap(mgr.registrations);
        for(auto &item:retired) {
            std::lock_guard<std::mutex> reglock(item.second->lock);
            item.second->active=false;
        }
        if(mgr.timer) SetThreadpoolTimer(mgr.timer,nullptr,0,0);
    }
    if(mgr.timer) {
        // An Immediate completion callback may be called by this timer itself.
        // Exclude that current frame from the join; its captured registrations
        // remain alive and inactive until its stack unwinds.
        if(current_timer_instance&&!timer_disassociated) {
            DisassociateCurrentThreadFromCallback(current_timer_instance);
            timer_disassociated=true;
        }
        WaitForThreadpoolTimerCallbacks(mgr.timer,TRUE);
    }
    for(auto &item:retired) {
        auto &reg=item.second;
        unsigned self_depth=0;
        for(auto frame=current_invocation;frame;frame=frame->previous)
            if(frame->registration==reg.get()) ++self_depth;
        std::unique_lock<std::mutex> lock(reg->lock);
        reg->idle.wait(lock,[&]{return reg->running<=self_depth;});
    }
    {
        std::lock_guard<std::mutex> lock(mgr.lock);
        mgr.stopped=true;
        mgr.stopping=false;
    }
    mgr.lifecycle_idle.notify_all();
}

class NetworkingImpl final : public IXNetworkingImpl2 {
    std::atomic<ULONG> ref{1};
    std::once_flag base_once;
    IXNetworkingImpl *original=nullptr;
    HRESULT original_result=E_UNEXPECTED;
    HRESULT base() { std::call_once(base_once,[&]{const GUID id=__uuidof(IXNetworkingImpl);original_result=QueryOriginalApi(&id,id,reinterpret_cast<void**>(&original));});return original_result; }
public:
    HRESULT WINAPI QueryInterface(REFIID iid,void **out) override {
        HRESULT hr=E_NOINTERFACE;
        if(!out) hr=E_POINTER;
        else {
            *out=nullptr;
            if(iid==__uuidof(IUnknown)||iid==__uuidof(IAgileObject)||iid==__uuidof(IXNetworkingImpl)||iid==__uuidof(IXNetworkingImpl2)) {
                *out=static_cast<IXNetworkingImpl2*>(this);AddRef();hr=S_OK;
            }
        }
        std::fprintf(stderr,"xodus-network: QueryInterface iid=%08lx-%04x-%04x-%02x%02x-%02x%02x%02x%02x%02x%02x result=%08lx\n",
            iid.Data1,iid.Data2,iid.Data3,iid.Data4[0],iid.Data4[1],iid.Data4[2],iid.Data4[3],
            iid.Data4[4],iid.Data4[5],iid.Data4[6],iid.Data4[7],static_cast<ULONG>(hr));
        return hr;
    }
    ULONG WINAPI AddRef() override {return ++ref;}
    ULONG WINAPI Release() override {return --ref;}
    HRESULT WINAPI XNetworkingQueryPreferredLocalUdpMultiplayerPort(UINT16 *preferredLocalUdpMultiplayerPort) override {HRESULT hr=base();if(FAILED(hr))return hr;return original->XNetworkingQueryPreferredLocalUdpMultiplayerPort(preferredLocalUdpMultiplayerPort);}
    HRESULT WINAPI XNetworkingQueryPreferredLocalUdpMultiplayerPortAsync(XAsyncBlock *asyncBlock) override {HRESULT hr=base();if(FAILED(hr))return hr;return original->XNetworkingQueryPreferredLocalUdpMultiplayerPortAsync(asyncBlock);}
    HRESULT WINAPI XNetworkingQueryPreferredLocalUdpMultiplayerPortAsyncResult(XAsyncBlock *asyncBlock, UINT16 *preferredLocalUdpMultiplayerPort) override {HRESULT hr=base();if(FAILED(hr))return hr;return original->XNetworkingQueryPreferredLocalUdpMultiplayerPortAsyncResult(asyncBlock,preferredLocalUdpMultiplayerPort);}
    HRESULT WINAPI XNetworkingRegisterPreferredLocalUdpMultiplayerPortChanged(XTaskQueueHandle queue, PVOID context, XNetworkingPreferredLocalUdpMultiplayerPortChangedCallback *callback, XTaskQueueRegistrationToken *token) override {HRESULT hr=base();if(FAILED(hr))return hr;return original->XNetworkingRegisterPreferredLocalUdpMultiplayerPortChanged(queue,context,callback,token);}
    BOOLEAN WINAPI XNetworkingUnregisterPreferredLocalUdpMultiplayerPortChanged(XTaskQueueRegistrationToken token, BOOLEAN wait) override {HRESULT hr=base();if(FAILED(hr))return FALSE;return original->XNetworkingUnregisterPreferredLocalUdpMultiplayerPortChanged(token,wait);}
    HRESULT WINAPI XNetworkingQuerySecurityInformationForUrlAsync(LPCSTR url, XAsyncBlock *asyncBlock) override {return NetworkSecurityQueryUtf8(url,asyncBlock);}
    HRESULT WINAPI XNetworkingQuerySecurityInformationForUrlAsyncResultSize(XAsyncBlock *asyncBlock, SIZE_T *securityInformationBufferByteCount) override {return NetworkSecurityResultSize(asyncBlock,securityInformationBufferByteCount);}
    HRESULT WINAPI XNetworkingQuerySecurityInformationForUrlAsyncResult(XAsyncBlock *asyncBlock, SIZE_T securityInformationBufferByteCount, SIZE_T *securityInformationBufferByteCountUsed, UINT8 *securityInformationBuffer, XNetworkingSecurityInformation **securityInformation) override {return NetworkSecurityResult(asyncBlock,securityInformationBufferByteCount,securityInformationBufferByteCountUsed,securityInformationBuffer,securityInformation);}
    HRESULT WINAPI XNetworkingQuerySecurityInformationForUrlUtf16Async(LPCWSTR url, XAsyncBlock *asyncBlock) override {return NetworkSecurityQuery(url,asyncBlock);}
    HRESULT WINAPI XNetworkingQuerySecurityInformationForUrlUtf16AsyncResultSize(XAsyncBlock *asyncBlock, SIZE_T *securityInformationBufferByteCount) override {return NetworkSecurityResultSize(asyncBlock,securityInformationBufferByteCount);}
    HRESULT WINAPI XNetworkingQuerySecurityInformationForUrlUtf16AsyncResult(XAsyncBlock *asyncBlock, SIZE_T securityInformationBufferByteCount, SIZE_T *securityInformationBufferByteCountUsed, UINT8 *securityInformationBuffer, XNetworkingSecurityInformation **securityInformation) override {return NetworkSecurityResult(asyncBlock,securityInformationBufferByteCount,securityInformationBufferByteCountUsed,securityInformationBuffer,securityInformation);}
    HRESULT WINAPI XNetworkingVerifyServerCertificate(PVOID requestHandle, const XNetworkingSecurityInformation *securityInformation) override {return NetworkSecurityVerify(requestHandle,securityInformation);}
    HRESULT WINAPI XNetworkingGetConnectivityHint(XNetworkingConnectivityHint *hint) override {
        HRESULT hr=read_hint(hint);
        if(SUCCEEDED(hr))std::fprintf(stderr,"xodus-network: local status level=%u interface=%u initialized=%u cost=%u\n",(unsigned)hint->connectivityLevel,hint->ianaInterfaceType,hint->networkInitialized,(unsigned)hint->connectivityCost);
        return hr;
    }
    HRESULT WINAPI XNetworkingRegisterConnectivityHintChanged(XTaskQueueHandle queue,void *context,XNetworkingConnectivityHintChangedCallback *callback,XTaskQueueRegistrationToken *token) override {return register_hint(queue,context,callback,token);}
    BOOLEAN WINAPI XNetworkingUnregisterConnectivityHintChanged(XTaskQueueRegistrationToken token,BOOLEAN wait) override {return unregister_hint(token,wait);}
    // Microsoft documents all three settings as zero on PC and unsupported
    // for modification. These are Xbox queue-budget settings, not host TCP
    // socket buffer sizes. No Linux/Windows system setting is changed here.
    HRESULT WINAPI XNetworkingQueryConfigurationSetting(XNetworkingConfigurationSetting setting,UINT64 *value) override {
        HRESULT hr=E_INVALIDARG;
        if(!value) hr=E_POINTER;
        else switch(setting) {
            case XNetworkingConfigurationSetting::MaxTitleTcpQueuedReceiveBufferSize:
            case XNetworkingConfigurationSetting::MaxSystemTcpQueuedReceiveBufferSize:
            case XNetworkingConfigurationSetting::MaxToolsTcpQueuedReceiveBufferSize:
                *value=0;hr=S_OK;break;
            default:break;
        }
        std::fprintf(stderr,"xodus-network: QueryConfigurationSetting setting=%u result=%08lx\n",static_cast<unsigned>(setting),static_cast<ULONG>(hr));
        return hr;
    }
    HRESULT WINAPI XNetworkingSetConfigurationSetting(XNetworkingConfigurationSetting setting,UINT64 value) override {
        (void)value;
        HRESULT hr=E_INVALIDARG;
        switch(setting) {
            case XNetworkingConfigurationSetting::MaxTitleTcpQueuedReceiveBufferSize:
            case XNetworkingConfigurationSetting::MaxSystemTcpQueuedReceiveBufferSize:
            case XNetworkingConfigurationSetting::MaxToolsTcpQueuedReceiveBufferSize:
                hr=E_NOTIMPL;break;
            default:break;
        }
        std::fprintf(stderr,"xodus-network: SetConfigurationSetting setting=%u result=%08lx\n",static_cast<unsigned>(setting),static_cast<ULONG>(hr));
        return hr;
    }
    HRESULT WINAPI XNetworkingQueryStatistics(XNetworkingStatisticsType type,XNetworkingStatisticsBuffer *buffer) override {
        HRESULT hr=E_INVALIDARG;
        if(!buffer) hr=E_POINTER;
        else switch(type) {
            case XNetworkingStatisticsType::TitleTcpQueuedReceivedBufferUsage:
            case XNetworkingStatisticsType::SystemTcpQueuedReceivedBufferUsage:
            case XNetworkingStatisticsType::ToolsTcpQueuedReceivedBufferUsage:
                // The PC contract specifies all-zero statistics for these
                // Xbox queue counters; this is not measured network traffic.
                buffer->tcpQueuedReceiveBufferUsage={};hr=S_OK;break;
            default:break;
        }
        std::fprintf(stderr,"xodus-network: QueryStatistics type=%u result=%08lx\n",static_cast<unsigned>(type),static_cast<ULONG>(hr));
        return hr;
    }
};
static NetworkingImpl networking;
IXNetworkingImpl *x_networking_impl=&networking;

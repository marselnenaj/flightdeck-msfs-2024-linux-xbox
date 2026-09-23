// SPDX-License-Identifier: LGPL-2.1-or-later
// Synthetic provider only: no account, network or package mutations.
#include "StoreQueries.h"
#include "StoreContext.h"
#include <atomic>
#include <cstdio>
void XodusStoreLicenseEventsContextClosed(void*){}
void XodusStoreLicenseEventsShutdown(){}
static int checks,failures;
static std::atomic<int> accounts{0},calls{0};
static HRESULT answer=S_OK;
static bool blocking=false;
static HANDLE entered;
static XTaskQueueHandle queue;
static void check(const char *name,bool passed){++checks;failures+=!passed;printf("package-updates case=%s pass=%d\n",name,passed);}
static HRESULT WINAPI acquire(void*,void **out){*out=new int(1);++accounts;return S_OK;}
static void WINAPI release(void*,void *p){delete static_cast<int*>(p);--accounts;}
static HRESULT WINAPI updates(void*,void *account,volatile LONG *cancelled){
    ++calls;if(!account)return E_HANDLE;
    if(blocking){SetEvent(entered);for(int i=0;i<2000;++i){if(InterlockedCompareExchange(cancelled,0,0))return E_ABORT;Sleep(1);}return E_FAIL;}
    return answer;
}
static XAsyncBlock block(){XAsyncBlock a{};a.queue=queue;return a;}
static void dispatch(){XTaskQueueDispatch(queue,XTaskQueuePort::Work,0);XTaskQueueDispatch(queue,XTaskQueuePort::Completion,0);}
struct Retry {void *context;HRESULT first=E_PENDING,next=E_PENDING;};
static void CALLBACK retry(XAsyncBlock *a){auto r=static_cast<Retry*>(a->context);UINT32 count=99;r->first=XodusStoreQueryGameAndDlcPackageUpdatesResultCount(a,&count);a->callback=nullptr;r->next=XodusStoreQueryGameAndDlcPackageUpdatesAsync(r->context,a);}
int main(){
    XodusStoreAccountProvider binding{nullptr,acquire,release};binding.check_package_updates=updates;
    check("queue",XTaskQueueCreate(XTaskQueueDispatchMode::Manual,XTaskQueueDispatchMode::Manual,&queue)==S_OK);
    void *context=nullptr;check("context",XodusStoreContextCreate(&binding,nullptr,&context)==S_OK);
    UINT32 count=99;XAsyncBlock a=block();
    check("null-async",XodusStoreQueryGameAndDlcPackageUpdatesAsync(context,nullptr)==E_POINTER);
    check("invalid-context",XodusStoreQueryGameAndDlcPackageUpdatesAsync(nullptr,&a)==E_HANDLE);
    check("null-count",XodusStoreQueryGameAndDlcPackageUpdatesResultCount(&a,nullptr)==E_POINTER);
    check("begin",XodusStoreQueryGameAndDlcPackageUpdatesAsync(context,&a)==S_OK);
    check("pending-is-not-empty",XodusStoreQueryGameAndDlcPackageUpdatesResultCount(&a,&count)==E_PENDING&&count==0);
    dispatch();count=99;
    check("authenticated-current",XodusStoreQueryGameAndDlcPackageUpdatesResultCount(&a,&count)==S_OK&&count==0);
    check("empty-size",[&]{SIZE_T size=99;return XAsyncGetResultSize(&a,&size)==S_OK&&size==0;}());
    check("optional-empty-result",XodusStoreQueryGameAndDlcPackageUpdatesResult(&a,0,nullptr)==S_OK);
    check("nonempty-result-rejected",XodusStoreQueryGameAndDlcPackageUpdatesResult(&a,1,nullptr)==E_INVALIDARG);
    // The SDK sample deliberately omits Result() when Count is zero. Release
    // the context here to prove that no hidden result allocation retains it.
    XodusStoreContextClose(context);check("count-only-releases-account",accounts==0);
    XodusStoreContextCreate(&binding,nullptr,&context);
    for(HRESULT status:{E_FAIL,E_ACCESSDENIED,E_NOTIMPL,HRESULT_FROM_WIN32(ERROR_TIMEOUT),HRESULT_FROM_WIN32(ERROR_REVISION_MISMATCH),S_FALSE}) {
        answer=status;a=block();count=99;
        check("failure-begin",XodusStoreQueryGameAndDlcPackageUpdatesAsync(context,&a)==S_OK);dispatch();
        HRESULT expected=status==S_FALSE?HRESULT_FROM_WIN32(ERROR_INVALID_DATA):status;
        check("failure-never-empty-success",XodusStoreQueryGameAndDlcPackageUpdatesResultCount(&a,&count)==expected&&count==0);
        check("failure-result",XodusStoreQueryGameAndDlcPackageUpdatesResult(&a,0,nullptr)==expected);
    }
    answer=S_OK;a=block();int before=calls;XodusStoreQueryGameAndDlcPackageUpdatesAsync(context,&a);XAsyncCancel(&a);dispatch();
    check("cancel-before-query",XodusStoreQueryGameAndDlcPackageUpdatesResultCount(&a,&count)==E_ABORT&&calls==before);
    a=block();XodusStoreQueryGameAndDlcPackageUpdatesAsync(context,&a);XodusStoreContextClose(context);dispatch();
    check("context-close-before-work",XodusStoreQueryGameAndDlcPackageUpdatesResultCount(&a,&count)==E_ABORT&&accounts==0);
    XodusStoreContextCreate(&binding,nullptr,&context);
    XTaskQueueHandle immediate=nullptr;XTaskQueueCreate(XTaskQueueDispatchMode::Manual,XTaskQueueDispatchMode::Immediate,&immediate);
    a={};a.queue=immediate;Retry r{context};a.context=&r;a.callback=retry;
    XodusStoreQueryGameAndDlcPackageUpdatesAsync(context,&a);XTaskQueueDispatch(immediate,XTaskQueuePort::Work,0);
    check("completion-reuses-empty-block",r.first==S_OK&&r.next==S_OK);
    XTaskQueueDispatch(immediate,XTaskQueuePort::Work,0);check("second-empty-result",XodusStoreQueryGameAndDlcPackageUpdatesResultCount(&a,&count)==S_OK);
    XTaskQueueCloseHandle(immediate);XodusStoreContextClose(context);
    binding.check_package_updates=nullptr;XodusStoreContextCreate(&binding,nullptr,&context);a=block();XodusStoreQueryGameAndDlcPackageUpdatesAsync(context,&a);dispatch();
    check("unknown-scope-not-empty",XodusStoreQueryGameAndDlcPackageUpdatesResultCount(&a,&count)==E_NOTIMPL);XodusStoreContextClose(context);
    binding.check_package_updates=updates;XTaskQueueHandle pool=nullptr;XTaskQueueCreate(XTaskQueueDispatchMode::ThreadPool,XTaskQueueDispatchMode::ThreadPool,&pool);
    entered=CreateEventW(nullptr,TRUE,FALSE,nullptr);blocking=true;
    for(bool close:{false,true}) {
        XodusStoreContextCreate(&binding,nullptr,&context);a={};a.queue=pool;ResetEvent(entered);
        check("inflight-query",XodusStoreQueryGameAndDlcPackageUpdatesAsync(context,&a)==S_OK&&WaitForSingleObject(entered,2000)==WAIT_OBJECT_0);
        if(close)XodusStoreContextClose(context);else XAsyncCancel(&a);
        check("inflight-cancel",XAsyncGetStatus(&a,TRUE)==E_ABORT&&XodusStoreQueryGameAndDlcPackageUpdatesResultCount(&a,&count)==E_ABORT);
        if(!close)XodusStoreContextClose(context);
    }
    blocking=false;CloseHandle(entered);XTaskQueueCloseHandle(pool);
    XodusStoreContextCreate(&binding,nullptr,&context);a=block();XodusStoreQueryGameAndDlcPackageUpdatesAsync(context,&a);
    XodusStoreQueriesShutdown();XodusStoreContextShutdown();dispatch();check("shutdown",XodusStoreQueryGameAndDlcPackageUpdatesResultCount(&a,&count)==E_ABORT);
    for(int i=0;i<1000&&accounts;++i)Sleep(1);
    check("all-released",accounts==0);XTaskQueueCloseHandle(queue);
    printf("package-updates checks=%d failures=%d external_requests=0\n",checks,failures);return failures?1:0;
}

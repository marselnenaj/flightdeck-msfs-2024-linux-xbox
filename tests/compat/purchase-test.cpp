// SPDX-License-Identifier: LGPL-2.1-or-later
// Synthetic checkout only. No UI, credentials, network or payments.
#include "StoreQueries.h"
#include "StoreContext.h"
#include <atomic>
#include <cstdio>
#include <string>
void XodusStoreLicenseEventsContextClosed(void*){}
void XodusStoreLicenseEventsShutdown(){}
static int checks,failures;
static std::atomic<int> accounts{0},calls{0};
static HRESULT answer=S_OK;
static std::string observed_id,observed_name,observed_json;
static XTaskQueueHandle queue;
static void check(const char *name,bool ok){++checks;failures+=!ok;printf("purchase case=%s pass=%d\n",name,ok);}
static HRESULT WINAPI acquire(void*,void **out){*out=new int(1);++accounts;return S_OK;}
static void WINAPI release(void*,void *p){delete static_cast<int*>(p);--accounts;}
static HRESULT WINAPI purchase(void*,void *account,const char *id,const char *name,const char *json,volatile LONG *cancelled){
    ++calls;if(!account)return E_HANDLE;
    observed_id=id;observed_name=name;observed_json=json;
    return InterlockedCompareExchange(cancelled,0,0)?E_ABORT:answer;
}
static XAsyncBlock block(){XAsyncBlock a{};a.queue=queue;return a;}
static void dispatch(){XTaskQueueDispatch(queue,XTaskQueuePort::Work,0);XTaskQueueDispatch(queue,XTaskQueuePort::Completion,0);}
int main(){
    XodusStoreAccountProvider binding{nullptr,acquire,release};binding.show_purchase=purchase;
    check("queue",XTaskQueueCreate(XTaskQueueDispatchMode::Manual,XTaskQueueDispatchMode::Manual,&queue)==S_OK);
    void *context=nullptr;check("context",XodusStoreContextCreate(&binding,nullptr,&context)==S_OK);
    XAsyncBlock a=block();
    check("null-id",XodusStoreShowPurchaseUIAsync(context,nullptr,nullptr,nullptr,&a)==E_POINTER);
    check("null-async",XodusStoreShowPurchaseUIAsync(context,"ABCD1234EFGH",nullptr,nullptr,nullptr)==E_POINTER);
    check("bad-id",XodusStoreShowPurchaseUIAsync(context,"https://example.com",nullptr,nullptr,&a)==E_INVALIDARG);
    check("invalid-context",XodusStoreShowPurchaseUIAsync(nullptr,"ABCD1234EFGH",nullptr,nullptr,&a)==E_HANDLE);
    check("null-result",XodusStoreShowPurchaseUIResult(nullptr)==E_POINTER);
    std::string name="Example",custom="{\"campaignId\":\"test\"}",id="ABCD1234EFGH/0001";
    check("begin",XodusStoreShowPurchaseUIAsync(context,id.c_str(),name.c_str(),custom.c_str(),&a)==S_OK);
    check("pending-is-not-success",XodusStoreShowPurchaseUIResult(&a)==E_PENDING);
    check("same-block-no-double-checkout",XodusStoreShowPurchaseUIAsync(context,id.c_str(),nullptr,nullptr,&a)==E_INVALIDARG);
    name="changed";custom.clear();id="DIFF1234EFGH";dispatch();
    check("copied-request",observed_id=="ABCD1234EFGH/0001"&&observed_name=="Example"&&observed_json=="{\"campaignId\":\"test\"}");
    check("confirmed-success",XodusStoreShowPurchaseUIResult(&a)==S_OK&&calls==1);
    for(HRESULT status:{E_ABORT,E_FAIL,E_ACCESSDENIED,HRESULT_FROM_WIN32(ERROR_TIMEOUT),HRESULT_FROM_WIN32(ERROR_BUSY),S_FALSE}) {
        answer=status;a=block();check("failure-begin",XodusStoreShowPurchaseUIAsync(context,"ABCD1234EFGH",nullptr,nullptr,&a)==S_OK);dispatch();
        check("failure-not-paid",XodusStoreShowPurchaseUIResult(&a)==(status==S_FALSE?HRESULT_FROM_WIN32(ERROR_INVALID_DATA):status));
    }
    answer=S_OK;a=block();int before=calls;
    XodusStoreShowPurchaseUIAsync(context,"ABCD1234EFGH",nullptr,nullptr,&a);XAsyncCancel(&a);dispatch();
    check("cancel-before-ui",XodusStoreShowPurchaseUIResult(&a)==E_ABORT&&calls==before);
    a=block();XodusStoreShowPurchaseUIAsync(context,"ABCD1234EFGH",nullptr,nullptr,&a);XodusStoreContextClose(context);dispatch();
    check("context-close-before-ui",XodusStoreShowPurchaseUIResult(&a)==E_ABORT&&calls==before&&accounts==0);
    binding.show_purchase=nullptr;XodusStoreContextCreate(&binding,nullptr,&context);a=block();
    XodusStoreShowPurchaseUIAsync(context,"ABCD1234EFGH",nullptr,nullptr,&a);dispatch();
    check("missing-provider-not-success",XodusStoreShowPurchaseUIResult(&a)==E_NOTIMPL);
    XodusStoreContextClose(context);XodusStoreQueriesShutdown();XodusStoreContextShutdown();
    check("all-released",accounts==0);XTaskQueueCloseHandle(queue);
    printf("purchase checks=%d failures=%d external_requests=0\n",checks,failures);return failures?1:0;
}

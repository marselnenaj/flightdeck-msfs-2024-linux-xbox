// SPDX-License-Identifier: LGPL-2.1-or-later
// Synthetic signed-observation boundary; no network, receipts or real accounts.
#include "StoreQueries.h"
#include "StoreContext.h"
#include "StoreDurableLicense.h"
#include <atomic>
#include <cstdio>
#include <cstring>
#include <thread>

void XodusStoreLicenseEventsContextClosed(void*){}
void XodusStoreLicenseEventsShutdown(){}
static unsigned checks=0,failures=0;
static std::atomic<unsigned> accounts{0},reads{0},notifications{0};
static std::atomic<bool> blocked{false};
static HANDLE entered;
static HRESULT failure=S_OK;
static unsigned malformed=0;
static unsigned lifetime=60;
static UINT64 offset=0;
static XTaskQueueHandle queue;
static void check(const char *name,bool ok){++checks;failures+=!ok;printf("durable-license case=%s pass=%d\n",name,ok);}
static INT64 now(){FILETIME f;GetSystemTimeAsFileTime(&f);return static_cast<INT64>(((UINT64(f.dwHighDateTime)<<32)|f.dwLowDateTime)/10000000)-11644473600LL+static_cast<INT64>(offset/1000);}
static void advance(UINT64 n){offset+=n;XodusStoreDurableTestAdvance(n);}
static HRESULT WINAPI acquire(void*,void **out){*out=new int(7);++accounts;return S_OK;}
static void WINAPI release(void*,void *p){delete static_cast<int*>(p);--accounts;}
static HRESULT WINAPI observe(void*,void *account,const char *id,volatile LONG *cancel,XStoreGameLicense *out){
    ++reads;if(!account||strcmp(id,"OTHER1234567"))return E_INVALIDARG;
    if(blocked){SetEvent(entered);for(unsigned i=0;i<4000;++i){if(InterlockedCompareExchange(cancel,0,0))return E_ABORT;Sleep(1);}return E_FAIL;}
    if(FAILED(failure))return failure;
    *out={};strcpy(out->skuStoreId,malformed==1?"FOREIGN12345/0001":"OTHER1234567/0001");
    out->isActive=TRUE;out->expirationDate=now()+(malformed==2?61:malformed==3?0:lifetime);
    if(malformed==4)out->isTrial=TRUE;
    return S_OK;
}
static void dispatch(){while(XTaskQueueDispatch(queue,XTaskQueuePort::Work,0)){}while(XTaskQueueDispatch(queue,XTaskQueuePort::Completion,0)){} }
static void CALLBACK lost(void*){++notifications;}
static void *context(const XodusStoreAccountProvider &p){void *c=nullptr;check("context",XodusStoreContextCreate(&p,nullptr,&c)==S_OK);return c;}
static XStoreLicenseHandle get(void *c){
    XAsyncBlock a{};a.queue=queue;check("acquire-begin",XodusStoreAcquireLicenseForDurablesAsync(c,"OTHER1234567",&a)==S_OK);dispatch();
    XStoreLicenseHandle h=nullptr;check("acquire-result",XodusStoreAcquireLicenseForDurablesResult(&a,&h)==S_OK&&h);return h;
}
struct Reentrant {XStoreLicenseHandle h;XTaskQueueRegistrationToken token;};
static void CALLBACK close_self(void *value){auto p=static_cast<Reentrant*>(value);++notifications;check("self-unregister-pending",!XodusStoreUnregisterPackageLicenseLost(p->h,p->token,TRUE));XodusStoreCloseLicenseHandle(p->h);}
struct Waiting {HANDLE entered,leave;};
static void CALLBACK wait_callback(void *value){auto p=static_cast<Waiting*>(value);SetEvent(p->entered);WaitForSingleObject(p->leave,4000);++notifications;}

int main(){
    check("queue",XTaskQueueCreate(XTaskQueueDispatchMode::Manual,XTaskQueueDispatchMode::Manual,&queue)==S_OK);
    entered=CreateEventW(nullptr,TRUE,FALSE,nullptr);
    XodusStoreAccountProvider p{nullptr,acquire,release};p.query_durable_license=observe;
    void *c=context(p);XAsyncBlock invalid{};invalid.queue=queue;
    check("bad-product",XodusStoreAcquireLicenseForDurablesAsync(c,"../invalid",&invalid)==E_INVALIDARG);
    check("null-output",XodusStoreAcquireLicenseForDurablesResult(&invalid,nullptr)==E_POINTER);
    check("unknown-handle",!XodusStoreIsLicenseValid(reinterpret_cast<void*>(0xabcdef)));
    auto h=get(c);check("valid-positive",XodusStoreIsLicenseValid(h)&&XodusStoreDurableTestCount()==1);
    XTaskQueueRegistrationToken token{};check("register",XodusStoreRegisterPackageLicenseLost(h,queue,nullptr,lost,&token)==S_OK&&token.token);
    XodusStoreDurableTestPoll();dispatch();check("no-initial-notification",notifications==0);
    auto before=reads.load();advance(21000);XodusStoreDurableTestPoll();dispatch();check("renew-positive-proof",reads==before+1&&XodusStoreIsLicenseValid(h)&&notifications==0);
    failure=E_FAIL;advance(21000);XodusStoreDurableTestPoll();check("transient-error-keeps-current-proof",XodusStoreIsLicenseValid(h));
    advance(40000);check("stale-proof-expires",!XodusStoreIsLicenseValid(h));XodusStoreDurableTestPoll();dispatch();check("loss-notified",notifications==1);
    failure=S_OK;XodusStoreDurableTestPoll();dispatch();check("lost-handle-never-revives",!XodusStoreIsLicenseValid(h)&&notifications==1);
    check("foreign-handle-unregister",!XodusStoreUnregisterPackageLicenseLost(reinterpret_cast<void*>(0xffffff),token,TRUE));
    check("unregister",XodusStoreUnregisterPackageLicenseLost(h,token,TRUE));XodusStoreCloseLicenseHandle(h);XodusStoreCloseLicenseHandle(h);
    check("close-removes-handle",!XodusStoreIsLicenseValid(h)&&XodusStoreDurableTestCount()==0);
    for(malformed=1;malformed<=4;++malformed){
        XAsyncBlock a{};a.queue=queue;XodusStoreAcquireLicenseForDurablesAsync(c,"OTHER1234567",&a);dispatch();h=reinterpret_cast<void*>(1);
        check("invalid-proof-rejected",FAILED(XodusStoreAcquireLicenseForDurablesResult(&a,&h))&&!h&&XodusStoreDurableTestCount()==0);
    }
    malformed=0;failure=E_ACCESSDENIED;
    {XAsyncBlock a{};a.queue=queue;XodusStoreAcquireLicenseForDurablesAsync(c,"OTHER1234567",&a);dispatch();h=nullptr;check("denied-is-not-license",XodusStoreAcquireLicenseForDurablesResult(&a,&h)==E_ACCESSDENIED&&!h);}
    failure=S_OK;
    lifetime=6;h=get(c);before=reads;advance(3000);XodusStoreDurableTestPoll();advance(5000);
    check("short-signed-proof-renews-before-expiry",reads==before+1&&XodusStoreIsLicenseValid(h));XodusStoreCloseLicenseHandle(h);lifetime=60;
    {XAsyncBlock a{};a.queue=queue;XodusStoreAcquireLicenseForDurablesAsync(c,"OTHER1234567",&a);dispatch();advance(61000);h=nullptr;check("delayed-result-expired",FAILED(XodusStoreAcquireLicenseForDurablesResult(&a,&h))&&!h);dispatch();check("unclaimed-expired-result-released",XodusStoreDurableTestCount()==0);}
    h=get(c);XodusStoreRegisterPackageLicenseLost(h,queue,nullptr,lost,&token);advance(61000);XodusStoreDurableTestPoll();
    check("unregister-suppresses-queued",XodusStoreUnregisterPackageLicenseLost(h,token,TRUE));dispatch();check("queued-no-callback",notifications==1);XodusStoreCloseLicenseHandle(h);
    h=get(c);Reentrant reentrant{h,{}};XodusStoreRegisterPackageLicenseLost(h,queue,&reentrant,close_self,&reentrant.token);
    advance(61000);XodusStoreDurableTestPoll();dispatch();check("self-close-no-leak",notifications==2&&XodusStoreDurableTestCount()==0);
    h=get(c);Waiting waiting{CreateEventW(nullptr,TRUE,FALSE,nullptr),CreateEventW(nullptr,TRUE,FALSE,nullptr)};
    XodusStoreRegisterPackageLicenseLost(h,queue,&waiting,wait_callback,&token);advance(61000);XodusStoreDurableTestPoll();
    std::thread callback([]{XTaskQueueDispatch(queue,XTaskQueuePort::Completion,0);});
    check("callback-started",WaitForSingleObject(waiting.entered,4000)==WAIT_OBJECT_0);
    check("unregister-nonwait-pending",!XodusStoreUnregisterPackageLicenseLost(h,token,FALSE));
    std::atomic<bool> waited{false};std::thread unregister([&]{check("unregister-wait-drains",XodusStoreUnregisterPackageLicenseLost(h,token,TRUE));waited=true;});
    Sleep(20);check("wait-does-not-return-early",!waited);SetEvent(waiting.leave);callback.join();unregister.join();
    XodusStoreCloseLicenseHandle(h);CloseHandle(waiting.entered);CloseHandle(waiting.leave);
    h=get(c);XodusStoreRegisterPackageLicenseLost(h,queue,nullptr,lost,&token);XodusStoreContextClose(c);
    check("context-close-invalidates",!XodusStoreIsLicenseValid(h));XodusStoreDurableTestPoll();dispatch();check("context-loss-notified",notifications==4);XodusStoreCloseLicenseHandle(h);
    c=context(p);
    h=get(c);blocked=true;ResetEvent(entered);advance(21000);
    {std::thread renewal([]{XodusStoreDurableTestPoll();});check("renewal-entered",WaitForSingleObject(entered,4000)==WAIT_OBJECT_0);
     XodusStoreContextClose(c);renewal.join();blocked=false;check("context-close-cancels-renewal",!XodusStoreIsLicenseValid(h));XodusStoreCloseLicenseHandle(h);}
    c=context(p);
    {XTaskQueueHandle pool=nullptr;XTaskQueueCreate(XTaskQueueDispatchMode::ThreadPool,XTaskQueueDispatchMode::ThreadPool,&pool);
     XAsyncBlock a{};a.queue=pool;blocked=true;ResetEvent(entered);XodusStoreAcquireLicenseForDurablesAsync(c,"OTHER1234567",&a);
     check("pending-entered",WaitForSingleObject(entered,4000)==WAIT_OBJECT_0);XAsyncCancel(&a);check("pending-cancel",XAsyncGetStatus(&a,TRUE)==E_ABORT);blocked=false;
     h=nullptr;check("cancel-no-handle",XodusStoreAcquireLicenseForDurablesResult(&a,&h)==E_ABORT&&!h);XTaskQueueCloseHandle(pool);}
    h=get(c);auto old=h;XodusStoreCloseLicenseHandle(h);h=get(c);check("closed-handle-not-reused",h!=old&&!XodusStoreIsLicenseValid(old));
    XodusStoreRegisterPackageLicenseLost(h,queue,nullptr,lost,&token);advance(61000);XodusStoreDurableTestPoll();
    XodusStoreQueriesShutdown();XodusStoreContextShutdown();dispatch();check("shutdown-suppresses-queued",notifications==4&&XodusStoreDurableTestCount()==0);
    XTaskQueueCloseHandle(queue);CloseHandle(entered);
    for(unsigned i=0;i<100&&accounts;++i)Sleep(10);
    check("accounts-released",accounts==0);printf("SUMMARY checks=%u failures=%u\n",checks,failures);return failures?1:0;
}

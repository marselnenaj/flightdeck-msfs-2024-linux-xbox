// SPDX-License-Identifier: LGPL-2.1-or-later
// Synthetic provider only: no catalog, token, account, or purchase requests.
#include "StoreQueries.h"
#include "StoreContext.h"
#include <atomic>
#include <cstdio>
#include <cstring>

void XodusStoreLicenseEventsContextClosed(void*){}
void XodusStoreLicenseEventsShutdown(){}
static int checks,failures;
static std::atomic<int> accounts{0},pages{0},calls{0};
static XTaskQueueHandle queue;
static HANDLE entered;
static std::atomic<bool> blocking{false};
enum Mode {Success,Denied,Malformed,Unsupported};
static Mode mode=Success;
static void check(const char *name,bool ok){++checks;failures+=!ok;std::printf("explicit-products case=%s pass=%d\n",name,ok);}
static HRESULT WINAPI acquire(void*,void **out){*out=new int(42);++accounts;return S_OK;}
static void WINAPI release(void*,void *p){delete static_cast<int*>(p);--accounts;}
struct Fixture {XodusStoreProductPage page{};XStoreProduct product{};XStoreSku sku{};};
static void WINAPI release_page(void*,XodusStoreProductPage *p){delete reinterpret_cast<Fixture*>(p);--pages;}
static HRESULT WINAPI query(void*,void *account,UINT32 kinds,const char *const *ids,SIZE_T count,
    const char *const *actions,SIZE_T action_count,const char *cursor,volatile LONG *cancel,XodusStoreProductPage **out) {
    ++calls;*out=nullptr;
    if(!account||kinds!=31||count!=2||action_count!=1||strcmp(ids[0],"ABCD1234EFGH")||strcmp(ids[1],"OTHER1234567/0001")||strcmp(actions[0],"Purchase"))return E_INVALIDARG;
    if(blocking){SetEvent(entered);for(unsigned i=0;i<2000;++i){if(InterlockedCompareExchange(cancel,0,0))return E_ABORT;Sleep(1);}return E_FAIL;}
    if(mode==Unsupported)return E_NOTIMPL;
    auto value=new Fixture{};++pages;
    value->page.structure_size=sizeof(value->page);value->page.product_count=mode==Malformed?3:1;
    value->page.products=&value->product;value->page.continuation=cursor?nullptr:"synthetic-page-two";
    value->product.storeId=cursor?"OTHER1234567":"ABCD1234EFGH";value->product.title="Synthetic product";
    value->product.productKind=XStoreProductKind::Durable;
    value->product.isInUserCollection=TRUE; // Explicit synthetic collection evidence.
    value->product.price.basePrice=12.50f;value->product.price.price=10.25f;
    value->product.price.currencyCode="EUR";strcpy(value->product.price.formattedPrice,"10.25 EUR");
    value->product.skusCount=1;value->product.skus=&value->sku;
    value->sku.skuId="0001";value->sku.isInUserCollection=TRUE;
    *out=&value->page;return mode==Denied?E_ACCESSDENIED:S_OK;
}
static XodusStoreAccountProvider binding{nullptr,acquire,release,nullptr,nullptr,release_page,nullptr,nullptr,query};
static const char *ids[]={"ABCD1234EFGH","OTHER1234567/0001"};
static const char *actions[]={"Purchase"};
static XAsyncBlock block(){XAsyncBlock a{};a.queue=queue;return a;}
static HRESULT start(void *context,XAsyncBlock *a){return XodusStoreQueryProductsAsync(context,static_cast<XStoreProductKind>(31),ids,2,actions,1,a);}
static void dispatch(){XTaskQueueDispatch(queue,XTaskQueuePort::Work,0);XTaskQueueDispatch(queue,XTaskQueuePort::Completion,0);}
static BOOLEAN CALLBACK inspect(const XStoreProduct *product,void *context){
    ++*static_cast<int*>(context);
    check("owned-page-field-pointers",product&&product->skusCount==1&&!strcmp(product->skus[0].skuId,"0001")&&product->isInUserCollection&&product->price.price==10.25f&&!strcmp(product->price.currencyCode,"EUR"));
    return TRUE;
}
struct SelfClose {void *handle;int seen=0;};
static BOOLEAN CALLBACK self_close(const XStoreProduct *product,void *context){
    auto c=static_cast<SelfClose*>(context);++c->seen;XodusStoreCloseProductsQueryHandle(c->handle);
    check("self-close-data-remains-alive",pages==1&&!strcmp(product->title,"Synthetic product"));return TRUE;
}
static HRESULT CALLBACK empty_work(XAsyncBlock*){return S_OK;}
static void CALLBACK completed(XAsyncBlock *a){++*static_cast<unsigned*>(a->context);}
struct Retry {void *context;HRESULT result=E_PENDING,restarted=E_PENDING;XTaskQueueHandle next_queue=nullptr;bool reset=false;};
static void CALLBACK retry_same_block(XAsyncBlock *a){
    auto retry=static_cast<Retry*>(a->context);void *page=nullptr;
    retry->result=XodusStoreQueryProductsResult(a,&page);
    if(page)XodusStoreCloseProductsQueryHandle(page);
    mode=Success;if(retry->reset)*a={};a->callback=nullptr;if(retry->next_queue)a->queue=retry->next_queue;
    retry->restarted=start(retry->context,a);
}
int main(){
    check("queue",XTaskQueueCreate(XTaskQueueDispatchMode::Manual,XTaskQueueDispatchMode::Manual,&queue)==S_OK);
    void *context=nullptr;check("context",XodusStoreContextCreate(&binding,nullptr,&context)==S_OK);
    XAsyncBlock a=block();void *page=(void*)1;
    check("null-async",start(context,nullptr)==E_POINTER);
    check("null-ids",XodusStoreQueryProductsAsync(context,XStoreProductKind::Game,nullptr,1,nullptr,0,&a)==E_POINTER);
    check("null-actions",XodusStoreQueryProductsAsync(context,XStoreProductKind::Game,ids,2,nullptr,1,&a)==E_POINTER);
    check("over-100",XodusStoreQueryProductsAsync(context,XStoreProductKind::Game,ids,101,nullptr,0,&a)==E_INVALIDARG);
    check("invalid-kind",XodusStoreQueryProductsAsync(context,static_cast<XStoreProductKind>(32),ids,2,nullptr,0,&a)==E_INVALIDARG);
    const char *bad[]={"../escape"};check("invalid-id",XodusStoreQueryProductsAsync(context,XStoreProductKind::Game,bad,1,nullptr,0,&a)==E_INVALIDARG);
    const char *unknown[]={"UnknownAction"};check("unsupported-action",XodusStoreQueryProductsAsync(context,XStoreProductKind::Game,ids,2,unknown,1,&a)==E_NOTIMPL);
    const char *null[]={nullptr};check("null-element",XodusStoreQueryProductsAsync(context,XStoreProductKind::Game,null,1,nullptr,0,&a)==E_POINTER);
    char first[]="ABCD1234EFGH",second[]="OTHER1234567/0001",action[]="Purchase";
    const char *mutable_ids[]={first,second},*mutable_actions[]={action};
    check("begin",XodusStoreQueryProductsAsync(context,static_cast<XStoreProductKind>(31),mutable_ids,2,mutable_actions,1,&a)==S_OK);
    memset(first,'X',12);memset(second,'Y',17);memset(action,'Z',8);
    check("pending-output-null",XodusStoreQueryProductsResult(&a,&page)==E_PENDING&&!page);
    check("null-result-nonconsuming",XodusStoreQueryProductsResult(&a,nullptr)==E_POINTER);
    dispatch();check("wrong-result-nonconsuming",XodusStoreQueryEntitledProductsResult(&a,&page)==E_INVALIDARG&&!page);
    check("copied-inputs-result",XodusStoreQueryProductsResult(&a,&page)==S_OK&&page&&pages==1);
    void *consumed=(void*)1;
    check("result-once",FAILED(XodusStoreQueryProductsResult(&a,&consumed))&&!consumed);
    int seen=0;check("enumerate",XodusStoreEnumerateProductsQuery(page,&seen,inspect)==S_OK&&seen==1);
    check("has-next",XodusStoreProductsQueryHasMorePages(page));
    a=block();check("next-start",XodusStoreProductsQueryNextPageAsync(page,&a)==S_OK);
    XodusStoreCloseProductsQueryHandle(page);check("old-page-released",pages==0);
    dispatch();check("next-distinct-result",XodusStoreQueryProductsResult(&a,&page)==E_INVALIDARG&&!page);
    check("next-retains-parameters",XodusStoreProductsQueryNextPageResult(&a,&page)==S_OK&&page&&pages==1);
    check("no-next",!XodusStoreProductsQueryHasMorePages(page)&&XodusStoreProductsQueryNextPageAsync(page,&a)==HRESULT_FROM_WIN32(ERROR_NO_MORE_ITEMS));
    SelfClose close{page};check("self-close",XodusStoreEnumerateProductsQuery(page,&close,self_close)==S_OK&&close.seen==1&&pages==0);
    check("closed-handle",XodusStoreEnumerateProductsQuery(page,&seen,inspect)==E_HANDLE);
    for(auto pair:{std::pair<Mode,HRESULT>{Denied,E_ACCESSDENIED},{Malformed,HRESULT_FROM_WIN32(ERROR_INVALID_DATA)},{Unsupported,E_NOTIMPL}}) {
        mode=pair.first;a=block();unsigned completion_count=0;a.callback=completed;a.context=&completion_count;
        check("provider-failure-starts-asynchronously",start(context,&a)==S_OK&&XAsyncGetStatus(&a,FALSE)==E_PENDING&&completion_count==0);
        dispatch();page=(void*)1;
        check("provider-failure-completes-once",XAsyncGetStatus(&a,FALSE)==pair.second&&completion_count==1);
        check("provider-failure-no-page",XodusStoreQueryProductsResult(&a,&page)==pair.second&&!page&&pages==0);
    }
    mode=Success;a=block();int before=calls;start(context,&a);XAsyncCancel(&a);dispatch();
    check("cancel-before-work",XodusStoreQueryProductsResult(&a,&page)==E_ABORT&&!page&&calls==before);
    a=block();XAsyncRun(&a,empty_work);dispatch();check("foreign-result",XodusStoreQueryProductsResult(&a,&page)==E_INVALIDARG&&!page);
    XodusStoreAccountProvider no_join{nullptr,acquire,release};void *missing=nullptr;XodusStoreContextCreate(&no_join,nullptr,&missing);
    a=block();unsigned missing_completed=0;a.callback=completed;a.context=&missing_completed;
    check("missing-collection-starts-asynchronously",start(missing,&a)==S_OK&&XAsyncGetStatus(&a,FALSE)==E_PENDING&&!missing_completed);
    dispatch();check("missing-collection-is-unsupported",XAsyncGetStatus(&a,FALSE)==E_NOTIMPL&&missing_completed==1&&XodusStoreQueryProductsResult(&a,&page)==E_NOTIMPL&&!page);XodusStoreContextClose(missing);
    XTaskQueueHandle immediate=nullptr;XTaskQueueCreate(XTaskQueueDispatchMode::Manual,XTaskQueueDispatchMode::Immediate,&immediate);
    for(auto pair:{std::pair<Mode,HRESULT>{Unsupported,E_NOTIMPL},{Success,S_OK}}) {
        mode=pair.first;Retry retry{context};a={};a.queue=immediate;a.context=&retry;a.callback=retry_same_block;
        check("reentrant-first-begin",start(context,&a)==S_OK);
        XTaskQueueDispatch(immediate,XTaskQueuePort::Work,0);
        check("reentrant-same-block-after-result",retry.result==pair.second&&retry.restarted==S_OK);
        XTaskQueueDispatch(immediate,XTaskQueuePort::Work,0);
        check("reentrant-second-result",XodusStoreQueryProductsResult(&a,&page)==S_OK&&page);
        XodusStoreCloseProductsQueryHandle(page);
    }
    XTaskQueueCloseHandle(immediate);
    for(bool reset:{false,true}) {
        XTaskQueueHandle terminated=nullptr;XTaskQueueCreate(XTaskQueueDispatchMode::Manual,XTaskQueueDispatchMode::Immediate,&terminated);
        Retry terminated_retry{context};terminated_retry.next_queue=queue;terminated_retry.reset=reset;a={};a.queue=terminated;a.context=&terminated_retry;a.callback=retry_same_block;
        check("queue-termination-begin",start(context,&a)==S_OK&&XTaskQueueTerminate(terminated,FALSE,nullptr,nullptr)==S_OK);
        XTaskQueueDispatch(terminated,XTaskQueuePort::Work,0);
        check(reset?"queue-termination-reinitializes-failed-block":"queue-termination-reuses-failed-block",terminated_retry.result==E_ABORT&&terminated_retry.restarted==S_OK);
        dispatch();check("queue-termination-retry-result",XodusStoreQueryProductsResult(&a,&page)==S_OK&&page);XodusStoreCloseProductsQueryHandle(page);
        while(XTaskQueueDispatch(terminated,XTaskQueuePort::Work,0)){}
        XTaskQueueCloseHandle(terminated);
    }
    a=block();start(context,&a);dispatch();XAsyncCancel(&a);
    check("unconsumed-success-block-not-replaced",start(context,&a)==E_INVALIDARG);
    check("cancel-after-success-noop",XAsyncGetStatus(&a,FALSE)==S_OK&&XodusStoreQueryProductsResult(&a,&page)==S_OK&&page);XodusStoreCloseProductsQueryHandle(page);
    a=block();start(context,&a);dispatch();XodusStoreContextClose(context);
    check("close-before-result",XodusStoreQueryProductsResult(&a,&page)==E_ABORT&&!page&&pages==0);
    XodusStoreContextCreate(&binding,nullptr,&context);a=block();start(context,&a);dispatch();XodusStoreQueryProductsResult(&a,&page);XodusStoreContextClose(context);
    seen=0;check("finished-page-retains-snapshot",XodusStoreEnumerateProductsQuery(page,&seen,inspect)==S_OK&&seen==1);
    XodusStoreCloseProductsQueryHandle(page);
    XTaskQueueHandle pool=nullptr;XTaskQueueCreate(XTaskQueueDispatchMode::ThreadPool,XTaskQueueDispatchMode::ThreadPool,&pool);
    entered=CreateEventW(nullptr,TRUE,FALSE,nullptr);blocking=true;
    for(int close_context=0;close_context<2;++close_context) {
        XodusStoreContextCreate(&binding,nullptr,&context);a={};a.queue=pool;ResetEvent(entered);
        check("inflight-start",start(context,&a)==S_OK&&WaitForSingleObject(entered,2000)==WAIT_OBJECT_0);
        if(close_context)XodusStoreContextClose(context);else XAsyncCancel(&a);
        check("inflight-abort",XAsyncGetStatus(&a,TRUE)==E_ABORT&&XodusStoreQueryProductsResult(&a,&page)==E_ABORT&&!page);
        if(!close_context)XodusStoreContextClose(context);
    }
    blocking=false;CloseHandle(entered);XTaskQueueCloseHandle(pool);
    XodusStoreContextCreate(&binding,nullptr,&context);a=block();start(context,&a);
    XodusStoreQueriesShutdown();XodusStoreContextShutdown();dispatch();
    check("shutdown",XodusStoreQueryProductsResult(&a,&page)==E_ABORT&&!page);
    for(unsigned i=0;i<1000&&accounts;++i)Sleep(1);
    check("all-owned-data-released",accounts==0&&pages==0);
    XTaskQueueCloseHandle(queue);
    std::printf("explicit-products checks=%d failures=%d external_requests=0\n",checks,failures);
    return failures?1:0;
}

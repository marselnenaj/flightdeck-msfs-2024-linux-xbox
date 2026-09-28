// SPDX-License-Identifier: LGPL-2.1-or-later
// Synthetic providers only: no acquisition, accounts, network or purchase UI.
#include "StoreQueries.h"
#include "StoreContext.h"
#include <atomic>
#include <cstdio>
#include <string>
void XodusStoreLicenseEventsContextClosed(void*){}
void XodusStoreLicenseEventsShutdown(){}
static int checks,failures,accounts,pages,preview_calls,current_calls;
static HRESULT answer=S_OK;
static XStoreCanLicenseStatus preview_status=XStoreCanLicenseStatus::Licensable;
static std::string sku="0001",observed;
static XTaskQueueHandle queue;
static void check(const char *name,bool ok){++checks;failures+=!ok;printf("preview case=%s pass=%d\n",name,ok);}
static HRESULT WINAPI acquire(void*,void **out){*out=new int(1);++accounts;return S_OK;}
static void WINAPI release(void*,void*p){delete static_cast<int*>(p);--accounts;}
static HRESULT WINAPI preview(void*,void*,const char *id,volatile LONG*,XStoreCanAcquireLicenseResult *out){
    ++preview_calls;observed=id;out->status=preview_status;std::memcpy(out->licensableSku,sku.c_str(),std::min(SIZE_T(5),sku.size()+1));return answer;
}
static XStoreProduct game{};
static HRESULT WINAPI current(void*,void*,volatile LONG*,XodusStoreProductPage **out){
    ++current_calls;*out=new XodusStoreProductPage{sizeof(XodusStoreProductPage),1,&game,nullptr};++pages;return answer;
}
static void WINAPI release_page(void*,XodusStoreProductPage*p){delete p;--pages;}
static XAsyncBlock block(){XAsyncBlock a{};a.queue=queue;return a;}
static void dispatch(){XTaskQueueDispatch(queue,XTaskQueuePort::Work,0);XTaskQueueDispatch(queue,XTaskQueuePort::Completion,0);}
int main(){
    XodusStoreAccountProvider p{nullptr,acquire,release};p.preview_license=preview;p.query_current_game=current;p.release_product_page=release_page;
    game.productKind=XStoreProductKind::Game;game.storeId="ABCD1234EFGH";
    check("queue",XTaskQueueCreate(XTaskQueueDispatchMode::Manual,XTaskQueueDispatchMode::Manual,&queue)==S_OK);
    void*context=nullptr;check("context",XodusStoreContextCreate(&p,nullptr,&context)==S_OK);
    auto a=block();XStoreCanAcquireLicenseResult result{};XStoreProductQueryHandle page=nullptr;
    check("null-id",XodusStoreCanAcquireLicenseForStoreIdAsync(context,nullptr,&a)==E_POINTER);
    check("invalid-id",XodusStoreCanAcquireLicenseForStoreIdAsync(context,"bad",&a)==E_INVALIDARG);
    check("root-product-only",XodusStoreCanAcquireLicenseForStoreIdAsync(context,"ABCD1234EFGH/0001",&a)==E_INVALIDARG);
    check("invalid-context",XodusStoreQueryProductForCurrentGameAsync(nullptr,&a)==E_HANDLE);
    check("null-async",XodusStoreQueryProductForCurrentGameAsync(context,nullptr)==E_POINTER);
    std::string id="ABCD1234EFGH";
    check("preview-begin",XodusStoreCanAcquireLicenseForStoreIdAsync(context,id.c_str(),&a)==S_OK);
    check("pending",XodusStoreCanAcquireLicenseForStoreIdResult(&a,&result)==E_PENDING);
    check("no-duplicate",XodusStoreCanAcquireLicenseForStoreIdAsync(context,id.c_str(),&a)==E_INVALIDARG);
    id="DIFF1234EFGH";dispatch();
    check("correct-api-identity",XodusStoreQueryProductForCurrentGameResult(&a,&page)==E_INVALIDARG);
    check("preview-result",XodusStoreCanAcquireLicenseForStoreIdResult(&a,&result)==S_OK&&result.status==XStoreCanLicenseStatus::Licensable&&!std::strcmp(result.licensableSku,"0001"));
    check("input-copied",observed=="ABCD1234EFGH");
    for(auto value:{XStoreCanLicenseStatus::NotLicensableToUser,XStoreCanLicenseStatus::LicenseActionNotApplicableToProduct}) {
        preview_status=value;sku.clear();a=block();XodusStoreCanAcquireLicenseForStoreIdAsync(context,"ABCD1234EFGH",&a);dispatch();
        check("negative-preview",XodusStoreCanAcquireLicenseForStoreIdResult(&a,&result)==S_OK&&result.status==value&&!result.licensableSku[0]);
    }
    for(auto value:{E_FAIL,E_ACCESSDENIED,E_NOTIMPL,HRESULT_FROM_WIN32(ERROR_TIMEOUT)}) {
        answer=value;a=block();XodusStoreCanAcquireLicenseForStoreIdAsync(context,"ABCD1234EFGH",&a);dispatch();
        check("error-not-negative-rights",XodusStoreCanAcquireLicenseForStoreIdResult(&a,&result)==value);
    }
    answer=S_OK;preview_status=XStoreCanLicenseStatus::Licensable;
    for(auto value:{"","x","lower","ABCDE"}) {
        sku=value;a=block();XodusStoreCanAcquireLicenseForStoreIdAsync(context,"ABCD1234EFGH",&a);dispatch();
        check("malformed-sku",XodusStoreCanAcquireLicenseForStoreIdResult(&a,&result)==HRESULT_FROM_WIN32(ERROR_INVALID_DATA));
    }
    a=block();check("current-begin",XodusStoreQueryProductForCurrentGameAsync(context,&a)==S_OK);dispatch();
    check("current-result",XodusStoreQueryProductForCurrentGameResult(&a,&page)==S_OK&&page&&pages==1);
    check("complete-page",!XodusStoreProductsQueryHasMorePages(page));XodusStoreCloseProductsQueryHandle(page);check("released-page",pages==0);
    for(auto value:{E_FAIL,E_NOTIMPL}) {
        answer=value;a=block();XodusStoreQueryProductForCurrentGameAsync(context,&a);dispatch();
        check("current-failure",XodusStoreQueryProductForCurrentGameResult(&a,&page)==value&&!page&&pages==0);
    }
    answer=S_OK;game.productKind=XStoreProductKind::Consumable;a=block();XodusStoreQueryProductForCurrentGameAsync(context,&a);dispatch();
    check("not-game",XodusStoreQueryProductForCurrentGameResult(&a,&page)==HRESULT_FROM_WIN32(ERROR_INVALID_DATA)&&pages==0);
    int before=preview_calls;a=block();XodusStoreCanAcquireLicenseForStoreIdAsync(context,"ABCD1234EFGH",&a);XAsyncCancel(&a);dispatch();
    check("cancel-before-call",XodusStoreCanAcquireLicenseForStoreIdResult(&a,&result)==E_ABORT&&preview_calls==before);
    before=current_calls;a=block();XodusStoreQueryProductForCurrentGameAsync(context,&a);XodusStoreContextClose(context);dispatch();
    check("closed-context",XodusStoreQueryProductForCurrentGameResult(&a,&page)==E_ABORT&&current_calls==before&&pages==0&&accounts==0);
    p.preview_license=nullptr;p.query_current_game=nullptr;XodusStoreContextCreate(&p,nullptr,&context);a=block();XodusStoreCanAcquireLicenseForStoreIdAsync(context,"ABCD1234EFGH",&a);dispatch();
    check("unavailable-preview",XodusStoreCanAcquireLicenseForStoreIdResult(&a,&result)==E_NOTIMPL);
    a=block();XodusStoreQueryProductForCurrentGameAsync(context,&a);dispatch();check("unavailable-current",XodusStoreQueryProductForCurrentGameResult(&a,&page)==E_NOTIMPL);
    XodusStoreContextClose(context);XodusStoreQueriesShutdown();XodusStoreContextShutdown();XTaskQueueCloseHandle(queue);
    check("released",accounts==0&&pages==0);printf("preview checks=%d failures=%d external_requests=0\n",checks,failures);return failures?1:0;
}

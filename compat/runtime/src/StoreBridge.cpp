/* SPDX-License-Identifier: LGPL-2.1-or-later */
// Isolated Store bridge; LGPL-2.1-or-later, signatures from pinned Xodus IDL.
// Implemented operations use actual broker results; no commerce answer is invented.
#include <initguid.h>
#include "StoreBridge.h"
#include "StoreContext.h"
#include "StoreQueries.h"
#include "StoreLicenseEvents.h"
#include "StoreCatalogProvider.h"
#include <atomic>
#include <xstore.h>
namespace {
using Acquire=HRESULT(WINAPI*)(void **);
using Release=void(WINAPI*)(void *);
using QueryLicense=HRESULT(WINAPI*)(void *,volatile LONG *,XStoreGameLicense *);
using QueryProducts=HRESULT(WINAPI*)(void *,UINT32,UINT32,const char *,volatile LONG *,XodusStoreProductPage **);
using ReleasePage=void(WINAPI*)(XodusStoreProductPage *);
using QueryToken=HRESULT(WINAPI*)(void *,const char *const *,SIZE_T,const char *,volatile LONG *,char **,SIZE_T *);
using QueryExplicit=HRESULT(WINAPI*)(void *,UINT32,const char *const *,SIZE_T,
    const char *const *,SIZE_T,const char *,volatile LONG *,XodusStoreProductPage **);
using QueryCollections=HRESULT(WINAPI*)(void *,const XodusStoreCollectionRequestItem *,SIZE_T,volatile LONG *,XodusStoreCollectionSnapshot **);
using ReleaseCollections=void(WINAPI*)(XodusStoreCollectionSnapshot *);
using GetTitleStoreId=HRESULT(WINAPI*)(char *);
struct AccountRef { void *owned; Release release; QueryLicense license; QueryProducts products; ReleasePage release_page; QueryToken token; QueryExplicit explicit_products; QueryCollections collections; ReleaseCollections release_collections; GetTitleStoreId title_store_id; };
HRESULT WINAPI acquire_account(void *,void **out) {
    if(!out)return E_POINTER; *out=nullptr;
    HMODULE module=GetModuleHandleW(L"xodus_store_test.dll");
    if(!module)return CO_E_NOTINITIALIZED;
    auto acquire=reinterpret_cast<Acquire>(GetProcAddress(module,"XodusStoreAcquireAccount"));
    auto release=reinterpret_cast<Release>(GetProcAddress(module,"XodusStoreReleaseAccount"));
    if(!acquire||!release)return HRESULT_FROM_WIN32(ERROR_PROC_NOT_FOUND);
    auto reference=new(std::nothrow)AccountRef{nullptr,release,
        reinterpret_cast<QueryLicense>(GetProcAddress(module,"XodusStoreQueryGameLicense")),
        reinterpret_cast<QueryProducts>(GetProcAddress(module,"XodusStoreQueryEntitledProducts")),
        reinterpret_cast<ReleasePage>(GetProcAddress(module,"XodusStoreReleaseProductPage")),
        reinterpret_cast<QueryToken>(GetProcAddress(module,"XodusStoreQueryLicenseToken")),
        reinterpret_cast<QueryExplicit>(GetProcAddress(module,"XodusStoreQueryProducts")),
        reinterpret_cast<QueryCollections>(GetProcAddress(module,"XodusStoreQueryCollections")),
        reinterpret_cast<ReleaseCollections>(GetProcAddress(module,"XodusStoreReleaseCollections")),
        reinterpret_cast<GetTitleStoreId>(GetProcAddress(module,"XodusStoreGetTitleStoreId"))};
    if(!reference)return E_OUTOFMEMORY;
    HRESULT hr=acquire(&reference->owned);
    if(FAILED(hr)||!reference->owned){if(reference->owned)release(reference->owned);delete reference;return FAILED(hr)?hr:E_UNEXPECTED;}
    *out=reference;return S_OK;
}
void WINAPI release_account(void *,void *value) {
    auto reference=static_cast<AccountRef*>(value);
    if(reference){reference->release(reference->owned);delete reference;}
}
HRESULT WINAPI query_license(void *,void *value,volatile LONG *cancelled,XStoreGameLicense *out) {
    auto reference=static_cast<AccountRef*>(value);
    if(!reference||!reference->license)return E_NOTIMPL;
    return reference->license(reference->owned,cancelled,out);
}
HRESULT WINAPI query_products(void *,void *value,UINT32 kinds,UINT32 size,const char *cursor,
    volatile LONG *cancelled,XodusStoreProductPage **out) {
    auto reference=static_cast<AccountRef*>(value);
    if(!reference||!reference->products||!reference->release_page)return E_NOTIMPL;
    return reference->products(reference->owned,kinds,size,cursor,cancelled,out);
}
void WINAPI release_product_page(void *,XodusStoreProductPage *page) {
    if(xodus_catalog::release_coin_page(page))return;
    HMODULE module=GetModuleHandleW(L"xodus_store_test.dll");
    auto release=module?reinterpret_cast<ReleasePage>(GetProcAddress(module,"XodusStoreReleaseProductPage")):nullptr;
    if(release)release(page);
}
HRESULT WINAPI query_token(void *,void *value,const char *const *ids,SIZE_T count,const char *custom,
    volatile LONG *cancelled,char **out,SIZE_T *size) {
    auto reference=static_cast<AccountRef*>(value);
    if(!reference||!reference->token)return E_NOTIMPL;
    return reference->token(reference->owned,ids,count,custom,cancelled,out,size);
}
void WINAPI release_token(void *,char *token,SIZE_T size) {
    if(token&&size<=60001)SecureZeroMemory(token,size);
    CoTaskMemFree(token);
}
HRESULT WINAPI query_explicit_products(void *,void *value,UINT32 kinds,const char *const *ids,SIZE_T count,
    const char *const *actions,SIZE_T action_count,const char *cursor,volatile LONG *cancelled,XodusStoreProductPage **out) {
    if(out)*out=nullptr;
    auto reference=static_cast<AccountRef*>(value);
    if(!reference||!reference->collections||!reference->release_collections||!reference->title_store_id)return E_NOTIMPL;
    char parent[13]{};
    HRESULT hr=reference->title_store_id(parent);
    if(FAILED(hr))return hr;
    std::string market,language;
    hr=xodus_catalog::catalog_locale(&market,&language);
    if(FAILED(hr))return hr;
    static xodus_catalog::CatalogReader catalog;
    const xodus_catalog::CollectionsProvider provider{reference->collections,reference->release_collections};
    return xodus_catalog::query_coins(catalog,provider,reference->owned,parent,market,language,kinds,ids,count,actions,action_count,cursor,cancelled,out);
}
const XodusStoreAccountProvider account_provider{nullptr,acquire_account,release_account,query_license,query_products,release_product_page,query_token,release_token,query_explicit_products};
void unsupported(const char *name) {
    static std::atomic<unsigned> count{0};
    if(count.fetch_add(1)<64)std::fprintf(stderr,"[xodus-store] %s hr=80004001\n",name);
}
/* Product-query diagnostics: no action-filter contents or account data. */
static bool diagnostic_store_id(const char *value, char out[18]) {
    out[0]='\0';
    if(!value)return false;
    SIZE_T length=0;
    while(length<18 && value[length])++length;
    if(length!=12 && length!=17)return false;
    for(SIZE_T index=0;index<length;++index) {
        const unsigned char c=static_cast<unsigned char>(value[index]);
        if(index==12 && length==17) {if(c!='/')return false;}
        else if(!((c>='A'&&c<='Z')||(c>='0'&&c<='9')))return false;
    }
    for(SIZE_T index=0;index<length;++index)out[index]=value[index];
    out[length]='\0';
    return true;
}
static void diagnose_product_query(UINT32 kinds,const char *const *ids,SIZE_T count,SIZE_T filter_count) {
    static std::atomic<unsigned> calls{0};
    const unsigned call=calls.fetch_add(1);
    if(call>=64)return;
    std::fprintf(stderr,"[xodus-store] XStoreQueryProductsAsync call=%u kinds=%u product_count=%llu action_filter_count=%llu stage=request\n",
        call,kinds,static_cast<unsigned long long>(count),static_cast<unsigned long long>(filter_count));
    /* The public API limit is 100; diagnostic work is further capped at 16 IDs. */
    if(!ids || count>100)return;
    for(SIZE_T index=0;index<count && index<16;++index) {
        char identifier[18];
        if(diagnostic_store_id(ids[index],identifier))
            std::fprintf(stderr,"[xodus-store] XStoreQueryProductsAsync call=%u product_index=%llu store_id=%s\n",
                call,static_cast<unsigned long long>(index),identifier);
    }
}
/* End product-query diagnostics. */
class Store final : public IXStoreImpl6 {
    std::atomic<ULONG> refs{1};
public:
    HRESULT WINAPI QueryInterface(REFIID iid,void **out) override {
        if(!out)return E_POINTER;*out=nullptr;
        if(iid!=IID_IUnknown && iid!=__uuidof(IXStoreImpl) && iid!=__uuidof(IXStoreImpl2) &&
           iid!=__uuidof(IXStoreImpl3) && iid!=__uuidof(IXStoreImpl4) &&
           iid!=__uuidof(IXStoreImpl5) && iid!=__uuidof(IXStoreImpl6))return E_NOINTERFACE;
        *out=static_cast<IXStoreImpl6*>(this);AddRef();return S_OK;
    }
    ULONG WINAPI AddRef() override {return ++refs;}
    ULONG WINAPI Release() override {return --refs;}
    HRESULT WINAPI XStoreCreateContext(const XUserHandle user, XStoreContextHandle *storeContextHandle) override { HRESULT hr=XodusStoreContextCreate(&account_provider,user,storeContextHandle);std::fprintf(stderr,"[xodus-store] XStoreCreateContext hr=%08lx\n",static_cast<ULONG>(hr));return hr; }
    void WINAPI XStoreCloseContextHandle(XStoreContextHandle storeContextHandle) override { XodusStoreContextClose(storeContextHandle); }
    HRESULT WINAPI XStoreQueryAssociatedProductsAsync(const XStoreContextHandle storeContextHandle, XStoreProductKind productKinds, UINT32 maxItemsToRetrievePerPage, XAsyncBlock *async) override { unsupported("XStoreQueryAssociatedProductsAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryAssociatedProductsResult(XAsyncBlock *async, XStoreProductQueryHandle *productQueryHandle) override { unsupported("XStoreQueryAssociatedProductsResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryProductsAsync(const XStoreContextHandle storeContextHandle, XStoreProductKind productKinds, const char **storeIds, SIZE_T storeIdsCount, const char **actionFilters, SIZE_T actionFiltersCount, XAsyncBlock *async) override { diagnose_product_query(static_cast<UINT32>(productKinds),storeIds,storeIdsCount,actionFiltersCount);return XodusStoreQueryProductsAsync(storeContextHandle,productKinds,storeIds,storeIdsCount,actionFilters,actionFiltersCount,async); }
    HRESULT WINAPI XStoreQueryProductsResult(XAsyncBlock *async, XStoreProductQueryHandle *productQueryHandle) override { return XodusStoreQueryProductsResult(async,productQueryHandle); }
    HRESULT WINAPI XStoreQueryEntitledProductsAsync(const XStoreContextHandle storeContextHandle, XStoreProductKind productKinds, UINT32 maxItemsToRetrievePerPage, XAsyncBlock *async) override { return XodusStoreQueryEntitledProductsAsync(storeContextHandle,productKinds,maxItemsToRetrievePerPage,async); }
    HRESULT WINAPI XStoreQueryEntitledProductsResult(XAsyncBlock *async, XStoreProductQueryHandle *productQueryHandle) override { return XodusStoreQueryEntitledProductsResult(async,productQueryHandle); }
    HRESULT WINAPI XStoreQueryProductForCurrentGameAsync(const XStoreContextHandle storeContextHandle, XAsyncBlock *async) override { unsupported("XStoreQueryProductForCurrentGameAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryProductForCurrentGameResult(XAsyncBlock *async, XStoreProductQueryHandle *productQueryHandle) override { unsupported("XStoreQueryProductForCurrentGameResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryProductForPackageAsync(const XStoreContextHandle storeContextHandle, XStoreProductKind productKinds, const char *packageIdentifier, XAsyncBlock *async) override { unsupported("XStoreQueryProductForPackageAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryProductForPackageResult(XAsyncBlock *async, XStoreProductQueryHandle *productQueryHandle) override { unsupported("XStoreQueryProductForPackageResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreEnumerateProductsQuery(const XStoreProductQueryHandle productQueryHandle, void *context, XStoreProductQueryCallback *callback) override { return XodusStoreEnumerateProductsQuery(productQueryHandle,context,callback); }
    BOOLEAN WINAPI XStoreProductsQueryHasMorePages(const XStoreProductQueryHandle productQueryHandle) override { return XodusStoreProductsQueryHasMorePages(productQueryHandle); }
    HRESULT WINAPI XStoreProductsQueryNextPageAsync(const XStoreProductQueryHandle productQueryHandle, XAsyncBlock *async) override { return XodusStoreProductsQueryNextPageAsync(productQueryHandle,async); }
    HRESULT WINAPI XStoreProductsQueryNextPageResult(XAsyncBlock *async, XStoreProductQueryHandle *productQueryHandle) override { return XodusStoreProductsQueryNextPageResult(async,productQueryHandle); }
    void WINAPI XStoreCloseProductsQueryHandle(XStoreProductQueryHandle productQueryHandle) override { XodusStoreCloseProductsQueryHandle(productQueryHandle); }
    HRESULT WINAPI XStoreAcquireLicenseForPackageAsync(const XStoreProductQueryHandle productQueryHandle, const char *packageIdentifier, XAsyncBlock *async) override { unsupported("XStoreAcquireLicenseForPackageAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreAcquireLicenseForPackageResult(XAsyncBlock *async, XStoreLicenseHandle *storeLicenseHandle) override { unsupported("XStoreAcquireLicenseForPackageResult");return E_NOTIMPL; }
    BOOLEAN WINAPI XStoreIsLicenseValid(const XStoreLicenseHandle storeLicenseHandle) override { unsupported("XStoreIsLicenseValid");return FALSE; }
    void WINAPI XStoreCloseLicenseHandle(XStoreLicenseHandle storeLicenseHandle) override { unsupported("XStoreCloseLicenseHandle"); }
    HRESULT WINAPI XStoreCanAcquireLicenseForStoreIdAsync(const XStoreContextHandle storeContextHandle, const char *storeProductId, XAsyncBlock *async) override { unsupported("XStoreCanAcquireLicenseForStoreIdAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreCanAcquireLicenseForStoreIdResult(XAsyncBlock *async, XStoreCanAcquireLicenseResult *storeCanAcquireLicense) override { unsupported("XStoreCanAcquireLicenseForStoreIdResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreCanAcquireLicenseForPackageAsync(const XStoreContextHandle storeContextHandle, const char *packageIdentifier, XAsyncBlock *async) override { unsupported("XStoreCanAcquireLicenseForPackageAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreCanAcquireLicenseForPackageResult(XAsyncBlock *async, XStoreCanAcquireLicenseResult *storeCanAcquireLicense) override { unsupported("XStoreCanAcquireLicenseForPackageResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryGameLicenseAsync(const XStoreContextHandle storeContextHandle, XAsyncBlock *async) override { return XodusStoreQueryGameLicenseAsync(storeContextHandle,async); }
    HRESULT WINAPI XStoreQueryGameLicenseResult(XAsyncBlock *async, XStoreGameLicense *license) override { return XodusStoreQueryGameLicenseResult(async,license); }
    HRESULT WINAPI XStoreQueryAddOnLicensesAsync(const XStoreContextHandle storeContextHandle, XAsyncBlock *async) override { unsupported("XStoreQueryAddOnLicensesAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryAddOnLicensesResultCount(XAsyncBlock *async, UINT32 *count) override { unsupported("XStoreQueryAddOnLicensesResultCount");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryAddOnLicensesResult(XAsyncBlock *async, UINT32 count, XStoreAddonLicense *addOnLicenses) override { unsupported("XStoreQueryAddOnLicensesResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryConsumableBalanceRemainingAsync(const XStoreContextHandle storeContextHandle, const char *storeProductId, XAsyncBlock *async) override { unsupported("XStoreQueryConsumableBalanceRemainingAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryConsumableBalanceRemainingResult(XAsyncBlock *async, XStoreConsumableResult *consumableResult) override { unsupported("XStoreQueryConsumableBalanceRemainingResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreReportConsumableFulfillmentAsync(const XStoreContextHandle storeContextHandle, const char *storeProductId, UINT32 quantity, GUID trackingId, XAsyncBlock *async) override { unsupported("XStoreReportConsumableFulfillmentAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreReportConsumableFulfillmentResult(XAsyncBlock *async, XStoreConsumableResult *consumableResult) override { unsupported("XStoreReportConsumableFulfillmentResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreGetUserCollectionsIdAsync(const XStoreContextHandle storeContextHandle, const char *serviceTicket, const char *publisherUserId, XAsyncBlock *async) override { unsupported("XStoreGetUserCollectionsIdAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreGetUserCollectionsIdResultSize(XAsyncBlock *async, SIZE_T *size) override { unsupported("XStoreGetUserCollectionsIdResultSize");return E_NOTIMPL; }
    HRESULT WINAPI XStoreGetUserCollectionsIdResult(XAsyncBlock *async, SIZE_T size, char *result) override { unsupported("XStoreGetUserCollectionsIdResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreGetUserPurchaseIdAsync(const XStoreContextHandle storeContextHandle, const char *serviceTicket, const char *publisherUserId, XAsyncBlock *async) override { unsupported("XStoreGetUserPurchaseIdAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreGetUserPurchaseIdResultSize(XAsyncBlock *async, SIZE_T *size) override { unsupported("XStoreGetUserPurchaseIdResultSize");return E_NOTIMPL; }
    HRESULT WINAPI XStoreGetUserPurchaseIdResult(XAsyncBlock *async, SIZE_T size, char *result) override { unsupported("XStoreGetUserPurchaseIdResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryLicenseTokenAsync(const XStoreContextHandle storeContextHandle, const char **productIds, SIZE_T productIdsCount, const char *customDeveloperString, XAsyncBlock *async) override { return XodusStoreQueryLicenseTokenAsync(storeContextHandle,productIds,productIdsCount,customDeveloperString,async); }
    HRESULT WINAPI XStoreQueryLicenseTokenResultSize(XAsyncBlock *async, SIZE_T *size) override { return XodusStoreQueryLicenseTokenResultSize(async,size); }
    HRESULT WINAPI XStoreQueryLicenseTokenResult(XAsyncBlock *async, SIZE_T size, char *result) override { return XodusStoreQueryLicenseTokenResult(async,size,result); }
    HRESULT WINAPI __PADDING__() override { unsupported("__PADDING__");return E_NOTIMPL; }
    HRESULT WINAPI __PADDING_2__() override { unsupported("__PADDING_2__");return E_NOTIMPL; }
    HRESULT WINAPI __PADDING_3__() override { unsupported("__PADDING_3__");return E_NOTIMPL; }
    HRESULT WINAPI XStoreShowPurchaseUIAsync(const XStoreContextHandle storeContextHandle, const char *storeId, const char *name, const char *extendedJsonData, XAsyncBlock *async) override { unsupported("XStoreShowPurchaseUIAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreShowPurchaseUIResult(XAsyncBlock *async) override { unsupported("XStoreShowPurchaseUIResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreShowRateAndReviewUIAsync(const XStoreContextHandle storeContextHandle, XAsyncBlock *async) override { unsupported("XStoreShowRateAndReviewUIAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreShowRateAndReviewUIResult(XAsyncBlock *async, XStoreRateAndReviewResult *result) override { unsupported("XStoreShowRateAndReviewUIResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreShowRedeemTokenUIAsync(const XStoreContextHandle storeContextHandle, const char *token, const char **allowedStoreIds, SIZE_T allowedStoreIdsCount, BOOLEAN disallowCsvRedemption, XAsyncBlock *async) override { unsupported("XStoreShowRedeemTokenUIAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreShowRedeemTokenUIResult(XAsyncBlock *async) override { unsupported("XStoreShowRedeemTokenUIResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryGameAndDlcPackageUpdatesAsync(const XStoreContextHandle storeContextHandle, XAsyncBlock *async) override { unsupported("XStoreQueryGameAndDlcPackageUpdatesAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryGameAndDlcPackageUpdatesResultCount(XAsyncBlock *async, UINT32 *count) override { unsupported("XStoreQueryGameAndDlcPackageUpdatesResultCount");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryGameAndDlcPackageUpdatesResult(XAsyncBlock *async, UINT32 count, XStorePackageUpdate *packageUpdates) override { unsupported("XStoreQueryGameAndDlcPackageUpdatesResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreDownloadPackageUpdatesAsync(XStoreContextHandle storeContextHandle, const char **packageIdentifiers, SIZE_T packageIdentifiersCount, XAsyncBlock *async) override { unsupported("XStoreDownloadPackageUpdatesAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreDownloadPackageUpdatesResult(XAsyncBlock *async) override { unsupported("XStoreDownloadPackageUpdatesResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreDownloadAndInstallPackageUpdatesAsync(const XStoreContextHandle storeContextHandle, const char **packageIdentifiers, SIZE_T packageIdentifiersCount, XAsyncBlock *async) override { unsupported("XStoreDownloadAndInstallPackageUpdatesAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreDownloadAndInstallPackageUpdatesResult(XAsyncBlock *async) override { unsupported("XStoreDownloadAndInstallPackageUpdatesResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreDownloadAndInstallPackagesAsync(const XStoreContextHandle storeContextHandle, const char **storeIds, SIZE_T storeIdsCount, XAsyncBlock *async) override { unsupported("XStoreDownloadAndInstallPackagesAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreDownloadAndInstallPackagesResultCount(XAsyncBlock *async, UINT32 *count) override { unsupported("XStoreDownloadAndInstallPackagesResultCount");return E_NOTIMPL; }
    HRESULT WINAPI XStoreDownloadAndInstallPackagesResult(XAsyncBlock *async, UINT32 count, char **packageIdentifiers) override { unsupported("XStoreDownloadAndInstallPackagesResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryPackageIdentifier(const char *storeId, SIZE_T size, char *packageIdentifier) override { unsupported("XStoreQueryPackageIdentifier");return E_NOTIMPL; }
    HRESULT WINAPI XStoreRegisterGameLicenseChanged(XStoreContextHandle storeContextHandle, XTaskQueueHandle queue, void *context, XStoreGameLicenseChangedCallback *callback, XTaskQueueRegistrationToken *token) override { return XodusStoreRegisterGameLicenseChanged(storeContextHandle,queue,context,callback,token); }
    BOOLEAN WINAPI XStoreUnregisterGameLicenseChanged(XStoreContextHandle storeContextHandle, XTaskQueueRegistrationToken token, BOOLEAN wait) override { return XodusStoreUnregisterGameLicenseChanged(storeContextHandle,token,wait); }
    HRESULT WINAPI XStoreRegisterPackageLicenseLost(XStoreLicenseHandle storeLicenseHandle, XTaskQueueHandle queue, void *context, XStorePackageLicenseLostCallback *callback, XTaskQueueRegistrationToken *token) override { unsupported("XStoreRegisterPackageLicenseLost");return E_NOTIMPL; }
    BOOLEAN WINAPI XStoreUnregisterPackageLicenseLost(XStoreLicenseHandle licenseHandle, XTaskQueueRegistrationToken token, BOOLEAN wait) override { unsupported("XStoreUnregisterPackageLicenseLost");return FALSE; }
    BOOLEAN WINAPI XStoreIsAvailabilityPurchasable(const XStoreAvailability availability) override { unsupported("XStoreIsAvailabilityPurchasable");return FALSE; }
    HRESULT WINAPI XStoreAcquireLicenseForDurablesAsync(const XStoreContextHandle storeContextHandle, const char *storeId, XAsyncBlock *async) override { unsupported("XStoreAcquireLicenseForDurablesAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreAcquireLicenseForDurablesResult(XAsyncBlock *async, XStoreLicenseHandle *storeLicenseHandle) override { unsupported("XStoreAcquireLicenseForDurablesResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreShowAssociatedProductsUIAsync(const XStoreContextHandle storeContextHandle, const char *storeId, XStoreProductKind productKinds, XAsyncBlock *async) override { unsupported("XStoreShowAssociatedProductsUIAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreShowAssociatedProductsUIResult(XAsyncBlock *async) override { unsupported("XStoreShowAssociatedProductsUIResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreShowProductPageUIAsync(const XStoreContextHandle storeContextHandle, const char *storeId, XAsyncBlock *async) override { unsupported("XStoreShowProductPageUIAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreShowProductPageUIResult(XAsyncBlock *async) override { unsupported("XStoreShowProductPageUIResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryAssociatedProductsForStoreIdAsync(const XStoreContextHandle storeContextHandle, const char *storeId, XStoreProductKind productKinds, UINT32 maxItemsToRetrievePerPage, XAsyncBlock *async) override { unsupported("XStoreQueryAssociatedProductsForStoreIdAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryAssociatedProductsForStoreIdResult(XAsyncBlock *async, XStoreProductQueryHandle *productQueryHandle) override { unsupported("XStoreQueryAssociatedProductsForStoreIdResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryPackageUpdatesAsync(XStoreContextHandle storeContextHandle, const char **packageIdentifiers, SIZE_T packageIdentifiersCount, XAsyncBlock *async) override { unsupported("XStoreQueryPackageUpdatesAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryPackageUpdatesResultCount(XAsyncBlock *async, UINT32 *count) override { unsupported("XStoreQueryPackageUpdatesResultCount");return E_NOTIMPL; }
    HRESULT WINAPI XStoreQueryPackageUpdatesResult(XAsyncBlock *async, UINT32 count, XStorePackageUpdate *packageUpdates) override { unsupported("XStoreQueryPackageUpdatesResult");return E_NOTIMPL; }
    HRESULT WINAPI XStoreShowGiftingUIAsync(const XStoreContextHandle storeContextHandle, const char *storeId, const char *name, const char *extendedJsonData, XAsyncBlock *async) override { unsupported("XStoreShowGiftingUIAsync");return E_NOTIMPL; }
    HRESULT WINAPI XStoreShowGiftingUIResult(XAsyncBlock *async) override { unsupported("XStoreShowGiftingUIResult");return E_NOTIMPL; }
};
Store instance;
}
bool IsStoreRuntimeClass(const GUID *clsid) {return clsid&&*clsid==__uuidof(IXStoreImpl);}
HRESULT QueryStoreRuntime(const GUID *clsid,REFIID iid,void **out) {
    if(!out||!clsid)return E_POINTER;*out=nullptr;
    if(!IsStoreRuntimeClass(clsid))return E_NOINTERFACE;
    return instance.QueryInterface(iid,out);
}
void ShutdownStoreRuntime() {XodusStoreQueriesShutdown();XodusStoreContextShutdown();}

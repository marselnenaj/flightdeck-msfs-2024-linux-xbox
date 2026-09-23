/* SPDX-License-Identifier: LGPL-2.1-or-later */
// Native Store query lifecycle. Provider data must come from verified backend
// responses. Synthetic fixtures are injected only by standalone test programs.
#include "StoreQueries.h"
#include "StoreContext.h"
#include "StoreDurableLicense.h"
#include <algorithm>
#include <atomic>
#include <memory>
#include <mutex>
#include <new>
#include <string>
#include <unordered_map>
#include <vector>

namespace {
enum class Kind { License, Products, NextPage, LicenseToken, ExplicitProducts, Balance, Durable, PackageUpdates };
const char license_identity=0, products_identity=0, next_identity=0, token_identity=0, explicit_identity=0, balance_identity=0, durable_identity=0, updates_identity=0;
constexpr SIZE_T max_token_size=60001, max_challenge_size=8192, max_product_count=100;
std::atomic<bool> stopped{false};
std::atomic<unsigned> diagnostic_count{0};
void phase(Kind kind,const char *stage,HRESULT hr,bool queue_null) {
    static std::atomic<unsigned> count{0};
    if(count.fetch_add(1)<128)std::fprintf(stderr,"[xodus-store-async] kind=%u stage=%s hr=%08lx queue_null=%d\n",
        static_cast<unsigned>(kind),stage,static_cast<ULONG>(hr),queue_null);
}
void diagnostic(Kind kind,HRESULT hr) {
    if(diagnostic_count.fetch_add(1)<64)
        std::fprintf(stderr,"[xodus-store-query] kind=%u hr=%08lx\n",static_cast<unsigned>(kind),static_cast<ULONG>(hr));
}
const void *identity(Kind kind) {
    return kind==Kind::License?&license_identity:kind==Kind::Products?&products_identity:
        kind==Kind::NextPage?&next_identity:kind==Kind::ExplicitProducts?&explicit_identity:
        kind==Kind::Balance?&balance_identity:kind==Kind::Durable?&durable_identity:
        kind==Kind::PackageUpdates?&updates_identity:&token_identity;
}
struct Page {
    XodusStoreContextRef context;
    XodusStoreProductPage *data=nullptr;
    UINT32 kinds=0, page_size=0;
    bool explicit_products=false;
    std::vector<std::string> product_ids, action_filters;
    std::atomic<bool> closed{false};
    ~Page() {
        auto provider=XodusStoreContextProvider(context);
        if(data&&provider&&provider->release_product_page)
            provider->release_product_page(provider->state,data);
    }
};
std::mutex pages_mutex;
std::unordered_map<void*,std::shared_ptr<Page>> pages;
struct Job {
    Kind kind;
    XAsyncBlock *key;
    XodusStoreContextRef context;
    UINT32 kinds=0, page_size=0;
    std::string continuation;
    volatile LONG cancelled=0;
    XStoreGameLicense license{};
    XStoreConsumableResult balance{};
    XStoreLicenseHandle durable=nullptr;
    std::shared_ptr<Page> page;
    std::vector<std::string> product_ids;
    std::vector<std::string> action_filters;
    bool explicit_products=false;
    std::string custom;
    char *token=nullptr;
    SIZE_T token_size=0;
    ~Job() {
        if(durable)XodusStoreCloseLicenseHandle(durable);
        auto binding=XodusStoreContextProvider(context);
        if(token&&binding&&binding->release_license_token)
            binding->release_license_token(binding->state,token,token_size);
        if(!custom.empty())SecureZeroMemory(&custom[0],custom.size());
    }
};
std::mutex jobs_mutex;
std::unordered_map<XAsyncBlock*,Job*> jobs;
void forget_job(Job *job) {
    std::lock_guard<std::mutex> lock(jobs_mutex);
    auto it=jobs.find(job->key);
    if(it!=jobs.end()&&it->second==job)jobs.erase(it);
}
bool abort_job(Job *job) {
    return stopped.load() || InterlockedCompareExchange(&job->cancelled,0,0) ||
        !XodusStoreContextIsOpen(job->context);
}
HRESULT validate_license(const XStoreGameLicense &value) {
    return std::memchr(value.skuStoreId,0,sizeof(value.skuStoreId)) &&
        std::memchr(value.trialUniqueId,0,sizeof(value.trialUniqueId)) &&
        value.isActive<=1 && value.isTrialOwnedByThisUser<=1 && value.isDiscLicense<=1 && value.isTrial<=1 ?
        S_OK:HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
}
HRESULT query_balance(Job *job,const XodusStoreAccountProvider *provider) {
    if(!provider->query_products||!provider->release_product_page)return E_NOTIMPL;
    const char *id=job->product_ids[0].c_str();
    XodusStoreProductPage *raw=nullptr;
    const HRESULT hr=provider->query_products(provider->state,XodusStoreContextAccount(job->context),
        static_cast<UINT32>(XStoreProductKind::Consumable),&id,1,nullptr,0,nullptr,&job->cancelled,&raw);
    // Providers may allocate a page even when they report failure.
    const auto release=[&](XodusStoreProductPage *page) {provider->release_product_page(provider->state,page);};
    std::unique_ptr<XodusStoreProductPage,decltype(release)> page(raw,release);
    if(FAILED(hr))return hr;
    if(!page || page->structure_size!=sizeof(*page) || page->product_count!=1 ||
       !page->products || (page->continuation&&*page->continuation))return E_NOTIMPL;
    const XStoreProduct &product=page->products[0];
    if(!product.storeId || std::strncmp(product.storeId,id,13) ||
       product.productKind!=XStoreProductKind::Consumable ||
       product.isInUserCollection>1 || product.skusCount!=1 || !product.skus)return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
    const XStoreSku &sku=product.skus[0];
    if(!sku.skuId || strnlen(sku.skuId,5)!=4 ||
       !std::all_of(sku.skuId,sku.skuId+4,[](char c){return (c>='A'&&c<='Z')||(c>='0'&&c<='9');}) ||
       sku.isInUserCollection>1 || sku.isInUserCollection!=product.isInUserCollection)
        return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
    job->balance.quantity=sku.isInUserCollection?sku.collectionData.quantity:0;
    return S_OK;
}
HRESULT do_work(Job *job) {
    if(abort_job(job))return E_ABORT;
    auto provider=XodusStoreContextProvider(job->context);
    HRESULT hr;
    if(job->kind==Kind::License) {
        if(!provider->query_game_license)return E_NOTIMPL;
        hr=provider->query_game_license(provider->state,XodusStoreContextAccount(job->context),&job->cancelled,&job->license);
        if(SUCCEEDED(hr))hr=validate_license(job->license);
    } else if(job->kind==Kind::PackageUpdates) {
        if(!provider->check_package_updates)return E_NOTIMPL;
        hr=provider->check_package_updates(provider->state,XodusStoreContextAccount(job->context),&job->cancelled);
        // The current provider contract proves an exact current revision only.
        // S_FALSE is not proof of an empty result, nor is any failed request.
        if(SUCCEEDED(hr)&&hr!=S_OK)hr=HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
    } else if(job->kind==Kind::Durable) {
        hr=XodusStoreDurableAcquire(job->context,job->product_ids[0].c_str(),&job->cancelled,&job->durable);
    } else if(job->kind==Kind::Balance) {
        hr=query_balance(job,provider);
    } else if(job->kind==Kind::LicenseToken) {
        if(!provider->query_license_token||!provider->release_license_token)return E_NOTIMPL;
        std::vector<const char*> ids;ids.reserve(job->product_ids.size());
        for(const auto &id:job->product_ids)ids.push_back(id.c_str());
        hr=provider->query_license_token(provider->state,XodusStoreContextAccount(job->context),
            ids.data(),ids.size(),job->custom.c_str(),&job->cancelled,&job->token,&job->token_size);
        if(SUCCEEDED(hr)&&(!job->token||job->token_size<2||job->token_size>max_token_size||
            job->token[job->token_size-1]||std::memchr(job->token,0,job->token_size-1)))
            hr=HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
    } else {
        if(!provider->release_product_page ||
            (job->explicit_products?!provider->query_products:!provider->query_entitled_products))return E_NOTIMPL;
        auto page=std::make_shared<Page>();
        page->context=job->context;page->kinds=job->kinds;page->page_size=job->page_size;
        page->explicit_products=job->explicit_products;
        if(job->explicit_products) {
            page->product_ids=job->product_ids;page->action_filters=job->action_filters;
            std::vector<const char*> ids,actions;
            for(const auto &id:job->product_ids)ids.push_back(id.c_str());
            for(const auto &action:job->action_filters)actions.push_back(action.c_str());
            hr=provider->query_products(provider->state,XodusStoreContextAccount(job->context),job->kinds,
                ids.data(),ids.size(),actions.data(),actions.size(),
                job->continuation.empty()?nullptr:job->continuation.c_str(),&job->cancelled,&page->data);
        } else hr=provider->query_entitled_products(provider->state,XodusStoreContextAccount(job->context),job->kinds,
                job->page_size,job->continuation.empty()?nullptr:job->continuation.c_str(),&job->cancelled,&page->data);
        if(SUCCEEDED(hr)) {
            if(!page->data || page->data->structure_size!=sizeof(*page->data) ||
               page->data->product_count>job->page_size || (page->data->product_count&&!page->data->products))
                hr=HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
            else job->page=std::move(page);
        }
    }
    return abort_job(job)?E_ABORT:hr;
}
HRESULT WINAPI provider(XAsyncOp op,const XAsyncProviderData *data) {
    auto job=static_cast<Job*>(data->context);
    switch(op) {
    case XAsyncOp::Begin: {
        const Kind kind=job->kind;
        const bool queue_null=!data->async->queue;
        HRESULT hr=XAsyncSchedule(data->async,0);
        /* Immediate dispatch can finish and clean up the job inside Schedule. */
        phase(kind,"schedule",hr,queue_null);
        return hr;
    }
    case XAsyncOp::DoWork: {
        phase(job->kind,"work_enter",S_OK,!data->async->queue);
        HRESULT hr;
        try { hr=do_work(job); }
        catch(const std::bad_alloc&) {hr=E_OUTOFMEMORY;}
        catch(...) {hr=E_FAIL;}
        diagnostic(job->kind,hr);
        SIZE_T required=job->kind==Kind::License?sizeof(job->license):
            job->kind==Kind::PackageUpdates?0:
            job->kind==Kind::Durable?sizeof(job->durable):
            job->kind==Kind::Balance?sizeof(job->balance):
            job->kind==Kind::LicenseToken?job->token_size:sizeof(XStoreProductQueryHandle);
        /* A failed operation has no retrievable provider result. Unregister
         * before completion can reenter Begin with this same caller block.
         * XAsync still owns the job until its delayed Cleanup callback. */
        if(FAILED(hr)||!required)forget_job(job);
        XAsyncComplete(data->async,hr,SUCCEEDED(hr)?required:0);
        return E_PENDING;
    }
    case XAsyncOp::GetResult:
        /* Result retrieval consumes this provider state. Its Cleanup may run
         * after GetResult returns when DoWork is still unwinding. */
        forget_job(job);
        if(abort_job(job))return E_ABORT;
        if(job->kind==Kind::License)std::memcpy(data->buffer,&job->license,sizeof(job->license));
        else if(job->kind==Kind::Durable) {
            if(!XodusStoreIsLicenseValid(job->durable))return HRESULT_FROM_WIN32(ERROR_LOGON_FAILURE);
            *static_cast<XStoreLicenseHandle*>(data->buffer)=job->durable;job->durable=nullptr;
        }
        else if(job->kind==Kind::Balance)std::memcpy(data->buffer,&job->balance,sizeof(job->balance));
        else if(job->kind==Kind::LicenseToken)std::memcpy(data->buffer,job->token,job->token_size);
        else {
            try {
                std::lock_guard<std::mutex> lock(pages_mutex);
                if(stopped.load()||!XodusStoreContextIsOpen(job->context))return E_ABORT;
                auto handle=job->page.get();
                pages.emplace(handle,job->page);
                *static_cast<XStoreProductQueryHandle*>(data->buffer)=handle;
            } catch(const std::bad_alloc&) {return E_OUTOFMEMORY;}
        }
        return S_OK;
    case XAsyncOp::Cancel:
        phase(job->kind,"cancel",S_OK,!data->async->queue);
        if(XAsyncGetStatus(data->async,FALSE)!=E_PENDING)return S_OK;
        InterlockedExchange(&job->cancelled,1);
        return S_OK;
    case XAsyncOp::Cleanup:
        phase(job->kind,"cleanup",S_OK,!data->async->queue);
        forget_job(job);
        delete job;
        return S_OK;
    }
    return E_NOTIMPL;
}
HRESULT begin(std::unique_ptr<Job> job,XAsyncBlock *async) {
    if(!async)return E_POINTER;
    if(stopped.load())return E_ABORT;
    job->key=async;
    {
        try {
            std::lock_guard<std::mutex> lock(jobs_mutex);
            auto existing=jobs.find(async);
            if(existing!=jobs.end()) {
                /* Queue termination can complete a failed operation without
                 * DoWork. A completion callback may reuse its caller block
                 * before old Cleanup; retain only pending/successful results. */
                SIZE_T required=0;
                const HRESULT status=XAsyncGetResultSize(async,&required);
                /* Successful product/license queries have nonempty results.
                 * Empty update checks and caller reinitialization after a
                 * completed failure, without retaining the old registry slot. */
                if(status==E_PENDING||(SUCCEEDED(status)&&required))return E_INVALIDARG;
                jobs.erase(existing);
            }
            jobs.emplace(async,job.get());
        } catch(const std::bad_alloc&) {return E_OUTOFMEMORY;}
    }
    /* XAsyncBegin takes provider cleanup ownership, including Begin failure. */
    Job *raw=job.release();
    Kind kind=raw->kind;
    HRESULT hr=XAsyncBegin(async,raw,identity(kind),"XodusStoreQuery",provider);
    phase(kind,"begin_return",hr,!async->queue);
    if(FAILED(hr)) {
        Job *detached=nullptr;
        {
            std::lock_guard<std::mutex> lock(jobs_mutex);
            auto it=jobs.find(async);
            if(it!=jobs.end()&&it->second==raw){detached=it->second;jobs.erase(it);}
        }
        delete detached;
    }
    return hr;
}
HRESULT get_result(Kind kind,XAsyncBlock *async,void *out,SIZE_T size) {
    if(!async||!out)return E_POINTER;
    SIZE_T required=0;
    HRESULT hr=XAsyncGetResultSize(async,&required);
    if(FAILED(hr))return hr;
    if(required!=size)return E_INVALIDARG;
    {
        std::lock_guard<std::mutex> lock(jobs_mutex);
        auto it=jobs.find(async);
        if(it==jobs.end()||it->second->kind!=kind)return E_INVALIDARG;
    }
    return XAsyncGetResult(async,identity(kind),size,out,nullptr);
}
std::shared_ptr<Page> retain_page(void *handle) {
    std::lock_guard<std::mutex> lock(pages_mutex);
    auto it=pages.find(handle);
    return it==pages.end()?nullptr:it->second;
}
}

HRESULT XodusStoreQueryGameAndDlcPackageUpdatesAsync(XStoreContextHandle handle,XAsyncBlock *async) {
    if(!async)return E_POINTER;
    try {
        auto job=std::make_unique<Job>();job->kind=Kind::PackageUpdates;
        HRESULT hr=XodusStoreContextRetain(handle,&job->context);
        if(FAILED(hr))return hr;
        return begin(std::move(job),async);
    } catch(const std::bad_alloc&) {return E_OUTOFMEMORY;}
}
HRESULT XodusStoreQueryGameAndDlcPackageUpdatesResultCount(XAsyncBlock *async,UINT32 *out) {
    if(out)*out=0;
    if(!async||!out)return E_POINTER;
    SIZE_T size=0;HRESULT hr=XAsyncGetResultSize(async,&size);
    if(FAILED(hr))return hr;
    // Zero-length success has already released its provider/account state.
    // This follows XAsync's empty-result contract: callers commonly stop at
    // ResultCount when there are no updates and never request a result array.
    return size==0?S_OK:E_INVALIDARG;
}
HRESULT XodusStoreQueryGameAndDlcPackageUpdatesResult(XAsyncBlock *async,UINT32 count,XStorePackageUpdate *out) {
    if(!async)return E_POINTER;
    if(count)return E_INVALIDARG;
    (void)out;
    UINT32 observed=0;HRESULT hr=XodusStoreQueryGameAndDlcPackageUpdatesResultCount(async,&observed);
    if(FAILED(hr))return hr;
    return XAsyncGetResult(async,identity(Kind::PackageUpdates),0,nullptr,nullptr);
}

HRESULT XodusStoreQueryGameLicenseAsync(XStoreContextHandle handle,XAsyncBlock *async) {
    static std::atomic<unsigned> calls{0};
    if(calls.fetch_add(1)<16)std::fprintf(stderr,"[xodus-store-query] GameLicenseAsync begin\n");
    if(!async)return E_POINTER;
    try {
        auto job=std::make_unique<Job>();job->kind=Kind::License;
        HRESULT hr=XodusStoreContextRetain(handle,&job->context);
        phase(Kind::License,"context_retain",hr,!async->queue);
        if(FAILED(hr))return hr;
        return begin(std::move(job),async);
    } catch(const std::bad_alloc&) {return E_OUTOFMEMORY;}
}
HRESULT XodusStoreQueryGameLicenseResult(XAsyncBlock *async,XStoreGameLicense *out) {
    if(out)std::memset(out,0,sizeof(*out));
    HRESULT hr=get_result(Kind::License,async,out,sizeof(*out));
    static std::atomic<unsigned> calls{0};
    if(calls.fetch_add(1)<32)std::fprintf(stderr,"[xodus-store-query] GameLicenseResult hr=%08lx\n",static_cast<ULONG>(hr));
    return hr;
}
HRESULT XodusStoreQueryLicenseTokenAsync(XStoreContextHandle handle,const char **ids,SIZE_T count,
    const char *custom,XAsyncBlock *async) {
    if(!async||(!ids&&count)||!custom)return E_POINTER;
    if(count>max_product_count)return E_INVALIDARG;
    SIZE_T length=strnlen(custom,max_challenge_size+1);
    static std::atomic<unsigned> calls{0};
    if(calls.fetch_add(1)<16)std::fprintf(stderr,"[xodus-store-query] LicenseTokenAsync product_count=%llu challenge_bytes=%llu\n",
        static_cast<unsigned long long>(count),static_cast<unsigned long long>(length));
    if(!length||length>max_challenge_size||!MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,custom,length,nullptr,0))return E_INVALIDARG;
    for(SIZE_T i=0;i<length;++i)if(static_cast<unsigned char>(custom[i])<32&&custom[i]!='\t'&&custom[i]!='\n'&&custom[i]!='\r')return E_INVALIDARG;
    try {
        auto job=std::make_unique<Job>();job->kind=Kind::LicenseToken;
        job->product_ids.reserve(count);
        for(SIZE_T i=0;i<count;++i){
            if(!ids[i])return E_POINTER;
            if(strnlen(ids[i],13)!=12)return E_INVALIDARG;
            for(unsigned j=0;j<12;++j)if(!((ids[i][j]>='A'&&ids[i][j]<='Z')||(ids[i][j]>='0'&&ids[i][j]<='9')))return E_INVALIDARG;
            job->product_ids.emplace_back(ids[i]);
        }
        job->custom.assign(custom,length);
        HRESULT hr=XodusStoreContextRetain(handle,&job->context);
        phase(Kind::LicenseToken,"context_retain",hr,!async->queue);
        if(FAILED(hr))return hr;
        return begin(std::move(job),async);
    } catch(const std::bad_alloc&) {return E_OUTOFMEMORY;}
}
HRESULT XodusStoreQueryLicenseTokenResultSize(XAsyncBlock *async,SIZE_T *out) {
    if(out)*out=0;
    if(!async||!out)return E_POINTER;
    SIZE_T size=0;HRESULT hr=XAsyncGetResultSize(async,&size);
    if(SUCCEEDED(hr)){
        std::lock_guard<std::mutex> lock(jobs_mutex);
        auto it=jobs.find(async);
        if(size<2||size>max_token_size||it==jobs.end()||it->second->kind!=Kind::LicenseToken||it->second->token_size!=size)
            hr=E_INVALIDARG;
        else *out=size;
    }
    static std::atomic<unsigned> calls{0};
    if(calls.fetch_add(1)<32)std::fprintf(stderr,"[xodus-store-query] LicenseTokenResultSize hr=%08lx\n",static_cast<ULONG>(hr));
    return hr;
}
HRESULT XodusStoreQueryLicenseTokenResult(XAsyncBlock *async,SIZE_T capacity,char *out) {
    if(out&&capacity)*out=0;
    if(!async||!out)return E_POINTER;
    SIZE_T required=0;HRESULT hr=XodusStoreQueryLicenseTokenResultSize(async,&required);
    if(SUCCEEDED(hr))hr=capacity<required?HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER):
        XAsyncGetResult(async,&token_identity,capacity,out,nullptr);
    static std::atomic<unsigned> calls{0};
    if(calls.fetch_add(1)<32)std::fprintf(stderr,"[xodus-store-query] LicenseTokenResult hr=%08lx\n",static_cast<ULONG>(hr));
    return hr;
}
HRESULT XodusStoreQueryEntitledProductsAsync(XStoreContextHandle handle,XStoreProductKind kinds,UINT32 page_size,XAsyncBlock *async) {
    static std::atomic<unsigned> calls{0};
    if(calls.fetch_add(1)<16)std::fprintf(stderr,"[xodus-store-query] EntitledProductsAsync kinds=%u page_size=%u\n",static_cast<UINT32>(kinds),page_size);
    if(!async)return E_POINTER;
    if(!page_size||!static_cast<UINT32>(kinds)||(static_cast<UINT32>(kinds)&~0x1fu))return E_INVALIDARG;
    try {
        auto job=std::make_unique<Job>();job->kind=Kind::Products;job->kinds=static_cast<UINT32>(kinds);job->page_size=page_size;
        HRESULT hr=XodusStoreContextRetain(handle,&job->context);
        phase(Kind::Products,"context_retain",hr,!async->queue);
        if(FAILED(hr))return hr;
        return begin(std::move(job),async);
    } catch(const std::bad_alloc&) {return E_OUTOFMEMORY;}
}
HRESULT XodusStoreQueryEntitledProductsResult(XAsyncBlock *async,XStoreProductQueryHandle *out) {
    if(out)*out=nullptr;
    HRESULT hr=get_result(Kind::Products,async,out,sizeof(*out));
    static std::atomic<unsigned> calls{0};
    if(calls.fetch_add(1)<32)std::fprintf(stderr,"[xodus-store-query] EntitledProductsResult hr=%08lx\n",static_cast<ULONG>(hr));
    return hr;
}
HRESULT XodusStoreQueryProductsAsync(XStoreContextHandle handle,XStoreProductKind kinds,
    const char **ids,SIZE_T count,const char **actions,SIZE_T action_count,XAsyncBlock *async) {
    if(!async||(!ids&&count)||(!actions&&action_count))return E_POINTER;
    if(count>max_product_count||!static_cast<UINT32>(kinds)||(static_cast<UINT32>(kinds)&~0x1fu))return E_INVALIDARG;
    // A bounded supported subset, not a claim about an undocumented GDK limit.
    if(action_count>64)return E_NOTIMPL;
    try {
        auto job=std::make_unique<Job>();job->kind=Kind::ExplicitProducts;job->explicit_products=true;
        job->kinds=static_cast<UINT32>(kinds);job->page_size=static_cast<UINT32>(count);
        for(SIZE_T i=0;i<count;++i) {
            if(!ids[i])return E_POINTER;
            SIZE_T length=strnlen(ids[i],18);
            if(length!=12&&length!=17)return E_INVALIDARG;
            for(SIZE_T n=0;n<length;++n) {
                unsigned char c=ids[i][n];
                if(n==12&&length==17){if(c!='/')return E_INVALIDARG;}
                else if(!((c>='A'&&c<='Z')||(c>='0'&&c<='9')))return E_INVALIDARG;
            }
            job->product_ids.emplace_back(ids[i],length);
        }
        for(SIZE_T i=0;i<action_count;++i) {
            if(!actions[i])return E_POINTER;
            SIZE_T length=strnlen(actions[i],17);
            if(!length||length>16)return E_INVALIDARG;
            std::string action(actions[i],length);
            if(action!="Purchase"&&action!="License"&&action!="Fulfill"&&action!="Browse"&&
                action!="Curate"&&action!="Details"&&action!="Redeem")return E_NOTIMPL;
            job->action_filters.push_back(std::move(action));
        }
        HRESULT hr=XodusStoreContextRetain(handle,&job->context);
        if(FAILED(hr))return hr;
        return begin(std::move(job),async);
    } catch(const std::bad_alloc&) {return E_OUTOFMEMORY;}
}
HRESULT XodusStoreQueryProductsResult(XAsyncBlock *async,XStoreProductQueryHandle *out) {
    if(out)*out=nullptr;
    return get_result(Kind::ExplicitProducts,async,out,sizeof(*out));
}
HRESULT XodusStoreQueryConsumableBalanceRemainingAsync(XStoreContextHandle handle,const char *id,XAsyncBlock *async) {
    if(!async||!id)return E_POINTER;
    if(strnlen(id,13)!=12 || !std::all_of(id,id+12,[](char c){return (c>='A'&&c<='Z')||(c>='0'&&c<='9');}))
        return E_INVALIDARG;
    try {
        auto job=std::make_unique<Job>();job->kind=Kind::Balance;job->product_ids.emplace_back(id);
        HRESULT hr=XodusStoreContextRetain(handle,&job->context);
        if(FAILED(hr))return hr;
        return begin(std::move(job),async);
    } catch(const std::bad_alloc&) {return E_OUTOFMEMORY;}
}
HRESULT XodusStoreQueryConsumableBalanceRemainingResult(XAsyncBlock *async,XStoreConsumableResult *out) {
    if(out)*out={};
    return get_result(Kind::Balance,async,out,sizeof(*out));
}
HRESULT XodusStoreAcquireLicenseForDurablesAsync(XStoreContextHandle handle,const char *id,XAsyncBlock *async) {
    if(!async||!id)return E_POINTER;
    if(strnlen(id,13)!=12||!std::all_of(id,id+12,[](char c){return(c>='A'&&c<='Z')||(c>='0'&&c<='9');}))return E_INVALIDARG;
    try {
        auto job=std::make_unique<Job>();job->kind=Kind::Durable;job->product_ids.emplace_back(id);
        HRESULT hr=XodusStoreContextRetain(handle,&job->context);if(FAILED(hr))return hr;
        return begin(std::move(job),async);
    }catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
}
HRESULT XodusStoreAcquireLicenseForDurablesResult(XAsyncBlock *async,XStoreLicenseHandle *out) {
    if(out)*out=nullptr;
    return get_result(Kind::Durable,async,out,sizeof(*out));
}
HRESULT XodusStoreEnumerateProductsQuery(XStoreProductQueryHandle handle,void *context,XStoreProductQueryCallback *callback) {
    if(!callback)return E_POINTER;
    auto page=retain_page(handle);
    if(!page)return E_HANDLE;
    for(UINT32 i=0;i<page->data->product_count&&!page->closed.load();++i)
        if(!callback(&page->data->products[i],context))break;
    return S_OK;
}
BOOLEAN XodusStoreProductsQueryHasMorePages(XStoreProductQueryHandle handle) {
    auto page=retain_page(handle);
    return page&&!page->closed.load()&&page->data->continuation&&*page->data->continuation;
}
HRESULT XodusStoreProductsQueryNextPageAsync(XStoreProductQueryHandle handle,XAsyncBlock *async) {
    if(!async)return E_POINTER;
    auto page=retain_page(handle);
    if(!page)return E_HANDLE;
    if(!page->data->continuation||!*page->data->continuation)return HRESULT_FROM_WIN32(ERROR_NO_MORE_ITEMS);
    try {
        auto job=std::make_unique<Job>();job->kind=Kind::NextPage;job->context=page->context;
        job->kinds=page->kinds;job->page_size=page->page_size;job->continuation=page->data->continuation;
        job->explicit_products=page->explicit_products;
        job->product_ids=page->product_ids;job->action_filters=page->action_filters;
        return begin(std::move(job),async);
    } catch(const std::bad_alloc&) {return E_OUTOFMEMORY;}
}
HRESULT XodusStoreProductsQueryNextPageResult(XAsyncBlock *async,XStoreProductQueryHandle *out) {
    if(out)*out=nullptr;
    return get_result(Kind::NextPage,async,out,sizeof(*out));
}
void XodusStoreCloseProductsQueryHandle(XStoreProductQueryHandle handle) {
    std::shared_ptr<Page> detached;
    {
        std::lock_guard<std::mutex> lock(pages_mutex);
        auto it=pages.find(handle);if(it==pages.end())return;
        detached=std::move(it->second);detached->closed=true;pages.erase(it);
    }
}
void XodusStoreQueriesContextClosed(void *context) {
    std::lock_guard<std::mutex> lock(jobs_mutex);
    for(auto &entry:jobs)
        if(entry.second->context.get()==context)InterlockedExchange(&entry.second->cancelled,1);
}
void XodusStoreQueriesShutdown() {
    stopped=true;
    decltype(pages) detached;
    {
        std::lock_guard<std::mutex> lock(pages_mutex);
        for(auto &entry:pages)entry.second->closed=true;
        pages.swap(detached);
    }
    {
        std::lock_guard<std::mutex> lock(jobs_mutex);
        for(auto &entry:jobs)InterlockedExchange(&entry.second->cancelled,1);
    }
}

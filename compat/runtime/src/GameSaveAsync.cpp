/* SPDX-License-Identifier: LGPL-2.1-or-later */
#define __WINESRC__ 1
#include "GameSaveAsync.h"
#include "GameSaveBridge.h"
#include "GameSaveLocalCore.h"
#include <xasyncprovider.h>
#include <algorithm>
#include <cstring>
#include <memory>
#include <mutex>
#include <unordered_map>

namespace {
enum class Kind {Initialize,Quota,Delete,Read,Submit};
const char identities[5]={};
std::atomic<bool> stopped{false};
struct Job {
    Kind kind;
    XAsyncBlock* key=nullptr;
    local_save::Handle handle=nullptr;
    std::shared_ptr<local_save::Retained> retained;
    std::shared_ptr<local_save::PendingMutation> mutation;
    uint64_t quota_sequence=0;
    local_save::Cancellation cancel;
    XUserHandle user=nullptr;
    std::string name;
    bool sync=false,all_blobs=false;
    std::vector<std::string> names;
    std::vector<local_save::Blob> blobs;
    XGameSaveProviderHandle provider=nullptr;
    int64_t quota=0;
    size_t result_bytes=0;
    ~Job(){if(provider)local_save::close_provider(provider);if(user)XodusGameSaveCloseUser(user);}
};
std::mutex jobs_mutex;
std::unordered_map<XAsyncBlock*,Job*> jobs;
const void* identity(Kind k){return &identities[(unsigned)k];}
bool aborted(Job* j){return stopped.load()||j->cancel.cancelled.load()||(j->kind!=Kind::Initialize&&!local_save::is_open(j->retained));}
HRESULT work(Job* j) {
    if(aborted(j))return E_ABORT;
    HRESULT hr;
    switch(j->kind) {
    case Kind::Initialize:
        hr=XodusGameSaveInitializeProvider(j->user,j->name.c_str(),j->sync,&j->provider);
        j->result_bytes=sizeof(j->provider);
        if(SUCCEEDED(hr)&&aborted(j))return E_ABORT;
        return hr;
    case Kind::Quota:
        hr=local_save::remaining_quota_at(j->handle,j->quota_sequence,&j->quota);j->result_bytes=sizeof(j->quota);return hr;
    case Kind::Delete:
        j->result_bytes=1;return local_save::delete_container(j->handle,j->name.c_str(),&j->cancel);
    case Kind::Read:
        hr=local_save::read_blobs(j->handle,j->all_blobs?nullptr:&j->names,&j->blobs,&j->cancel);
        if(FAILED(hr))return hr;
        return XodusGameSavePackedBlobSize(j->blobs,&j->result_bytes);
    case Kind::Submit:
        j->result_bytes=1;return local_save::submit_update(j->handle,&j->cancel);
    }
    return E_UNEXPECTED;
}
HRESULT WINAPI callback(XAsyncOp op,const XAsyncProviderData* data) {
    auto j=static_cast<Job*>(data->context);
    switch(op) {
    case XAsyncOp::Begin:return XAsyncSchedule(data->async,0);
    case XAsyncOp::DoWork: {
        HRESULT hr;
        try{hr=work(j);}catch(const std::bad_alloc&){hr=E_OUTOFMEMORY;}catch(...){hr=E_FAIL;}
        if(hr==E_PENDING&&j->kind==Kind::Quota) {
            // Do not occupy a worker while an earlier write waits on another
            // queue (or later dispatch). Later submissions are not our barrier.
            HRESULT scheduled=XAsyncSchedule(data->async,10);
            if(SUCCEEDED(scheduled))return E_PENDING;
            hr=scheduled;
        }
        j->mutation.reset();
        // Preserve a successful transaction after its atomic commit point.
        XAsyncComplete(data->async,hr,SUCCEEDED(hr)?std::max<size_t>(1,j->result_bytes):0);
        return E_PENDING;
    }
    case XAsyncOp::GetResult:
        if(j->kind==Kind::Initialize) {
            if(aborted(j))return E_ABORT;
            *static_cast<XGameSaveProviderHandle*>(data->buffer)=j->provider;j->provider=nullptr;
        } else if(j->kind==Kind::Quota) {
            if(aborted(j))return E_ABORT;std::memcpy(data->buffer,&j->quota,sizeof(j->quota));
        } else if(j->kind==Kind::Read) {
            if(aborted(j))return E_ABORT;
            uint32_t count=0;return XodusGameSavePackBlobs(j->blobs,data->bufferSize,static_cast<XGameSaveBlob*>(data->buffer),&count);
        } else *static_cast<uint8_t*>(data->buffer)=0;
        return S_OK;
    case XAsyncOp::Cancel:j->cancel.cancelled=true;return S_OK;
    case XAsyncOp::Cleanup:
        {std::lock_guard<std::mutex> lock(jobs_mutex);auto it=jobs.find(j->key);if(it!=jobs.end()&&it->second==j)jobs.erase(it);}
        delete j;return S_OK;
    }
    return E_NOTIMPL;
}
HRESULT begin(std::unique_ptr<Job> j,XAsyncBlock* async) {
    if(!async)return E_POINTER;if(stopped.load())return E_ABORT;
    j->key=async;
    {std::lock_guard<std::mutex> lock(jobs_mutex);if(jobs.count(async))return E_INVALIDARG;jobs.emplace(async,j.get());}
    Job* raw=j.release();HRESULT hr=XAsyncBegin(async,raw,identity(raw->kind),"XodusLocalGameSave",callback);
    if(FAILED(hr)) {
        Job* detached=nullptr;
        {std::lock_guard<std::mutex> lock(jobs_mutex);auto it=jobs.find(async);if(it!=jobs.end()&&it->second==raw){detached=it->second;jobs.erase(it);}}
        delete detached;
    }
    return hr;
}
template<class F> HRESULT safe(F f) noexcept {try{return f();}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return E_FAIL;}}
HRESULT from_handle(Kind kind,void* h,const char* name,XAsyncBlock* async) {
    if(!async)return E_POINTER;
    return safe([&]()->HRESULT{auto j=std::make_unique<Job>();j->kind=kind;j->handle=h;j->retained=local_save::retain(h);if(!local_save::is_open(j->retained))return local_save::handle_expired;
        if(name){size_t n=strnlen(name,257);if(n>256)return local_save::invalid_name;j->name.assign(name,n);}
        HRESULT hr=kind==Kind::Quota?local_save::quota_barrier(h,&j->quota_sequence):local_save::register_mutation(h,&j->mutation);
        if(FAILED(hr))return hr;return begin(std::move(j),async);});
}
HRESULT preflight(Kind kind,XAsyncBlock* async,size_t* required,uint32_t* count=nullptr) {
    if(!async||!required)return E_POINTER;
    HRESULT hr=XAsyncGetResultSize(async,required);if(FAILED(hr))return hr;
    std::lock_guard<std::mutex> lock(jobs_mutex);auto it=jobs.find(async);
    if(it==jobs.end()||it->second->kind!=kind||*required!=std::max<size_t>(1,it->second->result_bytes))return E_INVALIDARG;
    if(count)*count=(uint32_t)it->second->blobs.size();return S_OK;
}
HRESULT scalar_result(Kind kind,XAsyncBlock* async,void* out,size_t size) {
    if(!out)return E_POINTER;size_t required=0;HRESULT hr=preflight(kind,async,&required);
    if(FAILED(hr))return hr;if(required!=size)return E_INVALIDARG;
    return XAsyncGetResult(async,identity(kind),size,out,nullptr);
}
}

HRESULT WINAPI XodusGameSaveInitializeProviderAsync(XUserHandle user,const char* scid,bool sync,XAsyncBlock* async) {
    if(!async||!scid)return E_POINTER;
    return safe([&]()->HRESULT{size_t n=strnlen(scid,39);if(!n||n>38)return E_INVALIDARG;
        auto j=std::make_unique<Job>();j->kind=Kind::Initialize;j->sync=sync;j->name.assign(scid,n);
        HRESULT hr=XodusGameSaveDuplicateUser(user,&j->user);if(FAILED(hr))return hr;return begin(std::move(j),async);});
}
HRESULT WINAPI XodusGameSaveInitializeProviderResult(XAsyncBlock* async,XGameSaveProviderHandle* out) {
    if(out)*out=nullptr;return scalar_result(Kind::Initialize,async,out,sizeof(*out));
}
HRESULT WINAPI XodusGameSaveGetRemainingQuotaAsync(XGameSaveProviderHandle provider,XAsyncBlock* async){return from_handle(Kind::Quota,provider,nullptr,async);}
HRESULT WINAPI XodusGameSaveGetRemainingQuotaResult(XAsyncBlock* async,int64_t* out){if(out)*out=0;return scalar_result(Kind::Quota,async,out,sizeof(*out));}
HRESULT WINAPI XodusGameSaveDeleteContainerAsync(XGameSaveProviderHandle provider,const char* name,XAsyncBlock* async){if(!name)return E_POINTER;return from_handle(Kind::Delete,provider,name,async);}
HRESULT WINAPI XodusGameSaveDeleteContainerResult(XAsyncBlock* async){uint8_t unused=0;return scalar_result(Kind::Delete,async,&unused,1);}
HRESULT WINAPI XodusGameSaveSubmitUpdateAsync(XGameSaveUpdateHandle update,XAsyncBlock* async){return from_handle(Kind::Submit,update,nullptr,async);}
HRESULT WINAPI XodusGameSaveSubmitUpdateResult(XAsyncBlock* async){uint8_t unused=0;return scalar_result(Kind::Submit,async,&unused,1);}
HRESULT WINAPI XodusGameSaveReadBlobDataAsync(XGameSaveContainerHandle container,const char** names,uint32_t count,XAsyncBlock* async) {
    if(!async)return E_POINTER;if(count>65536)return E_INVALIDARG;
    return safe([&]()->HRESULT{auto j=std::make_unique<Job>();j->kind=Kind::Read;j->handle=container;j->retained=local_save::retain(container);
        if(!local_save::is_open(j->retained))return local_save::handle_expired;j->all_blobs=!names;
        if(names){j->names.reserve(count);for(uint32_t i=0;i<count;i++){if(!names[i])return E_POINTER;size_t n=strnlen(names[i],257);if(n>256)return local_save::invalid_name;j->names.emplace_back(names[i],n);}}
        return begin(std::move(j),async);});
}
HRESULT WINAPI XodusGameSaveReadBlobDataResult(XAsyncBlock* async,size_t capacity,XGameSaveBlob* out,uint32_t* count) {
    if(!count)return E_POINTER;*count=0;size_t required=0;uint32_t actual=0;HRESULT hr=preflight(Kind::Read,async,&required,&actual);
    if(FAILED(hr))return hr;
    if(actual) {if(!out)return E_POINTER;if(capacity<required)return local_save::buffer_too_small;if(reinterpret_cast<uintptr_t>(out)%alignof(XGameSaveBlob))return E_INVALIDARG;}
    uint8_t empty=0;hr=XAsyncGetResult(async,identity(Kind::Read),actual?capacity:1,actual?static_cast<void*>(out):&empty,nullptr);
    if(SUCCEEDED(hr))*count=actual;return hr;
}
void XodusGameSaveAsyncShutdown() {
    stopped=true;std::lock_guard<std::mutex> lock(jobs_mutex);for(auto& j:jobs)j.second->cancel.cancelled=true;
}
#ifdef XODUS_GAMESAVE_TESTING
size_t XodusGameSaveTestLiveJobs(){std::lock_guard<std::mutex> lock(jobs_mutex);return jobs.size();}
#endif

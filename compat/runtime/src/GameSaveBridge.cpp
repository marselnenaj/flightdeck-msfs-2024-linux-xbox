/* SPDX-License-Identifier: LGPL-2.1-or-later */
// Local-only opt-in adapter. It never reads, uploads, or claims cloud saves.
#define __WINESRC__
#include "GameSaveBridge.h"
#include "GameSaveAsync.h"
#include <bcrypt.h>
#include <algorithm>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <limits>
#include <mutex>
#include <new>

// Existing private User bridge. No independent authentication/account selection.
HRESULT QueryUserRuntime(const GUID*,REFIID,void**);

namespace {
std::atomic<bool> stopped{false};
std::mutex user_api_mutex;
IXUserImpl* retained_user_api=nullptr;
// The proxy's process-lifetime runtime holds this one reference. Queued init
// cleanup must be able to release its duplicated User after runtime shutdown.
HRESULT user_api(IXUserImpl** out) {
    *out=nullptr;
    std::lock_guard<std::mutex> guard(user_api_mutex);
    if(!retained_user_api) {
        const GUID id=__uuidof(IXUserImpl);
        HRESULT hr=QueryUserRuntime(&id,id,reinterpret_cast<void**>(&retained_user_api));
        if(FAILED(hr))return hr;
        if(!retained_user_api)return E_UNEXPECTED;
    }
    *out=retained_user_api;return S_OK;
}
template<class Work> HRESULT safe(Work work) {
    try{return work();}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return E_UNEXPECTED;}
}
bool valid_scid(const char* value,std::string& result) {
    result.clear();if(!value)return false;
    size_t length=0;while(length<37&&value[length])++length;
    if(length!=36)return false;
    result.resize(36);
    for(size_t i=0;i<36;++i) {
        unsigned char c=static_cast<unsigned char>(value[i]);
        if(i==8||i==13||i==18||i==23) {if(c!='-')return false;}
        else if(c>='A'&&c<='F')c=static_cast<unsigned char>(c+'a'-'A');
        else if(!((c>='a'&&c<='f')||(c>='0'&&c<='9')))return false;
        result[i]=static_cast<char>(c);
    }
    return true;
}
HRESULT actual_title(UINT32* result) {
    *result=0;
    static const GUID id={0x973a344e,0x24bf,0x4d0f,{0x84,0x57,0x56,0xc5,0x34,0x89,0x2b,0x29}};
    XodusGameSaveTitleRuntime* title=nullptr;
    HRESULT hr=QueryUserRuntime(&id,id,reinterpret_cast<void**>(&title));
    if(FAILED(hr))return hr;
    if(!title)return E_UNEXPECTED;
    hr=title->XGameGetXboxTitleId(result);title->Release();
    return SUCCEEDED(hr)&&!*result?HRESULT_FROM_WIN32(ERROR_INVALID_DATA):hr;
}
HRESULT make_namespace(UINT32 title,const std::string& scid,bool has_user,UINT64 user,std::string& output) {
    output.clear();
    BCRYPT_ALG_HANDLE algorithm=nullptr;
    BCRYPT_HASH_HANDLE hash=nullptr;
    const auto status_hr=[](NTSTATUS status){return HRESULT_FROM_NT(status);};
    NTSTATUS status=BCryptOpenAlgorithmProvider(&algorithm,BCRYPT_SHA256_ALGORITHM,nullptr,0);
    if(status<0)return status_hr(status);
    DWORD bytes=0,object_size=0;
    status=BCryptGetProperty(algorithm,BCRYPT_OBJECT_LENGTH,reinterpret_cast<PUCHAR>(&object_size),sizeof(object_size),&bytes,0);
    std::vector<UCHAR> object;
    if(status>=0) {
        if(!object_size||object_size>1024*1024||bytes!=sizeof(object_size))status=static_cast<NTSTATUS>(0xc000000d);
        else try{object.resize(object_size);}catch(...){BCryptCloseAlgorithmProvider(algorithm,0);return E_OUTOFMEMORY;}
    }
    if(status>=0)status=BCryptCreateHash(algorithm,&hash,object.data(),object_size,nullptr,0,0);
    static const UCHAR label[]="xodus.localgamesave.namespace.v1";
    UCHAR title_bytes[4],user_bytes[9]={static_cast<UCHAR>(has_user)};
    for(unsigned i=0;i<4;++i)title_bytes[3-i]=static_cast<UCHAR>(title>>(8*i));
    for(unsigned i=0;i<8;++i)user_bytes[8-i]=static_cast<UCHAR>(user>>(8*i));
    if(status>=0)status=BCryptHashData(hash,const_cast<PUCHAR>(label),sizeof(label),0);
    if(status>=0)status=BCryptHashData(hash,title_bytes,sizeof(title_bytes),0);
    if(status>=0)status=BCryptHashData(hash,reinterpret_cast<PUCHAR>(const_cast<char*>(scid.data())),static_cast<ULONG>(scid.size()),0);
    if(status>=0)status=BCryptHashData(hash,user_bytes,sizeof(user_bytes),0);
    UCHAR digest[32]={};
    if(status>=0)status=BCryptFinishHash(hash,digest,sizeof(digest),0);
    SecureZeroMemory(user_bytes,sizeof(user_bytes));
    if(hash)BCryptDestroyHash(hash);
    if(!object.empty())SecureZeroMemory(object.data(),object.size());
    BCryptCloseAlgorithmProvider(algorithm,0);
    if(status<0)return status_hr(status);
    static const char hex[]="0123456789abcdef";
    output.resize(64);
    for(unsigned i=0;i<32;++i){output[2*i]=hex[digest[i]>>4];output[2*i+1]=hex[digest[i]&15];}
    SecureZeroMemory(digest,sizeof(digest));return S_OK;
}
HRESULT local_options(XUserHandle user,const char* configuration,local_save::Options* out) {
    if(stopped.load())return E_ABORT;
    if(!XodusGameSaveLocalEnabled())return E_NOTIMPL;
    std::string scid;if(!valid_scid(configuration,scid))return E_INVALIDARG;
    DWORD size=GetEnvironmentVariableW(L"XODUS_LOCAL_GAMESAVE_ROOT",nullptr,0);
    if(size<4||size>32768)return E_INVALIDARG;
    std::vector<wchar_t> path(size);
    DWORD copied=GetEnvironmentVariableW(L"XODUS_LOCAL_GAMESAVE_ROOT",path.data(),size);
    if(!copied||copied>=size)return E_INVALIDARG;
    // Explicit existing local DOS path only: no relative cwd or remote UNC root.
    if(!((path[0]>='A'&&path[0]<='Z')||(path[0]>='a'&&path[0]<='z'))||path[1]!=L':'||(path[2]!=L'\\'&&path[2]!=L'/'))return E_INVALIDARG;
    const wchar_t drive[]={path[0],L':',L'\\',L'\0'};
    const UINT drive_type=GetDriveTypeW(drive);
    if(drive_type!=DRIVE_FIXED&&drive_type!=DRIVE_REMOVABLE&&drive_type!=DRIVE_RAMDISK)return E_INVALIDARG;
    DWORD attributes=GetFileAttributesW(path.data());
    if(attributes==INVALID_FILE_ATTRIBUTES)return HRESULT_FROM_WIN32(GetLastError());
    if(!(attributes&FILE_ATTRIBUTE_DIRECTORY))return HRESULT_FROM_WIN32(ERROR_DIRECTORY);
    UINT32 title=0;HRESULT hr=actual_title(&title);if(FAILED(hr))return hr;
    UINT64 id=0;
    if(user) {
        IXUserImpl* api=nullptr;if(FAILED(hr=user_api(&api)))return hr;
        if(FAILED(hr=api->XUserGetId(user,&id)))return hr;
        if(!id)return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
    }
    out->root.assign(path.data(),copied);
    hr=make_namespace(title,scid,user!=nullptr,id,out->namespace_key);
    SecureZeroMemory(&id,sizeof(id));if(FAILED(hr))return hr;
    out->enabled=true;return stopped.load()?E_ABORT:S_OK;
}
HRESULT enumerate_containers(XGameSaveProviderHandle provider,const char* filter,bool exact,void* context,XGameSaveContainerInfoCallback* callback) {
    if(!callback)return E_POINTER;
    return safe([&]()->HRESULT {
        std::vector<local_save::ContainerInfo> items;
        HRESULT hr=local_save::container_info(provider,filter,exact,&items);if(FAILED(hr))return hr;
        for(const auto& item:items) {
            XGameSaveContainerInfo info{item.name.c_str(),item.display_name.c_str(),item.blob_count,item.total_size,static_cast<time_t>(item.last_modified),false};
            if(!callback(&info,context))break;
        }
        return S_OK;
    });
}
HRESULT enumerate_blobs(XGameSaveContainerHandle container,const char* filter,void* context,XGameSaveBlobInfoCallback* callback) {
    if(!callback)return E_POINTER;
    return safe([&]()->HRESULT {
        std::vector<local_save::BlobInfo> items;
        HRESULT hr=local_save::blob_info(container,filter,&items);if(FAILED(hr))return hr;
        for(const auto& item:items){XGameSaveBlobInfo info{item.name.c_str(),item.size};if(!callback(&info,context))break;}
        return S_OK;
    });
}
class Bridge final : public IXGameSaveImpl3 {
    std::atomic<ULONG> refs{1};
public:
    HRESULT WINAPI QueryInterface(REFIID iid,void** out) override {
        if(!out)return E_POINTER;*out=nullptr;
        if(iid!=IID_IUnknown&&iid!=IID_IXGameSaveImpl&&iid!=IID_IXGameSaveImpl2&&iid!=IID_IXGameSaveImpl3)return E_NOINTERFACE;
        *out=static_cast<IXGameSaveImpl3*>(this);AddRef();return S_OK;
    }
    ULONG WINAPI AddRef() override{return ++refs;}
    ULONG WINAPI Release() override{return --refs;}
    HRESULT WINAPI XGameSaveInitializeProvider(XUserHandle u,const char* c,bool sync,XGameSaveProviderHandle* p) override{return XodusGameSaveInitializeProvider(u,c,sync,p);}
    HRESULT WINAPI XGameSaveInitializeProviderAsync(XUserHandle u,const char* c,bool sync,XAsyncBlock* a) override{return XodusGameSaveInitializeProviderAsync(u,c,sync,a);}
    HRESULT WINAPI XGameSaveInitializeProviderResult(XAsyncBlock* a,XGameSaveProviderHandle* p) override{return XodusGameSaveInitializeProviderResult(a,p);}
    void WINAPI XGameSaveCloseProvider(XGameSaveProviderHandle p) override{local_save::close_provider(p);}
    HRESULT WINAPI XGameSaveGetRemainingQuota(XGameSaveProviderHandle p,int64_t* q) override{return local_save::remaining_quota(p,q);}
    HRESULT WINAPI XGameSaveGetRemainingQuotaAsync(XGameSaveProviderHandle p,XAsyncBlock* a) override{return XodusGameSaveGetRemainingQuotaAsync(p,a);}
    HRESULT WINAPI XGameSaveGetRemainingQuotaResult(XAsyncBlock* a,int64_t* q) override{return XodusGameSaveGetRemainingQuotaResult(a,q);}
    HRESULT WINAPI XGameSaveDeleteContainer(XGameSaveProviderHandle p,const char* n) override{return local_save::delete_container(p,n);}
    HRESULT WINAPI XGameSaveDeleteContainerAsync(XGameSaveProviderHandle p,const char* n,XAsyncBlock* a) override{return XodusGameSaveDeleteContainerAsync(p,n,a);}
    HRESULT WINAPI XGameSaveDeleteContainerResult(XAsyncBlock* a) override{return XodusGameSaveDeleteContainerResult(a);}
    HRESULT WINAPI XGameSaveGetContainerInfo(XGameSaveProviderHandle p,const char* n,void* c,XGameSaveContainerInfoCallback* f) override{if(!n)return E_POINTER;return enumerate_containers(p,n,true,c,f);}
    HRESULT WINAPI XGameSaveEnumerateContainerInfo(XGameSaveProviderHandle p,void* c,XGameSaveContainerInfoCallback* f) override{return enumerate_containers(p,nullptr,false,c,f);}
    HRESULT WINAPI XGameSaveEnumerateContainerInfoByName(XGameSaveProviderHandle p,const char* n,void* c,XGameSaveContainerInfoCallback* f) override{if(!n)return E_POINTER;return enumerate_containers(p,n,false,c,f);}
    HRESULT WINAPI XGameSaveCreateContainer(XGameSaveProviderHandle p,const char* n,XGameSaveContainerHandle* out) override{
        if(!out)return E_POINTER;*out=nullptr;local_save::Handle h=nullptr;HRESULT hr=local_save::create_container(p,n,&h);if(SUCCEEDED(hr))*out=static_cast<XGameSaveContainerHandle>(h);return hr;}
    void WINAPI XGameSaveCloseContainer(XGameSaveContainerHandle c) override{local_save::close_container(c);}
    HRESULT WINAPI XGameSaveEnumerateBlobInfo(XGameSaveContainerHandle c,void* p,XGameSaveBlobInfoCallback* f) override{return enumerate_blobs(c,nullptr,p,f);}
    HRESULT WINAPI XGameSaveEnumerateBlobInfoByName(XGameSaveContainerHandle c,const char* n,void* p,XGameSaveBlobInfoCallback* f) override{if(!n)return E_POINTER;return enumerate_blobs(c,n,p,f);}
    HRESULT WINAPI XGameSaveReadBlobData(XGameSaveContainerHandle c,const char** names,uint32_t* count,size_t bytes,XGameSaveBlob* out) override{
        if(!count)return E_POINTER;const uint32_t requested=*count;*count=0;
        return safe([&]()->HRESULT{std::vector<std::string> copied;std::vector<local_save::Blob> blobs;HRESULT hr=XodusGameSaveCopyBlobNames(names,requested,&copied);if(FAILED(hr))return hr;
            hr=local_save::read_blobs(c,names?&copied:nullptr,&blobs);return FAILED(hr)?hr:XodusGameSavePackBlobs(blobs,bytes,out,count);});}
    HRESULT WINAPI XGameSaveReadBlobDataAsync(XGameSaveContainerHandle c,const char** n,uint32_t count,XAsyncBlock* a) override{return XodusGameSaveReadBlobDataAsync(c,n,count,a);}
    HRESULT WINAPI XGameSaveReadBlobDataResult(XAsyncBlock* a,size_t s,XGameSaveBlob* b,uint32_t* n) override{return XodusGameSaveReadBlobDataResult(a,s,b,n);}
    HRESULT WINAPI XGameSaveCreateUpdate(XGameSaveContainerHandle c,const char* n,XGameSaveUpdateHandle* out) override{
        if(!out)return E_POINTER;*out=nullptr;local_save::Handle h=nullptr;HRESULT hr=local_save::create_update(c,n,&h);if(SUCCEEDED(hr))*out=static_cast<XGameSaveUpdateHandle>(h);return hr;}
    void WINAPI XGameSaveCloseUpdate(XGameSaveUpdateHandle u) override{local_save::close_update(u);}
    HRESULT WINAPI XGameSaveSubmitBlobWrite(XGameSaveUpdateHandle u,const char* n,const uint8_t* p,size_t s) override{return local_save::write_blob(u,n,p,s);}
    HRESULT WINAPI XGameSaveSubmitBlobDelete(XGameSaveUpdateHandle u,const char* n) override{return local_save::delete_blob(u,n);}
    HRESULT WINAPI XGameSaveSubmitUpdate(XGameSaveUpdateHandle u) override{return local_save::submit_update(u);}
    HRESULT WINAPI XGameSaveSubmitUpdateAsync(XGameSaveUpdateHandle u,XAsyncBlock* a) override{return XodusGameSaveSubmitUpdateAsync(u,a);}
    HRESULT WINAPI XGameSaveSubmitUpdateResult(XAsyncBlock* a) override{return XodusGameSaveSubmitUpdateResult(a);}
    HRESULT WINAPI XGameSaveFilesGetFolderWithUiAsync(XUserHandle,const char*,XAsyncBlock*) override{return E_NOTIMPL;}
    HRESULT WINAPI XGameSaveFilesGetFolderWithUiResult(XAsyncBlock*,size_t,char*) override{return E_NOTIMPL;}
    HRESULT WINAPI XGameSaveFilesGetRemainingQuota(XUserHandle,const char*,int64_t* q) override{if(!q)return E_POINTER;*q=0;return E_NOTIMPL;}
};
Bridge bridge;
}

bool XodusGameSaveLocalEnabled() {
    char value[2]={};
    return GetEnvironmentVariableA("XODUS_LOCAL_GAMESAVE",value,sizeof(value))==1&&value[0]=='1';
}
bool IsGameSaveRuntimeClass(const GUID* clsid){return clsid&&*clsid==CLSID_XGameSaveImpl;}
HRESULT QueryGameSaveRuntime(const GUID* clsid,REFIID iid,void** out) {
    if(!out)return E_POINTER;*out=nullptr;
    if(!IsGameSaveRuntimeClass(clsid))return E_NOINTERFACE;
    if(stopped.load())return E_ABORT;
    if(!XodusGameSaveLocalEnabled())return E_NOTIMPL;
    return bridge.QueryInterface(iid,out);
}
void ShutdownGameSaveRuntime(){stopped.store(true);XodusGameSaveAsyncShutdown();local_save::shutdown();}
HRESULT XodusGameSaveDuplicateUser(XUserHandle user,XUserHandle* out) {
    if(!out)return E_POINTER;*out=nullptr;
    if(stopped.load())return E_ABORT;
    if(!XodusGameSaveLocalEnabled())return E_NOTIMPL;
    if(!user)return S_OK;
    IXUserImpl* api=nullptr;HRESULT hr=user_api(&api);return FAILED(hr)?hr:api->XUserDuplicateHandle(user,out);
}
void XodusGameSaveCloseUser(XUserHandle user) {
    if(!user)return;
    IXUserImpl* api=nullptr;
    {std::lock_guard<std::mutex> guard(user_api_mutex);api=retained_user_api;}
    if(api)api->XUserCloseHandle(user);
}
HRESULT WINAPI XodusGameSaveInitializeProvider(XUserHandle user,const char* scid,bool sync,XGameSaveProviderHandle* out) {
    if(!out)return E_POINTER;*out=nullptr;
    HRESULT hr=safe([&]()->HRESULT{local_save::Options options;HRESULT status=local_options(user,scid,&options);if(FAILED(status))return status;
        local_save::Handle handle=nullptr;status=local_save::initialize(options,sync,&handle);
        if(SUCCEEDED(status)&&stopped.load()){local_save::close_provider(handle);return E_ABORT;}
        if(SUCCEEDED(status))*out=static_cast<XGameSaveProviderHandle>(handle);return status;});
    static std::atomic<unsigned> logs{0};if(logs.fetch_add(1)<16)std::fprintf(stderr,"[xodus-gamesave] local_init enabled=%u sync_on_demand=%u hr=%08lx\n",XodusGameSaveLocalEnabled(),sync,static_cast<ULONG>(hr));
    return hr;
}
HRESULT XodusGameSaveCopyBlobNames(const char* const* names,uint32_t count,std::vector<std::string>* out) {
    if(!out)return E_POINTER;out->clear();if(!names)return S_OK;
    if(count>65536)return E_INVALIDARG;
    return safe([&]()->HRESULT{std::vector<std::string> result;result.reserve(count);
        for(uint32_t i=0;i<count;++i){if(!names[i])return E_POINTER;size_t n=0;while(n<=256&&names[i][n])++n;if(!n||n>256)return local_save::invalid_name;result.emplace_back(names[i],n);}
        *out=std::move(result);return S_OK;});
}
HRESULT XodusGameSavePackedBlobSize(const std::vector<local_save::Blob>& blobs,SIZE_T* out) {
    if(!out)return E_POINTER;*out=0;
    if(blobs.size()>UINT32_MAX||blobs.size()>SIZE_MAX/sizeof(XGameSaveBlob))return E_INVALIDARG;
    SIZE_T size=blobs.size()*sizeof(XGameSaveBlob);
    for(const auto& blob:blobs) {
        if(blob.name.find('\0')!=std::string::npos||blob.data.size()>UINT32_MAX)return E_INVALIDARG;
        if(blob.name.size()>=SIZE_MAX-size)return HRESULT_FROM_WIN32(ERROR_ARITHMETIC_OVERFLOW);
        size+=blob.name.size()+1;
        if(blob.data.size()>SIZE_MAX-size)return HRESULT_FROM_WIN32(ERROR_ARITHMETIC_OVERFLOW);
        size+=blob.data.size();
    }
    *out=size;return S_OK;
}
HRESULT XodusGameSavePackBlobs(const std::vector<local_save::Blob>& blobs,SIZE_T capacity,XGameSaveBlob* out,uint32_t* count) {
    if(!count)return E_POINTER;*count=0;SIZE_T required=0;HRESULT hr=XodusGameSavePackedBlobSize(blobs,&required);if(FAILED(hr))return hr;
    if(capacity<required)return local_save::buffer_too_small;
    if(required&&!out)return E_POINTER;
    if(required&&reinterpret_cast<uintptr_t>(out)%alignof(XGameSaveBlob))return E_INVALIDARG;
    if(!required)return S_OK;
    auto cursor=reinterpret_cast<uint8_t*>(out)+blobs.size()*sizeof(XGameSaveBlob);
    for(SIZE_T i=0;i<blobs.size();++i) {
        const auto& blob=blobs[i];out[i].info.name=reinterpret_cast<const char*>(cursor);out[i].info.size=static_cast<uint32_t>(blob.data.size());
        std::memcpy(cursor,blob.name.c_str(),blob.name.size()+1);cursor+=blob.name.size()+1;
        out[i].data=cursor;if(!blob.data.empty())std::memcpy(cursor,blob.data.data(),blob.data.size());cursor+=blob.data.size();
    }
    *count=static_cast<uint32_t>(blobs.size());return S_OK;
}

#include <compat.h>
#include "../../compat/runtime/src/GameSaveBridge.h"
#include "../../compat/runtime/src/GameSaveAsync.h"
#include <cstdio>
#include <cstring>
#include <vector>
#include <string>

static unsigned checks,failures,identity_calls,user_closes,user_duplicates,async_stops;
static UINT32 title_id=12345;
static HRESULT identity_status=S_OK;
static void check(bool okay,const char* name){++checks;failures+=!okay;std::printf("bridge case=%s pass=%u\n",name,unsigned(okay));}
struct SyntheticTitle : XodusGameSaveTitleRuntime {
    HRESULT WINAPI QueryInterface(REFIID,void** out) override{if(!out)return E_POINTER;*out=this;return S_OK;}
    ULONG WINAPI AddRef() override{return 2;} ULONG WINAPI Release() override{return 1;}
    HRESULT WINAPI XGameGetXboxTitleId(UINT32* out) override{*out=title_id;return identity_status;}
} fake_title;
static HRESULT WINAPI duplicate(void*,XUserHandle user,XUserHandle* out){++user_duplicates;*out=user;return S_OK;}
static void WINAPI close_user(void*,XUserHandle){++user_closes;}
static HRESULT WINAPI get_id(void*,XUserHandle user,UINT64* out){*out=reinterpret_cast<uintptr_t>(user);return identity_status;}
static void* user_vtable[12]={nullptr,nullptr,nullptr,reinterpret_cast<void*>(&duplicate),reinterpret_cast<void*>(&close_user),nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,reinterpret_cast<void*>(&get_id)};
struct {void** vtable;} fake_user{user_vtable};
HRESULT QueryUserRuntime(const GUID* id,REFIID,void** out){++identity_calls;*out=nullptr;if(id->Data1==0x973a344e){*out=&fake_title;return S_OK;}if(id->Data1==0x01acd177){*out=&fake_user;return S_OK;}return E_NOINTERFACE;}
// The synchronous adapter is tested independently from Async.cpp.
HRESULT WINAPI XodusGameSaveInitializeProviderAsync(XUserHandle,const char*,bool,XAsyncBlock*){return E_NOTIMPL;}
HRESULT WINAPI XodusGameSaveInitializeProviderResult(XAsyncBlock*,XGameSaveProviderHandle*){return E_NOTIMPL;}
HRESULT WINAPI XodusGameSaveGetRemainingQuotaAsync(XGameSaveProviderHandle,XAsyncBlock*){return E_NOTIMPL;}
HRESULT WINAPI XodusGameSaveGetRemainingQuotaResult(XAsyncBlock*,int64_t*){return E_NOTIMPL;}
HRESULT WINAPI XodusGameSaveDeleteContainerAsync(XGameSaveProviderHandle,const char*,XAsyncBlock*){return E_NOTIMPL;}
HRESULT WINAPI XodusGameSaveDeleteContainerResult(XAsyncBlock*){return E_NOTIMPL;}
HRESULT WINAPI XodusGameSaveReadBlobDataAsync(XGameSaveContainerHandle,const char**,uint32_t,XAsyncBlock*){return E_NOTIMPL;}
HRESULT WINAPI XodusGameSaveReadBlobDataResult(XAsyncBlock*,size_t,XGameSaveBlob*,uint32_t*){return E_NOTIMPL;}
HRESULT WINAPI XodusGameSaveSubmitUpdateAsync(XGameSaveUpdateHandle,XAsyncBlock*){return E_NOTIMPL;}
HRESULT WINAPI XodusGameSaveSubmitUpdateResult(XAsyncBlock*){return E_NOTIMPL;}
void XodusGameSaveAsyncShutdown(){++async_stops;}

static size_t directory_count(const std::wstring& root){WIN32_FIND_DATAW data{};HANDLE h=FindFirstFileW((root+L"\\*").c_str(),&data);if(h==INVALID_HANDLE_VALUE)return 0;size_t count=0;do{if((data.dwFileAttributes&FILE_ATTRIBUTE_DIRECTORY)&&data.cFileName[0]!=L'.')++count;}while(FindNextFileW(h,&data));FindClose(h);return count;}
struct Seen {unsigned count=0;bool okay=true;bool stop=false;};
static bool CALLBACK blob_seen(const XGameSaveBlobInfo* info,void* p){auto& s=*static_cast<Seen*>(p);++s.count;s.okay&=info&&info->name&&info->size<=3;return !s.stop;}
static bool CALLBACK container_seen(const XGameSaveContainerInfo* info,void* p){auto& s=*static_cast<Seen*>(p);++s.count;s.okay&=info&&info->name&&info->displayName&&!info->needsSync&&info->blobCount==2&&info->totalSize==3&&info->lastModifiedTime>0;return !s.stop;}
int main(int argc,char** argv){
    if(argc!=2)return 2;
    int n=MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,argv[1],-1,nullptr,0);if(!n)return 2;
    std::vector<wchar_t> path(n);MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,argv[1],-1,path.data(),n);std::wstring root(path.data());
    const char* scid="00000000-0000-0000-0000-00000000000A";
    XGameSaveProviderHandle provider=reinterpret_cast<XGameSaveProviderHandle>(1);
    SetEnvironmentVariableW(L"XODUS_LOCAL_GAMESAVE",nullptr);SetEnvironmentVariableW(L"XODUS_LOCAL_GAMESAVE_ROOT",root.c_str());
    check(XodusGameSaveInitializeProvider(nullptr,scid,false,&provider)==E_NOTIMPL&&!provider,"default-off-neutral-output");
    check(identity_calls==0&&directory_count(root)==0,"default-off-no-identity-or-storage");
    for(const char* value:{"true","01","11","0"}){SetEnvironmentVariableA("XODUS_LOCAL_GAMESAVE",value);check(!XodusGameSaveLocalEnabled(),"only-exact-opt-in-one");}
    void* object=reinterpret_cast<void*>(1);check(QueryGameSaveRuntime(&CLSID_XGameSaveImpl,IID_IXGameSaveImpl3,&object)==E_NOTIMPL&&!object,"query-gate-default-off");
    SetEnvironmentVariableA("XODUS_LOCAL_GAMESAVE","1");
    SetEnvironmentVariableW(L"XODUS_LOCAL_GAMESAVE_ROOT",L"relative");check(XodusGameSaveInitializeProvider(nullptr,scid,false,&provider)==E_INVALIDARG&&!provider,"relative-root-rejected");
    SetEnvironmentVariableW(L"XODUS_LOCAL_GAMESAVE_ROOT",L"\\\\example.invalid\\share");check(XodusGameSaveInitializeProvider(nullptr,scid,false,&provider)==E_INVALIDARG&&!provider,"unc-root-rejected-before-access");
    SetEnvironmentVariableW(L"XODUS_LOCAL_GAMESAVE_ROOT",root.c_str());
    for(const char* bad:{"","not-a-guid","00000000_0000-0000-0000-00000000000A","00000000-0000-0000-0000-00000000000AX","00000000-0000-0000-0000-00000000000G"})check(XodusGameSaveInitializeProvider(nullptr,bad,false,&provider)==E_INVALIDARG&&!provider,"strict-scid-rejected");
    check(identity_calls==0&&directory_count(root)==0,"invalid-config-no-identity-or-storage");
    title_id=0;check(FAILED(XodusGameSaveInitializeProvider(nullptr,scid,false,&provider))&&!provider&&directory_count(root)==0,"missing-title-no-namespace");title_id=12345;
    identity_status=E_ACCESSDENIED;check(XodusGameSaveInitializeProvider(nullptr,scid,false,&provider)==E_ACCESSDENIED&&!provider,"title-failure-propagated");identity_status=S_OK;
    check(XodusGameSaveInitializeProvider(nullptr,scid,true,&provider)==E_NOTIMPL&&!provider&&directory_count(root)==0,"unsupported-sync-demand-not-published");
    IXGameSaveImpl3* api=nullptr;check(QueryGameSaveRuntime(&CLSID_XGameSaveImpl,IID_IXGameSaveImpl3,reinterpret_cast<void**>(&api))==S_OK&&api,"opt-in-interface");if(!api)return 3;
    for(const GUID* iid:{&IID_IXGameSaveImpl,&IID_IXGameSaveImpl2,&IID_IXGameSaveImpl3}){void* alias=nullptr;check(api->QueryInterface(*iid,&alias)==S_OK&&alias==api,"revision-layout-alias");if(alias)static_cast<IUnknown*>(alias)->Release();}
    check(api->XGameSaveInitializeProvider(nullptr,scid,false,&provider)==S_OK&&provider&&directory_count(root)==1,"explicit-null-user-local-namespace");
    XGameSaveContainerHandle container=nullptr;XGameSaveUpdateHandle update=nullptr;
    check(api->XGameSaveCreateContainer(provider,"test/container",&container)==S_OK&&container,"create-container");
    check(api->XGameSaveCreateUpdate(container,"Synthetic display",&update)==S_OK&&update,"create-update");
    check(api->XGameSaveSubmitBlobWrite(update,"alpha",reinterpret_cast<const uint8_t*>("xyz"),3)==S_OK,"stage-three-bytes");
    check(api->XGameSaveSubmitBlobWrite(update,"empty",nullptr,0)==S_OK,"stage-empty-blob");
    check(api->XGameSaveSubmitUpdate(update)==S_OK,"commit-real-local-snapshot");api->XGameSaveCloseUpdate(update);
    Seen seen;check(api->XGameSaveEnumerateContainerInfo(provider,&seen,container_seen)==S_OK&&seen.count==1&&seen.okay,"container-callback-packing-context");
    seen={};check(api->XGameSaveEnumerateBlobInfo(container,&seen,blob_seen)==S_OK&&seen.count==2&&seen.okay,"blob-callback-packing-context");
    seen={0,true,true};check(api->XGameSaveEnumerateBlobInfo(container,&seen,blob_seen)==S_OK&&seen.count==1,"callback-false-stops");
    seen={};check(api->XGameSaveEnumerateBlobInfoByName(container,"al",&seen,blob_seen)==S_OK&&seen.count==1,"blob-prefix-selection");
    seen={};check(api->XGameSaveGetContainerInfo(provider,"missing",&seen,container_seen)==S_OK&&seen.count==0,"absent-container-no-callback");
    check(api->XGameSaveEnumerateBlobInfo(container,nullptr,nullptr)==E_POINTER,"null-callback-rejected");
    alignas(XGameSaveBlob) uint8_t storage[256];std::memset(storage,0xcc,sizeof(storage));auto blobs=reinterpret_cast<XGameSaveBlob*>(storage);uint32_t count=0;
    check(api->XGameSaveReadBlobData(container,nullptr,&count,sizeof(storage),blobs)==S_OK&&count==2,"null-names-read-all-not-count-capacity");
    bool packed=blobs[0].info.size==3&&!std::strcmp(blobs[0].info.name,"alpha")&&!std::memcmp(blobs[0].data,"xyz",3)&&blobs[1].info.size==0&&!std::strcmp(blobs[1].info.name,"empty");
    for(unsigned i=0;i<2;++i)packed&=reinterpret_cast<const uint8_t*>(blobs[i].info.name)>=storage+2*sizeof(XGameSaveBlob)&&reinterpret_cast<const uint8_t*>(blobs[i].info.name)<storage+sizeof(storage)&&blobs[i].data>=storage+2*sizeof(XGameSaveBlob)&&blobs[i].data<storage+sizeof(storage);
    check(packed,"sync-read-pointer-relocation-exact-data");
    std::memset(storage,0xcc,sizeof(storage));count=0;check(api->XGameSaveReadBlobData(container,nullptr,&count,1,blobs)==local_save::buffer_too_small&&!count&&storage[0]==0xcc,"short-read-no-output-write");
    const char* selected[]={"alpha"};count=1;check(api->XGameSaveReadBlobData(container,selected,&count,sizeof(storage),blobs)==S_OK&&count==1&&!std::strcmp(blobs[0].info.name,"alpha"),"selected-read-input-count");
    const char* absent[]={"missing"};count=1;check(api->XGameSaveReadBlobData(container,absent,&count,sizeof(storage),blobs)==local_save::blob_not_found&&!count,"missing-blob-no-invented-data");
    int64_t quota=0;check(api->XGameSaveGetRemainingQuota(provider,&quota)==S_OK&&quota==static_cast<int64_t>(local_save::default_quota-3),"quota-from-actual-local-data");
    XGameSaveContainerHandle boundary_container=nullptr;XGameSaveUpdateHandle boundary_update=nullptr;
    const std::string name256(256,'b'),name257(257,'b');
    check(api->XGameSaveCreateContainer(provider,"boundary",&boundary_container)==S_OK&&boundary_container,"boundary-container-create");
    check(api->XGameSaveCreateUpdate(boundary_container,"Boundary fixture",&boundary_update)==S_OK&&boundary_update,"boundary-update-create");
    check(api->XGameSaveSubmitBlobWrite(boundary_update,name256.c_str(),reinterpret_cast<const uint8_t*>("limit"),5)==S_OK,"name-256-write-accepted");
    check(api->XGameSaveSubmitUpdate(boundary_update)==S_OK,"name-256-write-committed");api->XGameSaveCloseUpdate(boundary_update);
    alignas(XGameSaveBlob) uint8_t boundary_storage[1024];std::memset(boundary_storage,0xa5,sizeof(boundary_storage));
    auto boundary_blobs=reinterpret_cast<XGameSaveBlob*>(boundary_storage);const char* boundary_names[]={name256.c_str()};count=1;
    check(api->XGameSaveReadBlobData(boundary_container,boundary_names,&count,sizeof(boundary_storage),boundary_blobs)==S_OK&&count==1&&!std::strcmp(boundary_blobs[0].info.name,name256.c_str())&&boundary_blobs[0].info.size==5&&!std::memcmp(boundary_blobs[0].data,"limit",5),"name-256-targeted-sync-read-matches-write");
    check(boundary_storage[sizeof(XGameSaveBlob)+257+5]==0xa5,"name-256-packed-buffer-canary");
    check(api->XGameSaveCreateUpdate(boundary_container,"Rejected boundary",&boundary_update)==S_OK&&boundary_update,"boundary-rejected-update-create");
    check(api->XGameSaveSubmitBlobWrite(boundary_update,name257.c_str(),reinterpret_cast<const uint8_t*>("bad"),3)==local_save::invalid_name,"name-257-write-rejected");api->XGameSaveCloseUpdate(boundary_update);
    std::memset(boundary_storage,0xa5,sizeof(boundary_storage));boundary_names[0]=name257.c_str();count=1;
    check(api->XGameSaveReadBlobData(boundary_container,boundary_names,&count,sizeof(boundary_storage),boundary_blobs)==local_save::invalid_name&&!count&&boundary_storage[0]==0xa5,"name-257-targeted-sync-read-rejected-no-output");
    api->XGameSaveCloseContainer(boundary_container);
    api->XGameSaveCloseContainer(container);api->XGameSaveCloseProvider(provider);
    check(api->XGameSaveInitializeProvider(nullptr,"00000000-0000-0000-0000-00000000000a",false,&provider)==S_OK&&directory_count(root)==1,"canonical-scid-same-persistent-namespace");
    api->XGameSaveCreateContainer(provider,"test/container",&container);count=1;check(api->XGameSaveReadBlobData(container,selected,&count,sizeof(storage),blobs)==S_OK&&blobs[0].info.size==3,"closed-and-reopened-data-persists");api->XGameSaveCloseContainer(container);api->XGameSaveCloseProvider(provider);
    const XUserHandle user_a=reinterpret_cast<XUserHandle>(uintptr_t(17)),user_b=reinterpret_cast<XUserHandle>(uintptr_t(18));
    check(api->XGameSaveInitializeProvider(user_a,scid,false,&provider)==S_OK&&directory_count(root)==2,"actual-user-id-has-separate-namespace");seen={};check(api->XGameSaveEnumerateContainerInfo(provider,&seen,container_seen)==S_OK&&!seen.count,"user-namespace-does-not-read-null-saves");api->XGameSaveCloseProvider(provider);
    check(api->XGameSaveInitializeProvider(user_b,scid,false,&provider)==S_OK&&directory_count(root)==3,"other-user-id-is-isolated");api->XGameSaveCloseProvider(provider);
    ++title_id;check(api->XGameSaveInitializeProvider(user_a,scid,false,&provider)==S_OK&&directory_count(root)==4,"other-title-is-isolated");api->XGameSaveCloseProvider(provider);--title_id;
    check(api->XGameSaveInitializeProvider(user_a,"00000000-0000-0000-0000-00000000000b",false,&provider)==S_OK&&directory_count(root)==5,"other-scid-is-isolated");api->XGameSaveCloseProvider(provider);
    std::vector<local_save::Blob> values={{"one",{1,2}},{"two",{}}};SIZE_T required=0;check(XodusGameSavePackedBlobSize(values,&required)==S_OK&&required==2*sizeof(XGameSaveBlob)+10,"size-includes-headers-names-nuls-data");
    std::memset(storage,0xaa,sizeof(storage));count=7;check(XodusGameSavePackBlobs({},0,nullptr,&count)==S_OK&&!count,"empty-zero-buffer-valid");count=7;check(XodusGameSavePackBlobs({},1,blobs,&count)==S_OK&&!count&&storage[0]==0xaa,"empty-sentinel-not-written");
    count=7;check(XodusGameSavePackBlobs(values,required-1,blobs,&count)==local_save::buffer_too_small&&!count&&storage[0]==0xaa,"packing-short-buffer-untouched");
    check(XodusGameSavePackBlobs(values,required,nullptr,&count)==E_POINTER,"packing-null-buffer-rejected");
    check(XodusGameSavePackBlobs(values,required,reinterpret_cast<XGameSaveBlob*>(storage+1),&count)==E_INVALIDARG,"packing-misaligned-buffer-rejected");
    check(XodusGameSavePackBlobs(values,required,blobs,&count)==S_OK&&count==2&&storage[required]==0xaa,"packing-exact-capacity-canary");
    values[0].name=std::string("bad\0suffix",10);check(XodusGameSavePackedBlobSize(values,&required)==E_INVALIDARG&&!required,"packing-embedded-nul-rejected");
    std::vector<std::string> copied;const char* bad_names[]={nullptr};check(XodusGameSaveCopyBlobNames(bad_names,1,&copied)==E_POINTER&&copied.empty(),"null-array-entry-rejected");
    check(XodusGameSaveCopyBlobNames(selected,65537,&copied)==E_INVALIDARG&&copied.empty(),"name-array-limit-before-read");
    XUserHandle owned=nullptr;check(XodusGameSaveDuplicateUser(user_a,&owned)==S_OK&&owned==user_a&&user_duplicates==1,"user-ref-retained-for-async");
    ShutdownGameSaveRuntime();XodusGameSaveCloseUser(owned);check(user_closes==1&&async_stops==1,"user-ref-cleanup-after-shutdown");
    check(api->XGameSaveInitializeProvider(nullptr,scid,false,&provider)==E_ABORT&&!provider,"terminal-init-rejected");
    object=reinterpret_cast<void*>(1);check(QueryGameSaveRuntime(&CLSID_XGameSaveImpl,IID_IXGameSaveImpl3,&object)==E_ABORT&&!object,"terminal-query-rejected");
    check(local_save::test_live_handles()==0,"all-core-handles-closed");api->Release();
    std::printf("bridge checks=%u failures=%u account_material=0 external_requests=0\n",checks,failures);return failures?1:0;
}

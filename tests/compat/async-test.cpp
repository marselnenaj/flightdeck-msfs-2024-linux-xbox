#include <compat.h>
#include "../../compat/runtime/src/GameSaveBridge.h"
#include "../../compat/runtime/src/GameSaveAsync.h"
#include <cstdio>
#include <cstring>
#include <vector>
#include <thread>

// This binary never loads a User backend or communicates with any account.
// The only private runtime method supplied is a synthetic, non-account title ID.
struct SyntheticTitle : XodusGameSaveTitleRuntime {
    HRESULT WINAPI QueryInterface(REFIID,void** out) override {if(!out)return E_POINTER;*out=this;return S_OK;}
    ULONG WINAPI AddRef() override{return 2;} ULONG WINAPI Release() override{return 1;}
    HRESULT WINAPI XGameGetXboxTitleId(UINT32* out) override{if(!out)return E_POINTER;*out=123456;return S_OK;}
} synthetic_title;
HRESULT QueryUserRuntime(const GUID* clsid,REFIID,void** out){if(!out)return E_POINTER;*out=nullptr;if(clsid&&clsid->Data1==0x973a344e){*out=&synthetic_title;return S_OK;}return E_NOTIMPL;}
bool UserRuntimeEnabled(){return false;}
extern size_t XodusGameSaveTestLiveJobs();
static unsigned checks,failures;
static XTaskQueueHandle queue;
static void check(bool ok,const char* name){++checks;if(!ok)++failures;std::printf("%s %s\n",ok?"PASS":"FAIL",name);}
static XAsyncBlock block(){XAsyncBlock x{};x.queue=queue;return x;}
static void dispatch(){while(XTaskQueueDispatch(queue,XTaskQueuePort::Work,0)){}while(XTaskQueueDispatch(queue,XTaskQueuePort::Completion,0)){}}
static HRESULT WINAPI unrelated(XAsyncBlock*){return S_OK;}
static bool immediate_result=false;
static void CALLBACK quota_completion(XAsyncBlock* a){int64_t quota=0;immediate_result=XodusGameSaveGetRemainingQuotaResult(a,&quota)==S_OK&&quota>0;}
static bool CALLBACK count_container(const XGameSaveContainerInfo* info,void* p){auto n=(unsigned*)p;(*n)++;return info&&false;}
int wmain(int argc,wchar_t** argv){
    if(argc!=2)return 2;
    constexpr const char* scid="01234567-89ab-cdef-0123-456789abcdef";
    SetEnvironmentVariableW(L"XODUS_LOCAL_GAMESAVE",nullptr);
    XGameSaveProviderHandle p=nullptr;
    check(XodusGameSaveInitializeProvider(nullptr,scid,false,&p)==E_NOTIMPL&&!p,"default_disabled_sync");
    SetEnvironmentVariableW(L"XODUS_LOCAL_GAMESAVE",L"1");SetEnvironmentVariableW(L"XODUS_LOCAL_GAMESAVE_ROOT",argv[1]);
    check(XTaskQueueCreate(XTaskQueueDispatchMode::Manual,XTaskQueueDispatchMode::Manual,&queue)==S_OK,"real_manual_queue");
    IXGameSaveImpl3* api=nullptr;
    check(QueryGameSaveRuntime(&CLSID_XGameSaveImpl,IID_IXGameSaveImpl3,(void**)&api)==S_OK&&api,"real_private_interface");
    if(!api)return 3;
    XAsyncBlock a=block();check(api->XGameSaveInitializeProviderAsync(nullptr,scid,false,&a)==S_OK,"async_initialize_begin");
    check(api->XGameSaveInitializeProviderResult(&a,&p)==E_PENDING&&!p,"initialize_pending_neutral");dispatch();
    check(api->XGameSaveInitializeProviderResult(&a,&p)==S_OK&&p,"initialize_owned_result");
    check(XodusGameSaveTestLiveJobs()==0,"initialize_cleanup");
    int64_t quota=3;a=block();check(api->XGameSaveGetRemainingQuotaAsync(p,&a)==S_OK,"quota_begin");
    check(api->XGameSaveGetRemainingQuotaResult(&a,&quota)==E_PENDING&&!quota,"quota_pending_neutral");dispatch();
    check(api->XGameSaveDeleteContainerResult(&a)==E_INVALIDARG,"foreign_result_nonconsuming");
    check(api->XGameSaveGetRemainingQuotaResult(&a,nullptr)==E_POINTER,"null_result_nonconsuming");
    check(api->XGameSaveGetRemainingQuotaResult(&a,&quota)==S_OK&&quota==(int64_t)local_save::default_quota,"real_quota_result");
    XGameSaveContainerHandle c=nullptr;check(api->XGameSaveCreateContainer(p,"synthetic/async",&c)==S_OK,"container_create_interface");
    XGameSaveUpdateHandle u=nullptr;check(api->XGameSaveCreateUpdate(c,"Synthetic async",&u)==S_OK,"update_create_interface");
    check(api->XGameSaveSubmitBlobWrite(u,"sample",(const uint8_t*)"payload",7)==S_OK,"stage_interface_write");
    a=block();check(api->XGameSaveSubmitUpdateAsync(u,&a)==S_OK,"submit_async_begin");dispatch();
    check(api->XGameSaveSubmitUpdateResult(&a)==S_OK,"submit_async_commit_result");api->XGameSaveCloseUpdate(u);
    unsigned seen=0;check(api->XGameSaveEnumerateContainerInfo(p,&seen,count_container)==S_OK&&seen==1,"enumeration_callback_interface");
    char name[]="sample";const char* names[]={name};a=block();
    check(api->XGameSaveReadBlobDataAsync(c,names,1,&a)==S_OK,"read_async_copies_names");std::memcpy(name,"mutate",7);names[0]=nullptr;
    uint32_t count=7;std::vector<uint8_t> buffer(256,0x7f);auto out=(XGameSaveBlob*)buffer.data();
    check(api->XGameSaveReadBlobDataResult(&a,buffer.size(),out,&count)==E_PENDING&&!count,"read_pending_neutral");dispatch();
    SIZE_T required=0;check(XAsyncGetResultSize(&a,&required)==S_OK&&required>=sizeof(XGameSaveBlob)+7,"packed_result_size");
    check(api->XGameSaveReadBlobDataResult(&a,required-1,out,&count)==local_save::buffer_too_small&&!count&&buffer[0]==0x7f,"short_result_nonconsuming_no_write");
    check(api->XGameSaveReadBlobDataResult(&a,buffer.size(),nullptr,&count)==E_POINTER,"null_blob_output_nonconsuming");
    check(api->XGameSaveReadBlobDataResult(&a,buffer.size()-1,(XGameSaveBlob*)(buffer.data()+1),&count)==E_INVALIDARG,"misaligned_result_nonconsuming");
    check(api->XGameSaveReadBlobDataResult(&a,buffer.size(),out,&count)==S_OK&&count==1&&!std::strcmp(out[0].info.name,"sample")&&out[0].info.size==7&&!std::memcmp(out[0].data,"payload",7)&&buffer[required]==0x7f,"packed_pointers_names_and_exact_bytes");
    check(XodusGameSaveTestLiveJobs()==0,"read_job_cleaned_after_result");
    a=block();check(api->XGameSaveReadBlobDataAsync(c,nullptr,0,&a)==S_OK,"read_all_async_begin");dispatch();
    check(api->XGameSaveReadBlobDataResult(&a,buffer.size(),out,&count)==S_OK&&count==1,"read_all_actual_inventory");
    XGameSaveContainerHandle empty=nullptr;api->XGameSaveCreateContainer(p,"empty",&empty);
    a=block();check(api->XGameSaveReadBlobDataAsync(empty,nullptr,0,&a)==S_OK,"empty_read_begin");dispatch();count=5;
    check(api->XGameSaveReadBlobDataResult(&a,0,nullptr,&count)==S_OK&&!count,"empty_result_no_invented_blob");api->XGameSaveCloseContainer(empty);
    a=block();check(api->XGameSaveReadBlobDataAsync(c,nullptr,0,&a)==S_OK,"cancel_read_begin");XAsyncCancel(&a);dispatch();
    check(api->XGameSaveReadBlobDataResult(&a,buffer.size(),out,&count)==E_ABORT&&!count,"prework_read_cancel");
    check(XodusGameSaveTestLiveJobs()==0,"cancel_cleanup");
    check(api->XGameSaveCreateUpdate(c,"cancelled",&u)==S_OK&&api->XGameSaveSubmitBlobWrite(u,"sample",(const uint8_t*)"wrong",5)==S_OK,"cancel_write_stage");
    a=block();check(api->XGameSaveSubmitUpdateAsync(u,&a)==S_OK,"cancel_submit_begin");XAsyncCancel(&a);dispatch();
    check(api->XGameSaveSubmitUpdateResult(&a)==E_ABORT,"prework_submit_cancel");api->XGameSaveCloseUpdate(u);
    const char* select[] = {"sample"};count=1;check(api->XGameSaveReadBlobData(c,select,&count,buffer.size(),out)==S_OK&&out[0].info.size==7&&!std::memcmp(out[0].data,"payload",7),"cancel_did_not_write");
    HANDLE entered=CreateEventW(nullptr,TRUE,FALSE,nullptr),resume=CreateEventW(nullptr,TRUE,FALSE,nullptr);
    check(api->XGameSaveCreateUpdate(c,"active cancellation",&u)==S_OK&&api->XGameSaveSubmitBlobWrite(u,"sample",(const uint8_t*)"wrong",5)==S_OK,"active_cancel_stage");
    local_save::test_commit_barrier(entered,resume);a=block();check(api->XGameSaveSubmitUpdateAsync(u,&a)==S_OK,"active_cancel_begin");
    std::thread worker([]{XTaskQueueDispatch(queue,XTaskQueuePort::Work,0);});
    check(WaitForSingleObject(entered,10000)==WAIT_OBJECT_0,"active_worker_reached_precommit");XAsyncCancel(&a);SetEvent(resume);worker.join();dispatch();
    check(api->XGameSaveSubmitUpdateResult(&a)==E_ABORT,"active_worker_cancel_result");api->XGameSaveCloseUpdate(u);CloseHandle(entered);CloseHandle(resume);
    count=1;check(api->XGameSaveReadBlobData(c,select,&count,buffer.size(),out)==S_OK&&out[0].info.size==7&&!std::memcmp(out[0].data,"payload",7),"active_cancel_atomic_rollback");
    XTaskQueueHandle immediate=nullptr;check(XTaskQueueCreate(XTaskQueueDispatchMode::Immediate,XTaskQueueDispatchMode::Immediate,&immediate)==S_OK,"immediate_queue");
    XAsyncBlock inline_block{};inline_block.queue=immediate;inline_block.callback=quota_completion;
    check(api->XGameSaveGetRemainingQuotaAsync(p,&inline_block)==S_OK&&immediate_result&&XodusGameSaveTestLiveJobs()==0,"inline_completion_consumes_result_without_uaf");XTaskQueueCloseHandle(immediate);
    // A quota query must include a mutation already queued on another queue.
    // It must not block the sole dispatcher of its own queue while waiting.
    XTaskQueueHandle quota_queue=nullptr;check(XTaskQueueCreate(XTaskQueueDispatchMode::Manual,XTaskQueueDispatchMode::Manual,&quota_queue)==S_OK,"separate_quota_queue");
    check(api->XGameSaveCreateUpdate(c,"quota ordering",&u)==S_OK&&api->XGameSaveSubmitBlobWrite(u,"sample",(const uint8_t*)"expanded!",9)==S_OK,"queued_quota_write_fixture");
    a=block();check(api->XGameSaveSubmitUpdateAsync(u,&a)==S_OK,"queued_mutation_registered");
    XAsyncBlock q{};q.queue=quota_queue;check(api->XGameSaveGetRemainingQuotaAsync(p,&q)==S_OK,"quota_captures_prior_submission");
    check(XTaskQueueDispatch(quota_queue,XTaskQueuePort::Work,0)&&XAsyncGetStatus(&q,FALSE)==E_PENDING,"quota_worker_reschedules_without_blocking");
    std::atomic<bool> sync_quota_done{false};HRESULT sync_quota_hr=E_FAIL;int64_t sync_quota_value=0;
    std::thread sync_quota([&]{sync_quota_hr=api->XGameSaveGetRemainingQuota(p,&sync_quota_value);sync_quota_done=true;});
    Sleep(30);check(!sync_quota_done.load(),"sync_quota_waits_already_queued_write");dispatch();sync_quota.join();
    check(sync_quota_hr==S_OK&&sync_quota_value==(int64_t)local_save::default_quota-9,"sync_quota_returns_after_commit");
    // Deliberately keep SubmitResult unconsumed: quota must wait only for Work.
    Sleep(20);while(XTaskQueueDispatch(quota_queue,XTaskQueuePort::Work,0)){}while(XTaskQueueDispatch(quota_queue,XTaskQueuePort::Completion,0)){}
    check(api->XGameSaveGetRemainingQuotaResult(&q,&quota)==S_OK&&quota==(int64_t)local_save::default_quota-9,"async_quota_does_not_wait_for_submit_result_cleanup");
    check(api->XGameSaveSubmitUpdateResult(&a)==S_OK,"queued_write_result_remains_owned");api->XGameSaveCloseUpdate(u);
    // A later queued mutation cannot make an earlier quota query deadlock.
    q={};q.queue=quota_queue;check(api->XGameSaveGetRemainingQuotaAsync(p,&q)==S_OK,"quota_before_future_write");
    check(api->XGameSaveCreateUpdate(c,"future write",&u)==S_OK&&api->XGameSaveSubmitBlobWrite(u,"sample",(const uint8_t*)"later",5)==S_OK,"future_write_fixture");
    a=block();check(api->XGameSaveSubmitUpdateAsync(u,&a)==S_OK,"future_mutation_registered");
    while(XTaskQueueDispatch(quota_queue,XTaskQueuePort::Work,0)){}while(XTaskQueueDispatch(quota_queue,XTaskQueuePort::Completion,0)){}
    check(api->XGameSaveGetRemainingQuotaResult(&q,&quota)==S_OK&&quota==(int64_t)local_save::default_quota-9,"earlier_quota_excludes_future_barrier");
    q={};q.queue=quota_queue;check(api->XGameSaveGetRemainingQuotaAsync(p,&q)==S_OK,"waiting_quota_cancel_begin");
    check(XTaskQueueDispatch(quota_queue,XTaskQueuePort::Work,0)&&XAsyncGetStatus(&q,FALSE)==E_PENDING,"waiting_quota_poll_pending");
    XAsyncCancel(&q);Sleep(20);while(XTaskQueueDispatch(quota_queue,XTaskQueuePort::Work,0)){}while(XTaskQueueDispatch(quota_queue,XTaskQueuePort::Completion,0)){}
    check(api->XGameSaveGetRemainingQuotaResult(&q,&quota)==E_ABORT&&!quota,"waiting_quota_cancel_does_not_wait_for_write");
    XAsyncCancel(&a);dispatch();check(api->XGameSaveSubmitUpdateResult(&a)==E_ABORT,"future_write_cancel_releases_ticket");api->XGameSaveCloseUpdate(u);
    check(api->XGameSaveGetRemainingQuota(p,&quota)==S_OK&&quota==(int64_t)local_save::default_quota-9,"cancelled_queued_write_does_not_change_quota");
    XTaskQueueCloseHandle(quota_queue);
    a=block();check(api->XGameSaveDeleteContainerAsync(p,"synthetic/async",&a)==S_OK,"delete_async_begin");dispatch();
    check(api->XGameSaveDeleteContainerResult(&a)==S_OK,"delete_async_result");seen=0;check(api->XGameSaveEnumerateContainerInfo(p,&seen,count_container)==S_OK&&!seen,"delete_reflected_in_enumeration");
    a=block();check(XAsyncRun(&a,unrelated)==S_OK,"unrelated_async_begin");dispatch();
    check(api->XGameSaveDeleteContainerResult(&a)==E_INVALIDARG,"unrelated_empty_result_rejected");
    a=block();check(api->XGameSaveGetRemainingQuotaAsync(p,&a)==S_OK,"close_during_queued_job_begin");api->XGameSaveCloseProvider(p);dispatch();
    check(api->XGameSaveGetRemainingQuotaResult(&a,&quota)==E_ABORT,"provider_close_cancels_queued_access");api->XGameSaveCloseContainer(c);
    a=block();check(api->XGameSaveInitializeProviderAsync(nullptr,scid,false,&a)==S_OK,"unretrieved_provider_begin");dispatch();XAsyncCancel(&a);dispatch();
    // Cancel signals the provider but does not itself consume a completed
    // result. Result retrieval performs the ownership cleanup even on E_ABORT.
    check(api->XGameSaveInitializeProviderResult(&a,&p)==E_ABORT&&!p,"completed_cancel_discards_unpublished_provider");
    check(XodusGameSaveTestLiveJobs()==0&&local_save::test_live_handles()==0,"owned_provider_result_cleanup");
    a=block();check(api->XGameSaveInitializeProviderAsync(nullptr,scid,false,&a)==S_OK,"prework_init_cancel_begin");XAsyncCancel(&a);dispatch();
    check(api->XGameSaveInitializeProviderResult(&a,&p)==E_ABORT&&!p&&XodusGameSaveTestLiveJobs()==0&&local_save::test_live_handles()==0,"prework_init_cancel_owns_no_provider");
    // A successful commit remains successful if cancellation is later requested.
    check(api->XGameSaveInitializeProvider(nullptr,scid,false,&p)==S_OK&&api->XGameSaveCreateContainer(p,"after",&c)==S_OK,"post_cancel_reopen");
    check(api->XGameSaveCreateUpdate(c,"after",&u)==S_OK&&api->XGameSaveSubmitBlobWrite(u,"value",(const uint8_t*)"yes",3)==S_OK,"post_commit_fixture");
    a=block();check(api->XGameSaveSubmitUpdateAsync(u,&a)==S_OK,"post_commit_begin");dispatch();XAsyncCancel(&a);
    check(api->XGameSaveSubmitUpdateResult(&a)==S_OK,"post_commit_result");api->XGameSaveCloseUpdate(u);
    a=block();check(api->XGameSaveGetRemainingQuotaAsync(p,&a)==S_OK,"shutdown_queued_begin");ShutdownGameSaveRuntime();dispatch();
    check(api->XGameSaveGetRemainingQuotaResult(&a,&quota)==E_ABORT,"shutdown_queued_abort");
    check(XodusGameSaveTestLiveJobs()==0&&local_save::test_live_handles()==0,"shutdown_jobs_and_handles_clean");
    api->Release();XTaskQueueCloseHandle(queue);
    std::printf("SUMMARY checks=%u failed=%u\n",checks,failures);return failures?1:0;
}

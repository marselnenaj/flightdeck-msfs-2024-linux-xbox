#include "../../compat/runtime/src/GameSaveLocalCore.h"
#include <cstdio>
#include <cstring>
#include <thread>

using namespace local_save;
static unsigned checks=0,failures=0;
static void check(bool ok,const char* name) {++checks;if(!ok)++failures;std::printf("%s %s\n",ok?"PASS":"FAIL",name);}
static Handle make_update(Handle c,const char* name,const char* bytes) {
    Handle u=nullptr;check(create_update(c,"Synthetic local save",&u)==S_OK,"create_update");
    check(write_blob(u,name,(const uint8_t*)bytes,std::strlen(bytes))==S_OK,"stage_write");return u;
}
static bool has(Handle c,const char* key,const char* text) {
    std::vector<std::string> names{key};std::vector<Blob> blobs;
    return read_blobs(c,&names,&blobs)==S_OK&&blobs.size()==1&&blobs[0].data.size()==std::strlen(text)&&!std::memcmp(blobs[0].data.data(),text,std::strlen(text));
}
static DWORD child(const std::wstring& root,const wchar_t* mode) {
    wchar_t exe[32768];GetModuleFileNameW(nullptr,exe,32768);
    std::wstring cmd=L"\""+std::wstring(exe)+L"\" "+mode+L" \""+root+L"\"";
    STARTUPINFOW si{};si.cb=sizeof(si);PROCESS_INFORMATION pi{};
    if(!CreateProcessW(nullptr,&cmd[0],nullptr,nullptr,FALSE,0,nullptr,nullptr,&si,&pi))return 999;
    WaitForSingleObject(pi.hProcess,10000);DWORD status=999;GetExitCodeProcess(pi.hProcess,&status);CloseHandle(pi.hThread);CloseHandle(pi.hProcess);return status;
}
int wmain(int argc,wchar_t** argv) {
    if(argc<2)return 2;
    Options options;options.enabled=true;options.root=argv[argc-1];options.namespace_key=std::string(64,'a');options.quota=1024;
    if(argc==3) {
        if(!std::wcscmp(argv[1],L"--hold")||!std::wcscmp(argv[1],L"--lock-other"))options.namespace_key=std::string(64,'c');
        Handle p=nullptr;HRESULT hr=initialize(options,false,&p);
        if(!std::wcscmp(argv[1],L"--lock"))return hr==HRESULT_FROM_WIN32(ERROR_SHARING_VIOLATION)&&!p?0:21;
        if(!std::wcscmp(argv[1],L"--lock-other"))return (hr==HRESULT_FROM_WIN32(ERROR_LOCK_VIOLATION)||hr==HRESULT_FROM_WIN32(ERROR_SHARING_VIOLATION))&&!p?0:24;
        if(!std::wcscmp(argv[1],L"--hold")){if(FAILED(hr))return 25;std::puts("HOLD_READY");std::fflush(stdout);std::getchar();close_provider(p);return 0;}
        if(FAILED(hr))return 22;Handle c=nullptr;hr=create_container(p,"profiles/save-1",&c);
        bool ok=SUCCEEDED(hr)&&has(c,"alpha","new")&&has(c,"beta","second")&&has(c,"thread_a","A")&&has(c,"thread_b","B");
        close_container(c);close_provider(p);return ok?0:23;
    }
    Options disabled=options;disabled.enabled=false;disabled.root+=L"\\disabled-must-not-exist";
    Handle p=(Handle)1;check(initialize(disabled,false,&p)==E_NOTIMPL&&!p,"disabled_is_unsupported");
    check(GetFileAttributesW(disabled.root.c_str())==INVALID_FILE_ATTRIBUTES,"disabled_no_files");
    check(initialize(options,true,&p)==E_NOTIMPL&&!p,"sync_on_demand_not_claimed");
    Options invalid=options;invalid.namespace_key="../escape";
    check(initialize(invalid,false,&p)==E_INVALIDARG&&!p,"namespace_traversal_rejected");
    check(initialize(options,false,&p)==S_OK&&p,"explicit_local_initialize");
    check(child(options.root,L"--lock")==0,"cross_process_exclusive_writer");
    int64_t quota=0;check(remaining_quota(p,&quota)==S_OK&&quota==1024,"actual_empty_local_quota");
    std::vector<ContainerInfo> info;
    check(container_info(p,nullptr,false,&info)==S_OK&&info.empty(),"new_local_inventory_only");
    Handle c=nullptr;
    for(const char* bad:{"", "../escape", "/absolute", "a//b", "end.", "folder/", "a:b", "two..dots"})
        check(create_container(p,bad,&c)==invalid_name&&!c,"invalid_container_name");
    check(create_container(p,"profiles/save-1",&c)==S_OK,"container_handle_created");
    check(container_info(p,nullptr,false,&info)==S_OK&&info.empty(),"uncommitted_container_invisible");
    Handle u=make_update(c,"alpha","old");
    check(delete_blob(u,"alpha")==E_INVALIDARG,"duplicate_write_delete_rejected");
    check(write_blob(u,"beta",(const uint8_t*)"second",6)==S_OK,"second_blob_staged");
    check(submit_update(u)==S_OK,"atomic_two_blob_commit");
    check(submit_update(u)==handle_expired,"submitted_update_not_reused");
    close_update(u);check(has(c,"alpha","old")&&has(c,"beta","second"),"committed_blob_data_exact");
    check(remaining_quota(p,&quota)==S_OK&&quota==1015,"quota_tracks_committed_bytes");
    check(container_info(p,"profiles/",false,&info)==S_OK&&info.size()==1&&info[0].blob_count==2&&info[0].total_size==9&&info[0].last_modified>0,"metadata_and_prefix");
    check(container_info(p,"profile",true,&info)==S_OK&&info.empty(),"exact_lookup_does_not_prefix_match");
    std::vector<BlobInfo> bi;check(blob_info(c,"b",&bi)==S_OK&&bi.size()==1&&bi[0].name=="beta"&&bi[0].size==6,"blob_prefix_metadata");
    std::vector<std::string> missing{"alpha","absent"};std::vector<Blob> blobs;
    check(read_blobs(c,&missing,&blobs)==blob_not_found&&blobs.empty(),"missing_blob_fails_whole_read");
    Cancellation cancel;cancel.cancelled=true;
    check(read_blobs(c,nullptr,&blobs,&cancel)==E_ABORT&&blobs.empty(),"cancelled_read_no_partial_output");
    u=make_update(c,"alpha","wrong");check(write_blob(u,"beta",(const uint8_t*)"wrong",5)==S_OK,"fault_update_two_blobs");
    test_commit_fault(1);check(FAILED(submit_update(u)),"write_fault_reported");close_update(u);
    check(has(c,"alpha","old")&&has(c,"beta","second"),"write_fault_preserves_complete_generation");
    u=make_update(c,"alpha","wrong");test_commit_fault(2);check(submit_update(u)==E_ABORT,"cancel_before_commit_reported");close_update(u);
    check(has(c,"alpha","old")&&has(c,"beta","second"),"cancel_preserves_complete_generation");
    u=make_update(c,"alpha","wrong");check(submit_update(u,&cancel)==E_ABORT,"pre_cancelled_submit");close_update(u);
    check(has(c,"alpha","old"),"pre_cancel_preserves_state");
    u=make_update(c,"alpha","new");check(submit_update(u)==S_OK,"replacement_commit");close_update(u);
    Handle ua=make_update(c,"thread_a","A"),ub=make_update(c,"thread_b","B");HRESULT ha=E_FAIL,hb=E_FAIL;
    std::thread a([&]{ha=submit_update(ua);});std::thread b([&]{hb=submit_update(ub);});a.join();b.join();close_update(ua);close_update(ub);
    check(ha==S_OK&&hb==S_OK&&has(c,"thread_a","A")&&has(c,"thread_b","B"),"concurrent_updates_no_lost_update");
    u=make_update(c,"oversize","small");std::vector<uint8_t> large(1024,7);
    check(write_blob(u,"quota",large.data(),large.size())==S_OK,"stage_over_quota_deferred");
    check(submit_update(u)==quota_exceeded,"quota_enforced_at_transaction");close_update(u);
    check(has(c,"alpha","new")&&!has(c,"oversize","small"),"quota_failure_is_atomic");
    check(create_update(c,"large",&u)==S_OK,"large_update_created");
    check(write_blob(u,"large",large.data(),max_update_bytes+1)==update_too_big,"sixteen_megabyte_update_limit");close_update(u);
    auto retained=retain(c);close_container(c);check(!is_open(retained),"closed_handle_guard_observes_close");retained.reset();
    check(read_blobs(c,nullptr,&blobs)==handle_expired,"stale_handle_rejected");close_container(c);
    close_provider(p);check(test_live_handles()==0,"closed_handles_released");
    check(child(options.root,L"--reopen")==0,"fresh_process_persistence_and_lock_release");
    check(initialize(options,false,&p)==S_OK,"reopen_after_atomic_commits");check(create_container(p,"profiles/save-1",&c)==S_OK,"reopen_container");
    check(has(c,"alpha","new")&&has(c,"beta","second"),"persisted_bytes_match");
    u=make_update(c,"delete_me","temporary");check(submit_update(u)==S_OK,"delete_fixture_commit");close_update(u);
    check(create_update(c,"after delete",&u)==S_OK&&delete_blob(u,"delete_me")==S_OK&&submit_update(u)==S_OK,"blob_delete_commit");close_update(u);
    std::vector<std::string> deleted{"delete_me"};check(read_blobs(c,&deleted,&blobs)==blob_not_found,"deleted_blob_absent");
    check(delete_container(p,"profiles/save-1",&cancel)==E_ABORT&&has(c,"alpha","new"),"cancelled_container_delete_preserves_state");
    check(delete_container(p,"profiles/save-1")==S_OK,"container_delete_commit");
    check(container_info(p,nullptr,false,&info)==S_OK&&info.empty(),"deleted_container_not_enumerated");
    check(remaining_quota(p,&quota)==S_OK&&quota==1024,"deletion_restores_quota");
    u=make_update(c,"late","x");close_provider(p);
    check(submit_update(u)==handle_expired,"provider_close_invalidates_descendants");close_update(u);close_container(c);
    // A corrupted committed snapshot is never silently treated as empty saves.
    std::wstring file=options.root+L"\\"+std::wstring(64,L'a')+L"\\state.bin";
    HANDLE f=CreateFileW(file.c_str(),GENERIC_WRITE,0,nullptr,OPEN_EXISTING,0,nullptr);DWORD written=0;char bad='!';
    check(f!=INVALID_HANDLE_VALUE&&WriteFile(f,&bad,1,&written,nullptr)&&written==1,"synthetic_corruption_written");if(f!=INVALID_HANDLE_VALUE)CloseHandle(f);
    check(initialize(options,false,&p)==HRESULT_FROM_WIN32(ERROR_INVALID_DATA)&&!p,"corrupt_snapshot_fails_closed");
    options.namespace_key=std::string(64,'b');check(initialize(options,false,&p)==S_OK,"separate_namespace_unaffected");
    shutdown();check(remaining_quota(p,&quota)==handle_expired,"shutdown_invalidates_handles");
    check(initialize(options,false,&p)==E_ABORT&&!p,"shutdown_terminal");
    check(test_live_handles()==0,"shutdown_cleans_registry");
    std::printf("SUMMARY checks=%u failed=%u\n",checks,failures);return failures?1:0;
}

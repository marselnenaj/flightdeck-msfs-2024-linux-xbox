/* SPDX-License-Identifier: LGPL-2.1-or-later */
#include "GameSaveLocalCore.h"
#include <bcrypt.h>
#include <algorithm>
#include <array>
#include <cstring>
#include <condition_variable>
#include <cwctype>
#include <limits>
#include <map>
#include <mutex>
#include <new>
#include <set>
#include <unordered_map>

namespace local_save {
namespace {
constexpr size_t max_containers = 4096, max_blobs = 65536;
constexpr size_t max_metadata = 32u * 1024 * 1024;
constexpr uint8_t magic[8] = {'X','D','L','O','C','A','L','1'};
HRESULT winerror() { DWORD e=GetLastError(); return HRESULT_FROM_WIN32(e?e:ERROR_GEN_FAILURE); }
HRESULT invalid_data() { return HRESULT_FROM_WIN32(ERROR_INVALID_DATA); }
bool cancelled(const Cancellation* c) { return c && c->cancelled.load(); }
bool hex_key(const std::string& s) {
    return s.size()==64 && std::all_of(s.begin(),s.end(),[](char c){return (c>='0'&&c<='9')||(c>='a'&&c<='f');});
}
bool ascii_base(char c) { return (c>='A'&&c<='Z')||(c>='a'&&c<='z')||(c>='0'&&c<='9')||c=='_'; }
bool valid_name(const std::string& s, bool container) {
    if(s.empty()||s.size()>256||s.back()=='.'||s.find("..")!=std::string::npos)return false;
    auto slash=container?s.find_last_of('/'):std::string::npos;
    if(slash!=std::string::npos && (s.front()=='/'||slash==s.size()-1||s.find("//")!=std::string::npos))return false;
    for(size_t i=0;i<s.size();++i) {
        char c=s[i];
        if(slash!=std::string::npos && i<=slash) { if(!ascii_base(c)&&c!='/')return false; }
        else if(!ascii_base(c)&&c!='.'&&c!='-')return false;
    }
    return true;
}
bool copy_string(const char* p,size_t limit,std::string& out) {
    if(!p)return false;
    size_t n=0; while(n<=limit&&p[n])++n;
    if(n>limit)return false;
    out.assign(p,n); return true;
}
bool valid_utf8(const std::string& s) {
    return s.empty() || MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,s.data(),(int)s.size(),nullptr,0)>0;
}
struct File {
    HANDLE value=INVALID_HANDLE_VALUE;
    explicit File(HANDLE h=INVALID_HANDLE_VALUE):value(h){}
    ~File(){if(value!=INVALID_HANDLE_VALUE)CloseHandle(value);}
    File(const File&)=delete; File& operator=(const File&)=delete;
};
bool ordinary_file(HANDLE h) {
    BY_HANDLE_FILE_INFORMATION info{};
    return GetFileInformationByHandle(h,&info) && !(info.dwFileAttributes&(FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_DIRECTORY));
}
struct Container {
    std::string display;
    int64_t modified=0;
    std::map<std::string,std::vector<uint8_t>> blobs;
};
struct State {
    uint64_t generation=0;
    std::map<std::string,Container> containers;
};
struct Session {
    std::mutex mutex;
    std::condition_variable changed;
    uint64_t submitted_sequence=0;
    std::set<uint64_t> pending_mutations;
    std::wstring directory;
    HANDLE file_lock=INVALID_HANDLE_VALUE;
    uint64_t quota=default_quota;
    State state;
    ~Session(){if(file_lock!=INVALID_HANDLE_VALUE)CloseHandle(file_lock);}
};
uint64_t used_bytes(const State& state) {
    uint64_t value=0;
    for(const auto& c:state.containers)for(const auto& b:c.second.blobs)value+=b.second.size();
    return value;
}
size_t blob_count(const State& state) {
    size_t value=0;for(const auto& c:state.containers)value+=c.second.blobs.size();return value;
}
std::atomic<bool> stopped{false};
std::atomic<uint64_t> temp_serial{0};
#ifdef XODUS_GAMESAVE_TESTING
std::atomic<int> commit_fault{0};
std::atomic<HANDLE> commit_entered{nullptr},commit_resume{nullptr};
#endif
HRESULT sha256(const uint8_t* data,size_t size,std::array<uint8_t,32>& result) {
    if(size>ULONG_MAX)return E_INVALIDARG;
    BCRYPT_ALG_HANDLE alg=nullptr; BCRYPT_HASH_HANDLE hash=nullptr;
    NTSTATUS rc=BCryptOpenAlgorithmProvider(&alg,BCRYPT_SHA256_ALGORITHM,nullptr,0);
    if(rc>=0)rc=BCryptCreateHash(alg,&hash,nullptr,0,nullptr,0,0);
    if(rc>=0)rc=BCryptHashData(hash,const_cast<PUCHAR>(data),(ULONG)size,0);
    if(rc>=0)rc=BCryptFinishHash(hash,result.data(),result.size(),0);
    if(hash)BCryptDestroyHash(hash);if(alg)BCryptCloseAlgorithmProvider(alg,0);
    return rc<0?E_FAIL:S_OK;
}
void put32(std::vector<uint8_t>& v,uint32_t x){for(int i=0;i<4;i++)v.push_back((uint8_t)(x>>(i*8)));}
void put64(std::vector<uint8_t>& v,uint64_t x){for(int i=0;i<8;i++)v.push_back((uint8_t)(x>>(i*8)));}
void putstr(std::vector<uint8_t>& v,const std::string& s){put32(v,(uint32_t)s.size());v.insert(v.end(),s.begin(),s.end());}
HRESULT serialize(const Session& session,const State& state,std::vector<uint8_t>& v) {
    if(state.containers.size()>max_containers||blob_count(state)>max_blobs)return quota_exceeded;
    const auto used=used_bytes(state);if(used>session.quota)return quota_exceeded;
    v.insert(v.end(),magic,magic+8);put32(v,1);put64(v,state.generation);put32(v,(uint32_t)state.containers.size());
    for(const auto& c:state.containers) {
        putstr(v,c.first);putstr(v,c.second.display);put64(v,(uint64_t)c.second.modified);put32(v,(uint32_t)c.second.blobs.size());
        for(const auto& b:c.second.blobs) {putstr(v,b.first);put32(v,(uint32_t)b.second.size());v.insert(v.end(),b.second.begin(),b.second.end());}
        if(v.size()>session.quota+max_metadata)return quota_exceeded;
    }
    std::array<uint8_t,32> digest{};HRESULT hr=sha256(v.data(),v.size(),digest);
    if(FAILED(hr))return hr;v.insert(v.end(),digest.begin(),digest.end());return S_OK;
}
struct Reader {
    const uint8_t *p,*end;
    bool take32(uint32_t& x) {if(end-p<4)return false;x=0;for(int i=0;i<4;i++)x|=(uint32_t)*p++<<(i*8);return true;}
    bool take64(uint64_t& x) {if(end-p<8)return false;x=0;for(int i=0;i<8;i++)x|=(uint64_t)*p++<<(i*8);return true;}
    bool text(std::string& x,size_t max) {uint32_t n;if(!take32(n)||n>max||(size_t)(end-p)<n)return false;x.assign((const char*)p,n);p+=n;return x.find('\0')==std::string::npos;}
};
HRESULT deserialize(const Session& session,const std::vector<uint8_t>& v,State& state) {
    if(v.size()<56||std::memcmp(v.data(),magic,8))return invalid_data();
    std::array<uint8_t,32> digest{};HRESULT hr=sha256(v.data(),v.size()-32,digest);
    if(FAILED(hr))return hr;if(std::memcmp(digest.data(),v.data()+v.size()-32,32))return invalid_data();
    Reader r{v.data()+8,v.data()+v.size()-32};uint32_t version,count;
    if(!r.take32(version)||version!=1||!r.take64(state.generation)||!r.take32(count)||count>max_containers)return invalid_data();
    uint64_t used=0;size_t total_blobs=0;
    for(uint32_t i=0;i<count;i++) {
        std::string name;Container c;uint64_t modified;uint32_t blobs;
        if(!r.text(name,256)||!valid_name(name,true)||!r.text(c.display,4096)||!valid_utf8(c.display)||
            !r.take64(modified)||modified>INT64_MAX||!r.take32(blobs)||blobs>max_blobs-total_blobs)return invalid_data();
        c.modified=(int64_t)modified;total_blobs+=blobs;
        for(uint32_t j=0;j<blobs;j++) {
            std::string key;uint32_t n;
            if(!r.text(key,256)||!valid_name(key,false)||!r.take32(n)||(size_t)(r.end-r.p)<n||n>session.quota-used)return invalid_data();
            if(!c.blobs.emplace(key,std::vector<uint8_t>(r.p,r.p+n)).second)return invalid_data();r.p+=n;used+=n;
        }
        if(!state.containers.emplace(name,std::move(c)).second)return invalid_data();
    }
    return r.p==r.end?S_OK:invalid_data();
}
HRESULT load(Session& session) {
    File file(CreateFileW((session.directory+L"\\state.bin").c_str(),GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
    if(file.value==INVALID_HANDLE_VALUE) {DWORD e=GetLastError();return e==ERROR_FILE_NOT_FOUND?S_OK:HRESULT_FROM_WIN32(e);}
    if(!ordinary_file(file.value))return invalid_data();
    LARGE_INTEGER length{};if(!GetFileSizeEx(file.value,&length))return winerror();
    if(length.QuadPart<0||(uint64_t)length.QuadPart>session.quota+max_metadata+32)return invalid_data();
    std::vector<uint8_t> bytes((size_t)length.QuadPart);size_t at=0;
    while(at<bytes.size()){DWORD n=0;if(!ReadFile(file.value,bytes.data()+at,(DWORD)std::min<size_t>(1024*1024,bytes.size()-at),&n,nullptr))return winerror();if(!n)return invalid_data();at+=n;}
    State state;HRESULT hr=deserialize(session,bytes,state);if(SUCCEEDED(hr))session.state=std::move(state);return hr;
}
HRESULT save(Session& session,State& next,const Cancellation* cancel,const std::atomic<bool>* closed) {
    auto abort=[&]{return stopped.load()||cancelled(cancel)||(closed&&closed->load());};
    if(abort())return E_ABORT;
    if(session.state.generation==UINT64_MAX)return invalid_data();
    next.generation=session.state.generation+1;
    std::vector<uint8_t> bytes;HRESULT hr=serialize(session,next,bytes);if(FAILED(hr))return hr;
    if(abort())return E_ABORT;
    const auto temp=session.directory+L"\\pending-"+std::to_wstring(GetCurrentProcessId())+L"-"+std::to_wstring(++temp_serial)+L".bin";
    {
        File file(CreateFileW(temp.c_str(),GENERIC_WRITE,0,nullptr,CREATE_NEW,FILE_ATTRIBUTE_NORMAL|FILE_FLAG_WRITE_THROUGH,nullptr));
        if(file.value==INVALID_HANDLE_VALUE)return winerror();
        size_t at=0;
        while(at<bytes.size()) {
            if(abort()){hr=E_ABORT;break;}
            DWORD n=0;if(!WriteFile(file.value,bytes.data()+at,(DWORD)std::min<size_t>(1024*1024,bytes.size()-at),&n,nullptr)||!n){hr=winerror();break;}at+=n;
        }
        if(SUCCEEDED(hr)&&!FlushFileBuffers(file.value))hr=winerror();
    }
#ifdef XODUS_GAMESAVE_TESTING
    HANDLE entered=commit_entered.exchange(nullptr),resume=commit_resume.exchange(nullptr);
    if(entered&&resume){SetEvent(entered);if(WaitForSingleObject(resume,10000)!=WAIT_OBJECT_0)hr=HRESULT_FROM_WIN32(ERROR_TIMEOUT);}
    int fault=commit_fault.exchange(0);if(fault==1)hr=HRESULT_FROM_WIN32(ERROR_WRITE_FAULT);if(fault==2)hr=E_ABORT;
#endif
    if(SUCCEEDED(hr)&&abort())hr=E_ABORT;
    if(SUCCEEDED(hr)&&!MoveFileExW(temp.c_str(),(session.directory+L"\\state.bin").c_str(),MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH))hr=winerror();
    if(FAILED(hr)){DeleteFileW(temp.c_str());return hr;}
    // Rename is the commit point. A later cancel cannot truthfully undo it.
    session.state=std::move(next);return S_OK;
}
int64_t utc_now() {FILETIME ft;GetSystemTimeAsFileTime(&ft);ULARGE_INTEGER n;n.LowPart=ft.dwLowDateTime;n.HighPart=ft.dwHighDateTime;return (int64_t)(n.QuadPart/10000000)-11644473600ll;}
}
struct Retained {
    enum class Kind {Provider,Container,Update} kind;
    std::shared_ptr<Session> session;
    std::shared_ptr<Retained> parent;
    std::atomic<bool> closed{false};
    std::string name,display;
    struct Change {bool remove=false;std::vector<uint8_t> bytes;};
    std::map<std::string,Change> changes;
    uint64_t staged_bytes=0;
    bool submitted=false;
    explicit Retained(Kind k):kind(k){}
};
struct PendingMutation {
    std::shared_ptr<Session> session;
    uint64_t ticket=0;
    ~PendingMutation() {
        if(!ticket)return;
        std::lock_guard<std::mutex> lock(session->mutex);
        session->pending_mutations.erase(ticket);session->changed.notify_all();
    }
};
namespace {
std::mutex handles_mutex, sessions_mutex;
std::unordered_map<Handle,std::shared_ptr<Retained>> handles;
std::map<std::wstring,std::weak_ptr<Session>> sessions;
uintptr_t next_handle=0x10000;
bool open(const std::shared_ptr<Retained>& p) {
    if(stopped.load()||!p||p->closed.load())return false;
    return !p->parent||open(p->parent);
}
std::shared_ptr<Retained> get(Handle h,Retained::Kind kind) {
    std::lock_guard<std::mutex> lock(handles_mutex);auto it=handles.find(h);
    return it!=handles.end()&&it->second->kind==kind?it->second:nullptr;
}
Handle insert(std::shared_ptr<Retained> p) {
    std::lock_guard<std::mutex> lock(handles_mutex);
    if(stopped.load()||next_handle>UINTPTR_MAX-16)return nullptr;
    Handle h=reinterpret_cast<Handle>(next_handle);next_handle+=16;handles.emplace(h,std::move(p));return h;
}
void close(Handle h,Retained::Kind kind) {
    std::shared_ptr<Retained> p;
    {std::lock_guard<std::mutex> lock(handles_mutex);auto it=handles.find(h);if(it==handles.end()||it->second->kind!=kind)return;p=it->second;handles.erase(it);}
    std::lock_guard<std::mutex> lock(p->session->mutex);p->closed=true;p->session->changed.notify_all();
}
template<class F> HRESULT safe(F f) noexcept {try{return f();}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return E_FAIL;}}
HRESULT acquire_session(const Options& opts,std::shared_ptr<Session>& out) {
    if(!hex_key(opts.namespace_key)||opts.root.size()<3||opts.root[1]!=L':'||(opts.root[2]!=L'\\'&&opts.root[2]!=L'/')||opts.quota>default_quota||!opts.quota)return E_INVALIDARG;
    std::vector<wchar_t> full(32768);DWORD len=GetFullPathNameW(opts.root.c_str(),(DWORD)full.size(),full.data(),nullptr);
    if(!len||len>=full.size())return E_INVALIDARG;
    std::wstring root(full.data(),len);while(root.size()>3&&(root.back()==L'\\'||root.back()==L'/'))root.pop_back();
    DWORD attrs=GetFileAttributesW(root.c_str());
    if(attrs==INVALID_FILE_ATTRIBUTES)return winerror();
    if(!(attrs&FILE_ATTRIBUTE_DIRECTORY)||(attrs&FILE_ATTRIBUTE_REPARSE_POINT))return E_INVALIDARG;
    const auto directory=root+L"\\"+std::wstring(opts.namespace_key.begin(),opts.namespace_key.end());
    std::wstring key=directory;std::transform(key.begin(),key.end(),key.begin(),[](wchar_t c){return (wchar_t)towlower(c);});
    std::lock_guard<std::mutex> lock(sessions_mutex);
    auto existing=sessions[key].lock();if(existing){if(existing->quota!=opts.quota)return E_INVALIDARG;out=std::move(existing);return S_OK;}
    if(!CreateDirectoryW(directory.c_str(),nullptr)&&GetLastError()!=ERROR_ALREADY_EXISTS)return winerror();
    attrs=GetFileAttributesW(directory.c_str());if(attrs==INVALID_FILE_ATTRIBUTES||!(attrs&FILE_ATTRIBUTE_DIRECTORY)||(attrs&FILE_ATTRIBUTE_REPARSE_POINT))return invalid_data();
    auto session=std::make_shared<Session>();session->directory=directory;session->quota=opts.quota;
    session->file_lock=CreateFileW((directory+L"\\writer.lock").c_str(),GENERIC_READ|GENERIC_WRITE,0,nullptr,OPEN_ALWAYS,FILE_FLAG_OPEN_REPARSE_POINT,nullptr);
    if(session->file_lock==INVALID_HANDLE_VALUE)return winerror();if(!ordinary_file(session->file_lock))return invalid_data();
    // Share-mode denial is scoped to a wineserver. An actual byte-range lock
    // additionally reaches Wine's Unix fcntl lock and excludes other prefixes.
    OVERLAPPED range{};
    if(!LockFileEx(session->file_lock,LOCKFILE_EXCLUSIVE_LOCK|LOCKFILE_FAIL_IMMEDIATELY,0,1,0,&range))return winerror();
    HRESULT hr=load(*session);if(FAILED(hr))return hr;
    sessions[key]=session;out=std::move(session);return S_OK;
}
}
HRESULT initialize(const Options& opts,bool sync,Handle* result) {
    if(!result)return E_POINTER;*result=nullptr;
    // In particular, no directory or lock file is touched before explicit opt-in.
    if(!opts.enabled)return E_NOTIMPL;if(sync)return E_NOTIMPL;if(stopped.load())return E_ABORT;
    return safe([&]()->HRESULT{auto p=std::make_shared<Retained>(Retained::Kind::Provider);HRESULT hr=acquire_session(opts,p->session);if(FAILED(hr))return hr;*result=insert(p);return *result?S_OK:E_ABORT;});
}
void close_provider(Handle h){close(h,Retained::Kind::Provider);}
void close_container(Handle h){close(h,Retained::Kind::Container);}
void close_update(Handle h){close(h,Retained::Kind::Update);}
HRESULT remaining_quota(Handle h,int64_t* value) {
    if(!value)return E_POINTER;*value=0;auto p=get(h,Retained::Kind::Provider);if(!p)return handle_expired;
    std::unique_lock<std::mutex> lock(p->session->mutex);const auto barrier=p->session->submitted_sequence;
    p->session->changed.wait(lock,[&]{return !open(p)||p->session->pending_mutations.empty()||*p->session->pending_mutations.begin()>barrier;});
    if(!open(p))return handle_expired;
    *value=(int64_t)(p->session->quota-used_bytes(p->session->state));return S_OK;
}
HRESULT register_mutation(Handle h,std::shared_ptr<PendingMutation>* result) {
    if(!result)return E_POINTER;result->reset();
    return safe([&]()->HRESULT{auto p=retain(h);if(!p||(p->kind!=Retained::Kind::Provider&&p->kind!=Retained::Kind::Update))return handle_expired;
        auto token=std::make_shared<PendingMutation>();token->session=p->session;
        {std::lock_guard<std::mutex> lock(p->session->mutex);if(!open(p))return handle_expired;if(p->session->submitted_sequence==UINT64_MAX)return E_OUTOFMEMORY;
        uint64_t ticket=++p->session->submitted_sequence;p->session->pending_mutations.insert(ticket);token->ticket=ticket;}
        *result=std::move(token);return S_OK;});
}
HRESULT quota_barrier(Handle h,uint64_t* result) {
    if(!result)return E_POINTER;*result=0;auto p=get(h,Retained::Kind::Provider);if(!p)return handle_expired;
    std::lock_guard<std::mutex> lock(p->session->mutex);if(!open(p))return handle_expired;*result=p->session->submitted_sequence;return S_OK;
}
HRESULT remaining_quota_at(Handle h,uint64_t barrier,int64_t* value) {
    if(!value)return E_POINTER;*value=0;auto p=get(h,Retained::Kind::Provider);if(!p)return handle_expired;
    std::lock_guard<std::mutex> lock(p->session->mutex);if(!open(p))return handle_expired;
    if(!p->session->pending_mutations.empty()&&*p->session->pending_mutations.begin()<=barrier)return E_PENDING;
    *value=(int64_t)(p->session->quota-used_bytes(p->session->state));return S_OK;
}
HRESULT create_container(Handle h,const char* name,Handle* result) {
    if(!result)return E_POINTER;*result=nullptr;
    return safe([&]()->HRESULT{std::string n;if(!copy_string(name,256,n)||!valid_name(n,true))return invalid_name;auto p=get(h,Retained::Kind::Provider);if(!p)return handle_expired;
        std::lock_guard<std::mutex> lock(p->session->mutex);if(!open(p))return handle_expired;auto c=std::make_shared<Retained>(Retained::Kind::Container);c->parent=p;c->session=p->session;c->name=std::move(n);*result=insert(c);return *result?S_OK:E_ABORT;});
}
HRESULT container_info(Handle h,const char* filter,bool exact,std::vector<ContainerInfo>* result) {
    if(!result)return E_POINTER;result->clear();
    return safe([&]()->HRESULT{std::string prefix;if(filter&&!copy_string(filter,256,prefix))return invalid_name;auto p=get(h,Retained::Kind::Provider);if(!p)return handle_expired;
        std::lock_guard<std::mutex> lock(p->session->mutex);if(!open(p))return handle_expired;
        for(const auto& c:p->session->state.containers){if(filter&&(exact?c.first!=prefix:c.first.compare(0,prefix.size(),prefix)!=0))continue;ContainerInfo info;info.name=c.first;info.display_name=c.second.display;info.blob_count=(uint32_t)c.second.blobs.size();info.last_modified=c.second.modified;for(const auto& b:c.second.blobs)info.total_size+=b.second.size();result->push_back(std::move(info));}
        return S_OK;});
}
HRESULT blob_info(Handle h,const char* prefix,std::vector<BlobInfo>* result) {
    if(!result)return E_POINTER;result->clear();
    return safe([&]()->HRESULT{std::string filter;if(prefix&&!copy_string(prefix,256,filter))return invalid_name;auto p=get(h,Retained::Kind::Container);if(!p)return handle_expired;
        std::lock_guard<std::mutex> lock(p->session->mutex);if(!open(p))return handle_expired;auto c=p->session->state.containers.find(p->name);if(c==p->session->state.containers.end())return S_OK;
        for(const auto& b:c->second.blobs)if(b.first.compare(0,filter.size(),filter)==0)result->push_back({b.first,(uint32_t)b.second.size()});return S_OK;});
}
HRESULT read_blobs(Handle h,const std::vector<std::string>* names,std::vector<Blob>* result,const Cancellation* cancel) {
    if(!result)return E_POINTER;result->clear();
    return safe([&]()->HRESULT{auto p=get(h,Retained::Kind::Container);if(!p)return handle_expired;std::lock_guard<std::mutex> lock(p->session->mutex);if(!open(p))return handle_expired;if(cancelled(cancel))return E_ABORT;
        auto c=p->session->state.containers.find(p->name);if(c==p->session->state.containers.end())return names&&!names->empty()?blob_not_found:S_OK;
        std::vector<Blob> output;if(names){if(names->size()>max_blobs)return E_INVALIDARG;for(const auto& name:*names){if(!valid_name(name,false))return invalid_name;auto b=c->second.blobs.find(name);if(b==c->second.blobs.end())return blob_not_found;output.push_back({b->first,b->second});}}
        else for(const auto& b:c->second.blobs)output.push_back({b.first,b.second});
        if(cancelled(cancel))return E_ABORT;*result=std::move(output);return S_OK;});
}
HRESULT create_update(Handle h,const char* display,Handle* result) {
    if(!result)return E_POINTER;*result=nullptr;
    return safe([&]()->HRESULT{std::string text;if(!copy_string(display,4096,text)||!valid_utf8(text))return E_INVALIDARG;auto c=get(h,Retained::Kind::Container);if(!c)return handle_expired;
        std::lock_guard<std::mutex> lock(c->session->mutex);if(!open(c))return handle_expired;auto u=std::make_shared<Retained>(Retained::Kind::Update);u->parent=c;u->session=c->session;u->name=c->name;u->display=std::move(text);*result=insert(u);return *result?S_OK:E_ABORT;});
}
HRESULT write_blob(Handle h,const char* name,const uint8_t* data,size_t size) {
    if(size&&!data)return E_POINTER;if(size>max_update_bytes)return update_too_big;
    return safe([&]()->HRESULT{std::string n;if(!copy_string(name,256,n)||!valid_name(n,false))return invalid_name;auto u=get(h,Retained::Kind::Update);if(!u)return handle_expired;
        std::lock_guard<std::mutex> lock(u->session->mutex);if(!open(u)||u->submitted)return handle_expired;if(u->changes.count(n))return E_INVALIDARG;if(u->changes.size()>=max_blobs)return quota_exceeded;if(size>max_update_bytes-u->staged_bytes)return update_too_big;
        Retained::Change change;if(size)change.bytes.assign(data,data+size);u->changes.emplace(std::move(n),std::move(change));u->staged_bytes+=size;return S_OK;});
}
HRESULT delete_blob(Handle h,const char* name) {
    return safe([&]()->HRESULT{std::string n;if(!copy_string(name,256,n)||!valid_name(n,false))return invalid_name;auto u=get(h,Retained::Kind::Update);if(!u)return handle_expired;
        std::lock_guard<std::mutex> lock(u->session->mutex);if(!open(u)||u->submitted)return handle_expired;if(u->changes.count(n))return E_INVALIDARG;if(u->changes.size()>=max_blobs)return quota_exceeded;Retained::Change change;change.remove=true;u->changes.emplace(std::move(n),std::move(change));return S_OK;});
}
HRESULT submit_update(Handle h,const Cancellation* cancel) {
    return safe([&]()->HRESULT{auto u=get(h,Retained::Kind::Update);if(!u)return handle_expired;std::lock_guard<std::mutex> lock(u->session->mutex);if(!open(u)||u->submitted)return handle_expired;u->submitted=true;if(cancelled(cancel))return E_ABORT;
        State next=u->session->state;auto& c=next.containers[u->name];c.display=u->display;c.modified=utc_now();for(const auto& b:u->changes){if(b.second.remove)c.blobs.erase(b.first);else c.blobs[b.first]=b.second.bytes;}
        HRESULT hr=save(*u->session,next,cancel,&u->closed);u->changes.clear();return hr;});
}
HRESULT delete_container(Handle h,const char* name,const Cancellation* cancel) {
    return safe([&]()->HRESULT{std::string n;if(!copy_string(name,256,n)||!valid_name(n,true))return invalid_name;auto p=get(h,Retained::Kind::Provider);if(!p)return handle_expired;std::lock_guard<std::mutex> lock(p->session->mutex);if(!open(p))return handle_expired;if(cancelled(cancel))return E_ABORT;
        State next=p->session->state;next.containers.erase(n);return save(*p->session,next,cancel,&p->closed);});
}
std::shared_ptr<Retained> retain(Handle h){std::lock_guard<std::mutex> lock(handles_mutex);auto it=handles.find(h);return it==handles.end()?nullptr:it->second;}
bool is_open(const std::shared_ptr<Retained>& p){return open(p);}
bool is_open(Handle h){return is_open(retain(h));}
void shutdown() {
    stopped=true;std::unordered_map<Handle,std::shared_ptr<Retained>> old;
    {std::lock_guard<std::mutex> lock(handles_mutex);old.swap(handles);}
    for(auto& p:old){std::lock_guard<std::mutex> lock(p.second->session->mutex);p.second->closed=true;p.second->session->changed.notify_all();}
}
#ifdef XODUS_GAMESAVE_TESTING
void test_commit_fault(int fault){commit_fault=fault;}
void test_commit_barrier(HANDLE entered,HANDLE resume){commit_resume=resume;commit_entered=entered;}
size_t test_live_handles(){std::lock_guard<std::mutex> lock(handles_mutex);return handles.size();}
#endif
}

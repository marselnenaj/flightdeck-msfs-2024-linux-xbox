/* SPDX-License-Identifier: LGPL-2.1-or-later */
// Personal GDK networking adaptation. LGPL-2.1-or-later.
// Policy is obtained from Microsoft over verified HTTPS. No account data is used.
#include "NetworkSecurity.h"
#include <winhttp.h>
#include <vendor/nlohmann_json.hpp>
#include <atomic>
#include <memory>
#include <map>
#include <mutex>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
using Json=nlohmann::json;
constexpr size_t max_document=1024*1024, max_url=32768;
const char security_identity=0;
std::atomic<UINT64> generation{1};
std::mutex policy_lock;
std::shared_ptr<const Json> cached_policy;
ULONGLONG cached_at;
// An authenticated native title-policy fetch publishes only public NSAL here.
// Keep each document's certificate indexes in its own namespace.
std::shared_ptr<const Json> title_policy;
UINT32 published_title_id=0;
bool publication_closed=false;
#ifdef NETWORK_SECURITY_TESTING
std::atomic<void(*)()> publish_barrier{nullptr};
#endif
std::atomic<unsigned> diagnostic_count{0};
std::atomic<unsigned> verify_diagnostic_count{0};
struct InternetHandle {
    HINTERNET value=nullptr;
    explicit InternetHandle(HINTERNET v):value(v){}
    ~InternetHandle(){if(value)WinHttpCloseHandle(value);}
    InternetHandle(const InternetHandle&)=delete;
    InternetHandle&operator=(const InternetHandle&)=delete;
};
struct Url {std::wstring host,path;std::string host_ascii;};
HRESULT last_error(){DWORD e=GetLastError();return e?HRESULT_FROM_WIN32(e):E_FAIL;}

HRESULT parse_url(LPCWSTR text,Url &url){
    if(!text)return E_POINTER;
    size_t length=0;while(length<=max_url&&text[length])++length;
    if(!length||length>max_url)return E_INVALIDARG;
    URL_COMPONENTS parts{};parts.dwStructSize=sizeof(parts);
    parts.dwHostNameLength=parts.dwUrlPathLength=parts.dwExtraInfoLength=parts.dwUserNameLength=parts.dwPasswordLength=DWORD(-1);
    if(!WinHttpCrackUrl(text,static_cast<DWORD>(length),0,&parts))return last_error();
    if(parts.nScheme!=INTERNET_SCHEME_HTTPS||parts.nPort!=443||parts.dwUserNameLength||parts.dwPasswordLength)return E_INVALIDARG;
    if(!parts.dwHostNameLength||parts.dwHostNameLength>253)return E_INVALIDARG;
    url.host.assign(parts.lpszHostName,parts.dwHostNameLength);url.host_ascii.clear();
    for(auto &c:url.host){if(c>=L'A'&&c<=L'Z')c+=L'a'-L'A';if(!((c>=L'a'&&c<=L'z')||(c>=L'0'&&c<=L'9')||c==L'-'||c==L'.'))return E_INVALIDARG;url.host_ascii.push_back(static_cast<char>(c));}
    if(url.host.front()==L'.'||url.host.back()==L'.'||url.host.find(L"..")!=std::wstring::npos)return E_INVALIDARG;
    size_t label_start=0;
    while(label_start<url.host.size()){
        size_t end=url.host.find(L'.',label_start);if(end==std::wstring::npos)end=url.host.size();
        if(end-label_start>63||url.host[label_start]==L'-'||url.host[end-1]==L'-')return E_INVALIDARG;label_start=end+1;
    }
    url.path.assign(parts.lpszUrlPath?parts.lpszUrlPath:L"",parts.dwUrlPathLength);
    if(url.path.empty())url.path=L"/";
    // Do not let alternate encodings or path normalization evade a stricter rule.
    if(url.path.find(L'%')!=std::wstring::npos||url.path.find(L'\\')!=std::wstring::npos||url.path.find(L"/.")!=std::wstring::npos)return E_INVALIDARG;
    return S_OK;
}
void diagnostic(const Url &url,HRESULT hr,const char *match){
    if(diagnostic_count.fetch_add(1)>=64)return;
    std::fprintf(stderr,"[xodus-network] security scheme=https host=%s policy=%s result=%08lx\n",url.host_ascii.c_str(),match,static_cast<unsigned long>(hr));
}
bool ends_with(const std::string &value,const std::string &suffix){return value.size()>suffix.size()&&value.compare(value.size()-suffix.size(),suffix.size(),suffix)==0;}
bool valid_policy_host(const std::string &host){
    if(host.empty()||host.size()>253)return false;
    size_t start=0;
    while(start<host.size()){
        size_t end=host.find('.',start);if(end==std::string::npos)end=host.size();
        if(end==start||end-start>63||host[start]=='-'||host[end-1]=='-')return false;
        for(size_t i=start;i<end;++i){unsigned char c=host[i];if(!((c>='a'&&c<='z')||(c>='A'&&c<='Z')||(c>='0'&&c<='9')||c=='-'))return false;}
        start=end+1;
    }
    return host.back()!='.';
}
Json parse_unique_document(const char *text,size_t length){
    std::vector<std::set<std::string>> keys;
    return Json::parse(text,text+length,[&keys](int,Json::parse_event_t event,Json &value){
        if(event==Json::parse_event_t::object_start)keys.emplace_back();
        else if(event==Json::parse_event_t::key){if(keys.empty()||!keys.back().insert(value.get<std::string>()).second)throw std::invalid_argument("duplicate-policy-key");}
        else if(event==Json::parse_event_t::object_end)keys.pop_back();
        return true;
    });
}
HRESULT validate_title_document(Json &document){
    if(!document.is_object()||!document.contains("EndPoints")||!document["EndPoints"].is_array()||document["EndPoints"].size()>4096)return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
    if(document.contains("Certs")&&(!document["Certs"].is_array()||document["Certs"].size()>4096))return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
    Json supported=Json::array();
    std::map<std::string,Json> routes;
    for(const auto &entry:document["EndPoints"]){
        if(!entry.is_object())return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
        for(auto field=entry.begin();field!=entry.end();++field){
            const auto &key=field.key();
            if(key!="Protocol"&&key!="HostType"&&key!="Host"&&key!="Path"&&key!="MinTlsVersion"&&key!="ServerCertIndex"&&
                    key!="RelyingParty"&&key!="SubRelyingParty"&&key!="TokenType"&&key!="SignaturePolicyIndex")return E_NOTIMPL;
        }
        for(const auto *key:{"Protocol","HostType","Host"})if(!entry.contains(key)||!entry[key].is_string())return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
        const auto protocol=entry["Protocol"].get<std::string>(),type=entry["HostType"].get<std::string>(),host=entry["Host"].get<std::string>();
        if(protocol!="https"&&protocol!="http")return E_NOTIMPL;
        if(type=="wildcard"&&host=="*"){
            // The authenticated title response explicitly permits ordinary
            // HTTPS transport through this bare fallback. This supplies only
            // TLS policy, never an RP/token grant. HTTP remains unsupported;
            // constraints on a future catchall must not be silently discarded.
            if(entry.size()!=3)return E_NOTIMPL;
            if(protocol=="https")supported.push_back(entry);
            continue;
        }
        if(type=="fqdn"){if(!valid_policy_host(host))return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);}
        else if(type=="wildcard"){
            if(host.size()<3||host.compare(0,2,"*.")||!valid_policy_host(host.substr(2)))return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
        }else return E_NOTIMPL;
        if(entry.contains("Path")){
            if(!entry["Path"].is_string())return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
            const auto path=entry["Path"].get<std::string>();
            if(!path.empty()&&(path[0]!='/'||path.find_first_of("%\\?#")!=std::string::npos||path.find("/.")!=std::string::npos))return E_NOTIMPL;
            for(unsigned char c:path)if(c<0x20||c>0x7e)return E_NOTIMPL;
        }
        if(entry.contains("MinTlsVersion")&&!entry["MinTlsVersion"].is_string())return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
        if(entry.contains("ServerCertIndex")){
            const auto &indexes=entry["ServerCertIndex"];
            if(!indexes.is_array()||!document.contains("Certs"))return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
            for(const auto &index:indexes){
                if(!index.is_number_integer()||index.get<long long>()<0||static_cast<size_t>(index.get<long long>())>=document["Certs"].size())return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
                const auto &cert=document["Certs"][index.get<size_t>()];
                if(!cert.is_object()||!cert.contains("Thumbprint")||!cert["Thumbprint"].is_string())return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
            }
        }
        auto canonical_host=host;for(auto &c:canonical_host)if(c>='A'&&c<='Z')c+='a'-'A';
        const auto key=protocol+"\n"+type+"\n"+canonical_host+"\n"+entry.value("Path",std::string{});
        auto found=routes.find(key);
        if(found!=routes.end()&&found->second!=entry)return E_NOTIMPL;
        routes.emplace(key,entry);
        supported.push_back(entry);
    }
    document["EndPoints"]=std::move(supported);
    return S_OK;
}
HRESULT select_policy(const Json &document,const Url &url,XNetworkingSecurityInformation &result,const char **status,bool authenticated_title=false){
    result={};*status="invalid";
    if(!document.is_object()||!document.contains("EndPoints")||!document["EndPoints"].is_array()||document["EndPoints"].size()>4096)return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
    const Json *best=nullptr;size_t best_host=0,best_path=0;bool ambiguous=false;
    for(const auto &entry:document["EndPoints"]){
        if(!entry.is_object()||!entry.contains("Protocol")||!entry["Protocol"].is_string()||!entry.contains("Host")||!entry["Host"].is_string()||!entry.contains("HostType")||!entry["HostType"].is_string())return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
        const auto protocol=entry["Protocol"].get<std::string>();if(protocol!="https")continue;
        auto host=entry["Host"].get<std::string>();const auto type=entry["HostType"].get<std::string>();
        for(auto &c:host)if(c>='A'&&c<='Z')c+='a'-'A';
        bool matches=false;size_t host_score=host.size()*2;
        if(type=="fqdn")matches=host==url.host_ascii;
        else if(type=="wildcard"&&host=="*"){
            // Only the authenticated publication path grants a TLS fallback.
            // It loses to every concrete host/path and cannot waive constraints
            // from the separately selected default NSAL policy.
            if(entry.size()!=3)return E_NOTIMPL;
            if(!authenticated_title)continue;
            matches=true;host_score=0;
        }
        else if(type=="wildcard"&&host.size()>2&&host.compare(0,2,"*.")==0&&host.find('*',1)==std::string::npos){matches=ends_with(url.host_ascii,host.substr(1));--host_score;}
        else return E_NOTIMPL;
        if(!matches)continue;
        std::string path;
        if(entry.contains("Path")){if(!entry["Path"].is_string())return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);path=entry["Path"].get<std::string>();}
        std::wstring wide_path(path.begin(),path.end());
        if(!path.empty()&&(url.path.compare(0,wide_path.size(),wide_path)!=0||(url.path.size()>wide_path.size()&&wide_path.back()!=L'/'&&url.path[wide_path.size()]!=L'/')))continue;
        if(!best||host_score>best_host||(host_score==best_host&&path.size()>best_path)){best=&entry;best_host=host_score;best_path=path.size();ambiguous=false;}
        else if(host_score==best_host&&path.size()==best_path&&entry!=*best)ambiguous=true;
    }
    if(!best){*status="unmatched";return HRESULT_FROM_WIN32(ERROR_NOT_FOUND);}
    if(ambiguous){*status="ambiguous";return E_NOTIMPL;}
    if(best->contains("ServerCertIndex")){
        const auto &indexes=(*best)["ServerCertIndex"];
        if(!indexes.is_array()||!document.contains("Certs")||!document["Certs"].is_array())return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
        for(const auto &index:indexes){
            if(!index.is_number_integer()||index.get<long long>()<0||static_cast<size_t>(index.get<long long>())>=document["Certs"].size())return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
            const auto &cert=document["Certs"][index.get<size_t>()];
            if(!cert.is_object()||!cert.contains("Thumbprint")||!cert["Thumbprint"].is_string())return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
        }
        // Pin/root coupling is not fully specified by the public service schema.
        // Preserve every restriction by failing closed until that path is supported.
        if(!indexes.empty()){*status="pins-unsupported";return E_NOTIMPL;}
    }
    if(best->contains("MinTlsVersion")){
        if(!(*best)["MinTlsVersion"].is_string())return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
        const auto tls=(*best)["MinTlsVersion"].get<std::string>();
        if(tls!="1.0"&&tls!="1.1"&&tls!="1.2"){*status="tls-unsupported";return E_NOTIMPL;}
    }
    // A conservative supported subset: TLS 1.2 only, never older protocols.
    // The matched public policy contains no additional certificate pins.
    result.enabledHttpSecurityProtocolFlags=WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2;
    *status=best_host?"matched-unpinned":"matched-https-fallback";return S_OK;
}

HRESULT select_combined_policy(const Json &defaults,const Json *title,const Url &url,XNetworkingSecurityInformation &result,const char **status){
    result={};XNetworkingSecurityInformation base{},extra{};const char *base_status=nullptr,*title_status=nullptr;
    HRESULT base_hr=select_policy(defaults,url,base,&base_status);
    if(FAILED(base_hr)&&base_hr!=HRESULT_FROM_WIN32(ERROR_NOT_FOUND)){*status=base_status;return base_hr;}
    HRESULT title_hr=title?select_policy(*title,url,extra,&title_status,true):HRESULT_FROM_WIN32(ERROR_NOT_FOUND);
    if(FAILED(title_hr)&&title_hr!=HRESULT_FROM_WIN32(ERROR_NOT_FOUND)){*status=title_status;return title_hr;}
    if(FAILED(base_hr)&&FAILED(title_hr)){*status="unmatched";return HRESULT_FROM_WIN32(ERROR_NOT_FOUND);}
    // The title overrides authentication routing in the native signer. Here
    // both selected security policies must pass: an unpinned title rule cannot
    // erase default pins/TLS restrictions, even at a more specific path.
    result=SUCCEEDED(title_hr)?extra:base;
    if(SUCCEEDED(base_hr)&&SUCCEEDED(title_hr))result.enabledHttpSecurityProtocolFlags&=base.enabledHttpSecurityProtocolFlags;
    if(!result.enabledHttpSecurityProtocolFlags){*status="tls-conflict";return E_NOTIMPL;}
    *status=SUCCEEDED(title_hr)?(!strcmp(title_status,"matched-https-fallback")?
        (SUCCEEDED(base_hr)?"matched-unpinned":"matched-title-https-fallback"):"matched-title-unpinned"):"matched-unpinned";return S_OK;
}

bool stopped(UINT64 epoch,const std::atomic<bool> *cancel){return generation.load()!=epoch||(cancel&&cancel->load());}
HRESULT download_policy(UINT64 epoch,const std::atomic<bool> *cancel,std::shared_ptr<const Json> &out){
    if(stopped(epoch,cancel))return E_ABORT;
    {std::lock_guard<std::mutex> guard(policy_lock);if(cached_policy&&GetTickCount64()-cached_at<60*60*1000){out=cached_policy;return S_OK;}}
    InternetHandle session(WinHttpOpen(L"Xodus-Network-Policy/1",WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,WINHTTP_NO_PROXY_NAME,WINHTTP_NO_PROXY_BYPASS,0));
    if(!session.value)return last_error();
    if(!WinHttpSetTimeouts(session.value,5000,5000,5000,5000))return last_error();
    DWORD tls=WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2;
    if(!WinHttpSetOption(session.value,WINHTTP_OPTION_SECURE_PROTOCOLS,&tls,sizeof(tls)))return last_error();
    InternetHandle connection(WinHttpConnect(session.value,L"title.mgt.xboxlive.com",443,0));if(!connection.value)return last_error();
    InternetHandle request(WinHttpOpenRequest(connection.value,L"GET",L"/titles/default/endpoints?type=1",nullptr,WINHTTP_NO_REFERER,WINHTTP_DEFAULT_ACCEPT_TYPES,WINHTTP_FLAG_SECURE));if(!request.value)return last_error();
    DWORD redirect=WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
    if(!WinHttpSetOption(request.value,WINHTTP_OPTION_REDIRECT_POLICY,&redirect,sizeof(redirect)))return last_error();
    if(stopped(epoch,cancel))return E_ABORT;
    if(!WinHttpSendRequest(request.value,WINHTTP_NO_ADDITIONAL_HEADERS,0,WINHTTP_NO_REQUEST_DATA,0,0,0)||!WinHttpReceiveResponse(request.value,nullptr))return last_error();
    DWORD status=0,size=sizeof(status);
    if(!WinHttpQueryHeaders(request.value,WINHTTP_QUERY_STATUS_CODE|WINHTTP_QUERY_FLAG_NUMBER,WINHTTP_HEADER_NAME_BY_INDEX,&status,&size,WINHTTP_NO_HEADER_INDEX))return last_error();
    if(status!=200)return HRESULT_FROM_WIN32(ERROR_WINHTTP_INVALID_SERVER_RESPONSE);
    std::string body;char buffer[8192];
    for(;;){if(stopped(epoch,cancel))return E_ABORT;DWORD got=0;if(!WinHttpReadData(request.value,buffer,sizeof(buffer),&got))return last_error();if(!got)break;if(body.size()+got>max_document)return HRESULT_FROM_WIN32(ERROR_FILE_TOO_LARGE);body.append(buffer,got);}
    auto parsed=std::make_shared<const Json>(Json::parse(body));
    if(stopped(epoch,cancel))return E_ABORT;
    {std::lock_guard<std::mutex> guard(policy_lock);if(stopped(epoch,cancel))return E_ABORT;cached_policy=parsed;cached_at=GetTickCount64();}
    out=std::move(parsed);return S_OK;
}
struct AsyncContext {Url url;UINT64 epoch=generation.load();std::atomic<bool> cancelled{false};XNetworkingSecurityInformation info{};};
HRESULT WINAPI provider(XAsyncOp op,const XAsyncProviderData *data){
    auto ctx=static_cast<AsyncContext*>(data->context);
    try{
        switch(op){
        case XAsyncOp::Begin:return XAsyncSchedule(data->async,0);
        case XAsyncOp::DoWork:{
            std::shared_ptr<const Json> policy;HRESULT hr=download_policy(ctx->epoch,&ctx->cancelled,policy);const char *status="fetch-failed";
            std::shared_ptr<const Json> title;{std::lock_guard<std::mutex> guard(policy_lock);title=title_policy;}
            if(SUCCEEDED(hr))hr=select_combined_policy(*policy,title.get(),ctx->url,ctx->info,&status);
            if(stopped(ctx->epoch,&ctx->cancelled))hr=E_ABORT;
            diagnostic(ctx->url,hr,status);XAsyncComplete(data->async,hr,SUCCEEDED(hr)?sizeof(ctx->info):0);return E_PENDING;
        }
        case XAsyncOp::GetResult:
            if(stopped(ctx->epoch,&ctx->cancelled))return E_ABORT;
            if(!data->buffer||data->bufferSize<sizeof(ctx->info))return HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER);
            std::memcpy(data->buffer,&ctx->info,sizeof(ctx->info));return S_OK;
        case XAsyncOp::Cancel:ctx->cancelled=true;return S_OK;
        case XAsyncOp::Cleanup:delete ctx;return S_OK;
        }
    }catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);}
    return E_UNEXPECTED;
}
HRESULT verify_certificate(PCCERT_CONTEXT certificate,LPCWSTR host,const FILETIME *time){
    if(!certificate||!host||!*host)return E_POINTER;
    CERT_CHAIN_PARA parameters{};parameters.cbSize=sizeof(parameters);
    LPSTR oid=const_cast<LPSTR>(szOID_PKIX_KP_SERVER_AUTH);parameters.RequestedUsage.dwType=USAGE_MATCH_TYPE_AND;parameters.RequestedUsage.Usage.cUsageIdentifier=1;parameters.RequestedUsage.Usage.rgpszUsageIdentifier=&oid;
    PCCERT_CHAIN_CONTEXT chain=nullptr;
    if(!CertGetCertificateChain(nullptr,certificate,const_cast<FILETIME*>(time),certificate->hCertStore,&parameters,0,nullptr,&chain))return last_error();
    SSL_EXTRA_CERT_CHAIN_POLICY_PARA ssl{};ssl.cbSize=sizeof(ssl);ssl.dwAuthType=AUTHTYPE_SERVER;ssl.pwszServerName=const_cast<LPWSTR>(host);
    CERT_CHAIN_POLICY_PARA policy{};policy.cbSize=sizeof(policy);policy.pvExtraPolicyPara=&ssl;
    CERT_CHAIN_POLICY_STATUS status{};status.cbSize=sizeof(status);
    BOOL ok=CertVerifyCertificateChainPolicy(CERT_CHAIN_POLICY_SSL,chain,&policy,&status);
    HRESULT hr=ok?static_cast<HRESULT>(status.dwError):last_error();CertFreeCertificateChain(chain);return hr;
}
}

HRESULT NetworkSecurityQuery(LPCWSTR url,XAsyncBlock *async){
    if(!url||!async)return E_POINTER;
    try{
        auto ctx=std::make_unique<AsyncContext>();HRESULT hr=parse_url(url,ctx->url);if(FAILED(hr))return hr;
        hr=XAsyncBegin(async,ctx.get(),&security_identity,"XNetworkingSecurityInformation",provider);
        if(SUCCEEDED(hr))ctx.release();return hr;
    }catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return E_INVALIDARG;}
}
HRESULT NetworkSecurityQueryUtf8(LPCSTR url,XAsyncBlock *async){
    if(!url||!async)return E_POINTER;
    try{size_t length=0;while(length<=max_url&&url[length])++length;if(!length||length>max_url)return E_INVALIDARG;
        int count=MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,url,static_cast<int>(length+1),nullptr,0);if(!count)return last_error();
        std::vector<WCHAR> wide(count);if(!MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,url,static_cast<int>(length+1),wide.data(),count))return last_error();return NetworkSecurityQuery(wide.data(),async);
    }catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
}
HRESULT NetworkSecurityResultSize(XAsyncBlock *async,SIZE_T *size){if(!size)return E_POINTER;*size=0;if(!async)return E_POINTER;return XAsyncGetResultSize(async,size);}
HRESULT NetworkSecurityResult(XAsyncBlock *async,SIZE_T size,SIZE_T *used,UINT8 *buffer,XNetworkingSecurityInformation **info){
    if(used)*used=0;if(!info)return E_POINTER;*info=nullptr;if(!async||!buffer)return E_POINTER;
    if(reinterpret_cast<uintptr_t>(buffer)%alignof(XNetworkingSecurityInformation))return E_INVALIDARG;
    // The imported PR60 XAsyncGetResult consumes state even on a short buffer.
    // Validate capacity first so the documented two-call pattern remains usable.
    SIZE_T required=0;HRESULT status=XAsyncGetResultSize(async,&required);if(FAILED(status))return status;
    if(required<sizeof(XNetworkingSecurityInformation))return E_INVALIDARG;
    if(size<required)return HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER);
    HRESULT hr=XAsyncGetResult(async,&security_identity,size,buffer,used);if(SUCCEEDED(hr))*info=reinterpret_cast<XNetworkingSecurityInformation*>(buffer);return hr;
}
HRESULT NetworkSecurityVerify(void *request,const XNetworkingSecurityInformation *info){
    auto finish=[](HRESULT hr,const char *stage){if(verify_diagnostic_count.fetch_add(1)<64)std::fprintf(stderr,"[xodus-network] VerifyServerCertificate stage=%s result=%08lx\n",stage,static_cast<unsigned long>(hr));return hr;};
    if(!request||!info)return finish(E_POINTER,"arguments");
    if(info->thumbprintCount)return finish(E_NOTIMPL,"pins-unsupported"); // Never claim a supplied pin was checked.
    if(info->thumbprints||info->enabledHttpSecurityProtocolFlags!=WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2)return finish(E_INVALIDARG,"policy-structure");
    try{
        DWORD flags=0,size=sizeof(flags);
        if(!WinHttpQueryOption(request,WINHTTP_OPTION_SECURITY_FLAGS,&flags,&size))return finish(last_error(),"flags-option");
        constexpr DWORD ignored=SECURITY_FLAG_IGNORE_UNKNOWN_CA|SECURITY_FLAG_IGNORE_CERT_DATE_INVALID|SECURITY_FLAG_IGNORE_CERT_CN_INVALID|SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE;
        if(flags&ignored)return finish(HRESULT_FROM_WIN32(ERROR_WINHTTP_SECURE_FAILURE),"ignore-flags");
        // Protocol selection belongs to WinHttpSetOption(SECURE_PROTOCOLS), using
        // the flags returned by SecurityQuery. This API validates certificates.
        // The selected Wine runner does not expose SECURITY_INFO (option 151),
        // so do not claim that its negotiated protocol was queried here.
        size=0;WinHttpQueryOption(request,WINHTTP_OPTION_URL,nullptr,&size);if(!size||size>(max_url+1)*sizeof(WCHAR))return finish(E_INVALIDARG,"url-size");
        std::vector<WCHAR> text(size/sizeof(WCHAR)+1);if(!WinHttpQueryOption(request,WINHTTP_OPTION_URL,text.data(),&size))return finish(last_error(),"url-option");
        Url url;HRESULT hr=parse_url(text.data(),url);if(FAILED(hr))return finish(hr,"url-parse");
        // Verify the handle still targets a matched unpinned policy. Redirects or
        // a fabricated empty structure cannot turn a pinned/unknown URL into one.
        std::shared_ptr<const Json> policy,title;UINT64 epoch;
        {std::lock_guard<std::mutex> guard(policy_lock);if(!cached_policy||GetTickCount64()-cached_at>=60*60*1000)return finish(HRESULT_FROM_WIN32(ERROR_NOT_FOUND),"policy-cache");policy=cached_policy;title=title_policy;epoch=generation.load();}
        XNetworkingSecurityInformation expected{};const char *match=nullptr;hr=select_combined_policy(*policy,title.get(),url,expected,&match);if(FAILED(hr))return finish(hr,"policy-match");
        PCCERT_CONTEXT certificate=nullptr;size=sizeof(certificate);
        if(!WinHttpQueryOption(request,WINHTTP_OPTION_SERVER_CERT_CONTEXT,&certificate,&size))return finish(last_error(),"certificate-option");
        hr=verify_certificate(certificate,url.host.c_str(),nullptr);if(certificate)CertFreeCertificateContext(certificate);
        if(stopped(epoch,nullptr))return finish(E_ABORT,"policy-changed");
        return finish(hr,"cert-chain");
    }catch(const std::bad_alloc&){return finish(E_OUTOFMEMORY,"allocation");}catch(...){return finish(HRESULT_FROM_WIN32(ERROR_INVALID_DATA),"invalid-data");}
}
extern "C" HRESULT WINAPI XodusPublishTitleEndpointPolicy(UINT32 titleId,const char *json,SIZE_T length){
    if(!json)return E_POINTER;
    if(!titleId||!length||length>max_document)return E_INVALIDARG;
    UINT64 epoch;
    {std::lock_guard<std::mutex> guard(policy_lock);if(publication_closed)return E_ABORT;epoch=generation.load();}
    try{
        Json parsed=parse_unique_document(json,length);HRESULT hr=validate_title_document(parsed);if(FAILED(hr))return hr;
        auto installed=std::make_shared<const Json>(std::move(parsed));
#ifdef NETWORK_SECURITY_TESTING
        if(auto barrier=publish_barrier.load())barrier();
#endif
        std::lock_guard<std::mutex> guard(policy_lock);
        if(publication_closed||generation.load()!=epoch)return E_ABORT;
        if(published_title_id&&published_title_id!=titleId)return E_INVALIDARG;
        if(title_policy&&*title_policy==*installed)return S_OK;
        title_policy=std::move(installed);published_title_id=titleId;
        // No derived match cache: invalidate in-flight query results and
        // verification snapshots while retaining the independently fetched
        // default document and its original one-hour freshness deadline.
        generation.fetch_add(1);
        return S_OK;
    }catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);}
}
void NetworkSecurityShutdown(){std::lock_guard<std::mutex> guard(policy_lock);generation.fetch_add(1);cached_policy.reset();cached_at=0;title_policy.reset();published_title_id=0;publication_closed=true;}
#ifdef NETWORK_SECURITY_TESTING
HRESULT NetworkSecurityTestPolicy(const char *document,LPCWSTR url,XNetworkingSecurityInformation *result){
    if(!document||!result)return E_POINTER;*result={};try{Url parsed;HRESULT hr=parse_url(url,parsed);if(FAILED(hr))return hr;const char *match;return select_policy(Json::parse(document),parsed,*result,&match);}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);}
}
HRESULT NetworkSecurityTestCertificate(PCCERT_CONTEXT certificate,LPCWSTR host,const FILETIME *time){return verify_certificate(certificate,host,time);}
HRESULT NetworkSecurityTestCurrentPolicy(LPCWSTR url,XNetworkingSecurityInformation *result){
    if(!result)return E_POINTER;*result={};try{
        Url parsed;HRESULT hr=parse_url(url,parsed);if(FAILED(hr))return hr;
        std::shared_ptr<const Json> base,title;{std::lock_guard<std::mutex> guard(policy_lock);base=cached_policy;title=title_policy;}
        if(!base)return HRESULT_FROM_WIN32(ERROR_NOT_FOUND);const char *status;
        return select_combined_policy(*base,title.get(),parsed,*result,&status);
    }catch(...){return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);}
}
HRESULT NetworkSecurityTestReset(const char *defaults){
    try{auto parsed=std::make_shared<const Json>(Json::parse(defaults));std::lock_guard<std::mutex> guard(policy_lock);generation.fetch_add(1);cached_policy=std::move(parsed);cached_at=GetTickCount64();title_policy.reset();published_title_id=0;publication_closed=false;return S_OK;}
    catch(...){return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);}
}
void NetworkSecurityTestSetPublishBarrier(void(*barrier)()){publish_barrier=barrier;}
#endif

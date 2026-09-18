/* SPDX-License-Identifier: LGPL-2.1-or-later */
// Fixed-origin ConnectedStorage reader. Auth/signatures never cross this pipe.
#include <initguid.h>
#include "compat.h"
#include <xuser.h>
#include <xgame.h>
#include <fcntl.h>
#include <io.h>
#include <fstream>
#include <bcrypt.h>
#include "ConnectedStorageWrite.h"
#include "ConnectedStorageProtocol.h"
// WinHTTP's headers conflict with Wine-generated COM headers: separate TU.
HRESULT connected_get(const std::string&,const std::string&,bool,const char*,const char*,size_t,DWORD*,connected_storage::Json*,std::string*);
HRESULT connected_request(const std::string&,const std::string&,const std::string&,const std::vector<std::pair<std::string,std::string>>&,const std::string&,size_t,DWORD*,connected_storage::Json*,std::string*);
using namespace connected_storage;
using Query=HRESULT(WINAPI*)(const GUID*,REFIID,void**);
struct Options {UINT32 unknown;BOOLEAN inline_config;const char*xml;};
using InitFn=HRESULT(WINAPI*)(ULONG,ULONG,char,const Options*);
using Stop=void(WINAPI*)();
using CloudWriteToken=HRESULT(WINAPI*)(XUserHandle,const char*,const char*,SIZE_T,const XUserGetTokenAndSignatureHttpHeader*,SIZE_T,const void*,XAsyncBlock*);
using CloudToken=HRESULT(WINAPI*)(XUserHandle,const char*,SIZE_T,const XUserGetTokenAndSignatureHttpHeader*,XAsyncBlock*);
static bool read_exact(void*out,size_t n){return !n||fread(out,1,n,stdin)==n;}
static bool read_frame(Json&j) {
 unsigned char length[4];if(!read_exact(length,4))return false;
 uint32_t n=uint32_t(length[0])|(uint32_t(length[1])<<8)|(uint32_t(length[2])<<16)|(uint32_t(length[3])<<24);
 if(!n||n>frame_limit)throw Invalid();std::string data(n,'\0');if(!read_exact(data.data(),n))throw Invalid();
 j=Json::parse(data,nullptr,false);if(j.is_discarded()||!j.is_object())throw Invalid();return true;
}
static void reply(const Json&j,const std::string&body={}) {
 auto data=j.dump();uint32_t n=uint32_t(data.size());unsigned char length[]={static_cast<unsigned char>(n),static_cast<unsigned char>(n>>8),static_cast<unsigned char>(n>>16),static_cast<unsigned char>(n>>24)};
 if(fwrite(length,1,4,stdout)!=4||fwrite(data.data(),1,data.size(),stdout)!=data.size()||fwrite(body.data(),1,body.size(),stdout)!=body.size()||fflush(stdout))ExitProcess(112);
}
static void failure(const char*code,HRESULT hr=E_FAIL,DWORD status=0){reply({{"ok",false},{"error",code},{"hresult",uint32_t(hr)},{"http_status",status}});}
static HRESULT wait(IXThreadingImpl*t,XAsyncBlock*a) {
 auto until=GetTickCount64()+90000;HRESULT hr;
 while((hr=t->XAsyncGetStatus(a,FALSE))==E_PENDING&&GetTickCount64()<until)Sleep(10);
 if(hr==E_PENDING){t->XAsyncCancel(a);until=GetTickCount64()+10000;while(t->XAsyncGetStatus(a,FALSE)==E_PENDING&&GetTickCount64()<until)Sleep(10);if(t->XAsyncGetStatus(a,FALSE)==E_PENDING)ExitProcess(111);return HRESULT_FROM_WIN32(ERROR_TIMEOUT);}return hr;
}
struct Runtime {
 Stop stop=nullptr;CloudToken cloud_token=nullptr;CloudWriteToken cloud_write=nullptr;Lease lease;std::string lock_ext;IXThreadingImpl*t=nullptr;IXUserImpl6*u=nullptr;XTaskQueueHandle q=nullptr;XUserHandle user=nullptr;uint64_t id=0;
 ~Runtime(){if(user&&u)u->XUserCloseHandle(user);if(q&&t)t->XTaskQueueCloseHandle(q);if(u)u->Release();if(t)t->Release();if(stop)stop();}
 HRESULT init(const Init&in) {
  wchar_t path[MAX_PATH];UINT n=GetSystemDirectoryW(path,MAX_PATH);if(!n||n>MAX_PATH-20)return E_FAIL;
  std::wstring dll(path,n);dll+=L"\\xgameruntime.dll";auto module=LoadLibraryW(dll.c_str());if(!module)return HRESULT_FROM_WIN32(GetLastError());
  auto initialize=reinterpret_cast<InitFn>(GetProcAddress(module,"InitializeApiImplEx2"));auto query=reinterpret_cast<Query>(GetProcAddress(module,"QueryApiImpl"));auto close=reinterpret_cast<Stop>(GetProcAddress(module,"UninitializeApiImpl"));if(!initialize||!query||!close)return E_NOTIMPL;
  Options options{0,TRUE,in.config.c_str()};HRESULT hr=initialize(250600,0,0,&options);if(FAILED(hr))return hr;stop=close;
  // Use only the kernel already loaded by the initialized runtime. This
  // explicit export owns a separate User+Device cache; normal game auth stays
  // title-bound, including the TitleHub request used to establish our SCID.
  auto kernel=GetModuleHandleW(L"xodus_store_test.dll");
  if(!kernel||!(cloud_token=reinterpret_cast<CloudToken>(GetProcAddress(kernel,"XodusUserGetConnectedStorageTokenAndSignatureAsync"))))return E_NOTIMPL;
  cloud_write=reinterpret_cast<CloudWriteToken>(GetProcAddress(kernel,"XodusUserGetConnectedStorageWriteTokenAndSignatureAsync"));
  IXGameImpl*game=nullptr;hr=query(&CLSID_XGameImpl,IID_IXGameImpl,reinterpret_cast<void**>(&game));if(FAILED(hr)||!game)return FAILED(hr)?hr:E_FAIL;
  UINT32 title=0;hr=game->XGameGetXboxTitleId(&title);game->Release();if(FAILED(hr)||title!=in.title)return E_ACCESSDENIED;
  if(FAILED(hr=query(&CLSID_XThreadingImpl,IID_IXThreadingImpl,reinterpret_cast<void**>(&t)))||FAILED(hr=query(&CLSID_XUserImpl,IID_IXUserImpl6,reinterpret_cast<void**>(&u))))return hr;
  if(FAILED(hr=t->XTaskQueueCreate(XTaskQueueDispatchMode::ThreadPool,XTaskQueueDispatchMode::ThreadPool,&q)))return hr;
  XAsyncBlock a{};a.queue=q;hr=u->XUserAddAsync(XUserAddOptions::AddDefaultUserSilently,&a);if(FAILED(hr))return hr;if(FAILED(hr=wait(t,&a)))return hr;
  if(FAILED(hr=u->XUserAddResult(&a,&user))||!user)return FAILED(hr)?hr:E_FAIL;
  if(FAILED(hr=u->XUserGetId(user,&id))||!id)return E_ACCESSDENIED;return S_OK;
 }
 HRESULT send(const Init&in,const std::string&method,const Read&r,const std::string&input,DWORD*status,Json*headers,std::string*body,bool titlehub=false,bool lock=false) {
  uint64_t current=0;HRESULT hr=u->XUserGetId(user,&current);if(FAILED(hr)||current!=id){lease.lost();return E_ACCESSDENIED;}
  std::string url=std::string(titlehub?"https://titlehub.xboxlive.com":"https://titlestorage.xboxlive.com")+r.path;
  std::vector<std::pair<std::string,std::string>> fields={{"x-xbl-contract-version",titlehub?"2":"107"},{titlehub?"Accept-Language":"x-xbl-pfn",titlehub?"en-US":in.family},{"Accept-Encoding","identity"}};
  if(!titlehub&&(lock||lease.held)){if(lock_ext.empty())return E_ACCESSDENIED;fields.emplace_back("x-xbl-lock-ext",lock_ext);fields.emplace_back("x-xbl-lock-ver","1");}
  if(method=="POST"||(!input.empty()&&method=="PUT"))fields.emplace_back("Content-Type","application/json");
  std::vector<XUserGetTokenAndSignatureHttpHeader> h;for(const auto&v:fields)h.push_back({v.first.c_str(),v.second.c_str()});
  XAsyncBlock a{};a.queue=q;
  if(titlehub)hr=u->XUserGetTokenAndSignatureAsync(user,XUserGetTokenAndSignatureOptions::None,"GET",url.c_str(),h.size(),h.data(),0,nullptr,&a);
  else if(method=="GET")hr=cloud_token(user,url.c_str(),h.size(),h.data(),&a);
  else if(cloud_write)hr=cloud_write(user,method.c_str(),url.c_str(),h.size(),h.data(),input.size(),input.data(),&a);
  else return E_NOTIMPL;
  if(FAILED(hr))return hr;if(FAILED(hr=wait(t,&a)))return hr;
  SIZE_T size=0;hr=u->XUserGetTokenAndSignatureResultSize(&a,&size);if(FAILED(hr)||size<sizeof(XUserGetTokenAndSignatureData)||size>1024*1024)return FAILED(hr)?hr:E_FAIL;
  std::vector<BYTE> bytes(size);XUserGetTokenAndSignatureData*data=nullptr;SIZE_T used=0;hr=u->XUserGetTokenAndSignatureResult(&a,size,bytes.data(),&data,&used);
  if(SUCCEEDED(hr)&&data&&data->token&&*data->token&&data->signature&&*data->signature){
   fields.emplace_back("Authorization",data->token);fields.emplace_back("Signature",data->signature);
   hr=connected_request(titlehub?"titlehub.xboxlive.com":"titlestorage.xboxlive.com",method,r.path,fields,input,r.maximum,status,headers,body);
  }else if(SUCCEEDED(hr))hr=E_ACCESSDENIED;
  for(auto&f:fields)if(f.first=="Authorization"||f.first=="Signature")SecureZeroMemory(f.second.data(),f.second.size());
  SecureZeroMemory(bytes.data(),bytes.size());current=0;if(FAILED(u->XUserGetId(user,&current))||current!=id){body->clear();lease.lost();return E_ACCESSDENIED;}
  if((lock||lease.held)&&(FAILED(hr)||*status==403||*status==409))lease.lost();return hr;
 }
 HRESULT get(const Init&in,const Read&r,DWORD*s,Json*h,std::string*b,bool titlehub=false){return send(in,"GET",r,{},s,h,b,titlehub);}
 HRESULT seed(const Init&in) {
  if(!lock_ext.empty())return S_OK;if(in.device_file.empty())return E_ACCESSDENIED;
  int n=MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,in.device_file.data(),int(in.device_file.size()),nullptr,0);if(n<=0)return E_INVALIDARG;
  std::wstring path(n,L'\0');MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,in.device_file.data(),int(in.device_file.size()),path.data(),n);
  bool created=true;HANDLE file=CreateFileW(path.c_str(),GENERIC_READ|GENERIC_WRITE,0,nullptr,CREATE_NEW,FILE_ATTRIBUTE_NORMAL|FILE_FLAG_OPEN_REPARSE_POINT|FILE_FLAG_WRITE_THROUGH,nullptr);
  if(file==INVALID_HANDLE_VALUE&&GetLastError()==ERROR_FILE_EXISTS){created=false;file=CreateFileW(path.c_str(),GENERIC_READ,0,nullptr,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,nullptr);}
  if(file==INVALID_HANDLE_VALUE)return E_ACCESSDENIED;
  unsigned char value[32]{};BY_HANDLE_FILE_INFORMATION info{};DWORD used=0;HRESULT hr=E_FAIL;
  if(!GetFileInformationByHandle(file,&info)||(info.dwFileAttributes&(FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_DIRECTORY))||info.nNumberOfLinks!=1)goto done;
  if(created){if(BCryptGenRandom(nullptr,value,sizeof(value),BCRYPT_USE_SYSTEM_PREFERRED_RNG)<0||!WriteFile(file,value,sizeof(value),&used,nullptr)||used!=sizeof(value)||!FlushFileBuffers(file))goto done;}
  else if(info.nFileSizeHigh||info.nFileSizeLow!=sizeof(value)||!ReadFile(file,value,sizeof(value),&used,nullptr)||used!=sizeof(value))goto done;
  {static const char hex[]="0123456789abcdef";for(auto c:value){lock_ext+=hex[c>>4];lock_ext+=hex[c&15];}lock_ext+="_"+std::to_string(id);hr=S_OK;}
  done:SecureZeroMemory(value,sizeof(value));CloseHandle(file);return hr;
 }
 HRESULT lock_request(const Init&in,const std::string&base,bool renewal,DWORD*status,Json*safe) {
  if(!cloud_write||FAILED(seed(in))||(renewal&&!lease.held))return E_ACCESSDENIED;
  Json headers;std::string body;auto hr=send(in,"PUT",{base+"/lock?friendlyName=Flightdeck",json_limit},{},status,&headers,&body,false,true);
  if(FAILED(hr))return hr;
  try{auto j=Json::parse(body,nullptr,false);lease.result(*status,j,renewal);
   if(*status==200||*status==201)*safe={{"owner_change_id",string(j,"ownerChangeId")},{"quota_bytes",j.value("quotaBytes",uint64_t(256*1024*1024))}};
  }catch(...){lease.lost();return E_FAIL;}
  return S_OK;
 }

 HRESULT atom_upload(const Init&in,const std::string&base,const std::string&payload,DWORD*status,Json*result) {
  if(!lease.held||lease.uncertain)return E_ACCESSDENIED;
  GUID uuid{};if(FAILED(CoCreateGuid(&uuid)))return E_FAIL;wchar_t wideGuid[40];if(!StringFromGUID2(uuid,wideGuid,40))return E_FAIL;
  std::string atom;for(size_t i=1;i<=36;++i)atom+=char(wideGuid[i]);
  Json headers,confirmed;std::string body;const auto path=base+"/atoms/"+atom;
  auto hr=lock_request(in,base,true,status,&confirmed);if(FAILED(hr)||*status!=200||!lease.held)return FAILED(hr)?hr:E_ACCESSDENIED;
  hr=send(in,"POST",{path,json_limit},"{size: "+std::to_string(payload.size())+"}",status,&headers,&body,false,true);
  if(FAILED(hr)||*status<200||*status>=300)return hr;
  Azure azure;try{azure=azure_url(string(Json::parse(body),"blobUri"));}catch(...){lease.lost();return E_FAIL;}
  SecureZeroMemory(body.data(),body.size());body.clear();Json blocks=Json::array();
  // The allocation returns an account SAS, not a caller-selectable URL. Each
  // 4 MiB block is addressed with the base64 of its four-byte little-endian ID.
  for(size_t offset=0,index=0;;++index) {
   static const char chars[]="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
   unsigned char b[4]={static_cast<unsigned char>(index),static_cast<unsigned char>(index>>8),static_cast<unsigned char>(index>>16),static_cast<unsigned char>(index>>24)};
   std::string id;id+=chars[b[0]>>2];id+=chars[((b[0]&3)<<4)|(b[1]>>4)];id+=chars[((b[1]&15)<<2)|(b[2]>>6)];id+=chars[b[2]&63];id+=chars[b[3]>>2];id+=chars[(b[3]&3)<<4];id+="==";
   const size_t n=std::min(size_t(4*1024*1024),payload.size()-offset);
   auto target=azure.path;auto q=target.find('?');target.insert(q+1,"comp=block&blockid="+percent(id)+"&");
   Json h;std::string discarded;hr=connected_request(azure.host,"PUT",target,{{"x-ms-blob-type","BlockBlob"}},payload.substr(offset,n),json_limit,status,&h,&discarded);
   if(FAILED(hr)||*status<200||*status>=300){lease.lost();return hr;}
   blocks.push_back(id);offset+=n;if(offset==payload.size())break;
  }
  Json owned;hr=lock_request(in,base,true,status,&owned);if(FAILED(hr)||*status!=200||!lease.held)return FAILED(hr)?hr:E_ACCESSDENIED;
  hr=send(in,"POST",{path+"?commit=true",json_limit},Json({{"blockIds",blocks},{"size",payload.size()}}).dump(),status,&headers,&body,false,true);
  SecureZeroMemory(azure.path.data(),azure.path.size());
  if(SUCCEEDED(hr)&&*status>=200&&*status<300)*result={{"atom",atom}};return hr;
 }

};
int main(){_setmode(_fileno(stdin),_O_BINARY);_setmode(_fileno(stdout),_O_BINARY);setvbuf(stdout,nullptr,_IONBF,0);
 try {
  Json j;if(!read_frame(j))return 0;auto init=initialize(j);Runtime runtime;auto hr=runtime.init(init);if(FAILED(hr)){failure("authentication",hr);return 1;}
  if(init.scid.empty()) {
   Read query{"/users/xuid("+std::to_string(runtime.id)+")/titles/titlehistory/decoration/scid,image,detail",json_limit};
   DWORD status=0;Json headers;std::string body;hr=runtime.get(init,query,&status,&headers,&body,true);
   if(FAILED(hr)||status!=200){failure(status==401||status==403?"authentication":"title_binding",FAILED(hr)?hr:E_ACCESSDENIED,status);return 1;}
   auto metadata=Json::parse(body,nullptr,false);try{init.scid=title_scid(metadata,init.title,init.family,std::to_string(runtime.id));}catch(const Invalid&){failure("title_binding",E_INVALIDARG,status);return 1;}
  }
  std::string base="/connectedstorage/users/xuid("+std::to_string(runtime.id)+")/scids/"+init.scid;
  reply({{"ok",true},{"protocol",2},{"features",runtime.cloud_write?Json::array({"connected-storage-read-v1","connected-storage-sync-v1"}):Json::array({"connected-storage-read-v1"})},{"scope",{{"xuid",std::to_string(runtime.id)},{"scid",init.scid},{"package_family_name",init.family},{"title_id",init.title}}}});
  unsigned requests=0;while(read_frame(j)) {
   if(++requests>8192)throw Invalid();DWORD status=0;Json headers=Json::object();std::string body;
   auto op=string(j,"op");
   if(op=="index"||op=="container"||op=="atom") {
    auto r=request(j,base);hr=runtime.get(init,r,&status,&headers,&body);
   } else {
    const auto w=write_request(j,base);std::string input(w.input,'\0');if(!read_exact(input.data(),input.size()))throw Invalid();
    Json safe=Json::object();
    if(w.op=="lease_acquire"||w.op=="lease_renew") {
     if(w.op=="lease_acquire"&&(runtime.lease.held||runtime.lease.uncertain)){failure("lease_state",E_ACCESSDENIED);continue;}
     hr=runtime.lock_request(init,base,w.op=="lease_renew",&status,&safe);body=safe.dump();
    } else if(w.op=="lease_release") {
     if(!runtime.lease.held||runtime.lease.uncertain||runtime.lock_ext.empty()){failure("lease_state",E_ACCESSDENIED);continue;}
     hr=runtime.send(init,"DELETE",{w.path,w.maximum},{},&status,&headers,&body,false,true);runtime.lease.lost();body.clear();
    } else {
     if(!runtime.lease.held||runtime.lease.uncertain){failure("lease_state",E_ACCESSDENIED);continue;}
     if(w.op=="atom_upload"){hr=runtime.atom_upload(init,base,input,&status,&safe);body=safe.dump();}
     else {
      hr=runtime.lock_request(init,base,true,&status,&safe);
      if(SUCCEEDED(hr)&&status==200&&runtime.lease.held)hr=runtime.send(init,w.method,{w.path,w.maximum},w.body,&status,&headers,&body,false,true);
      else if(SUCCEEDED(hr))hr=E_ACCESSDENIED;
     }
    }
    if(!input.empty())SecureZeroMemory(input.data(),input.size());
    if(FAILED(hr)||status<200||status>=300)body.clear();
   }
   if(FAILED(hr)){failure("transport",hr,status);continue;}
   reply({{"ok",true},{"status",status},{"headers",headers},{"body_bytes",body.size()}},body);
  }
  return 0;
 }catch(const Invalid&){failure("invalid_request",E_INVALIDARG);}catch(...){failure("internal");}return 1;
}

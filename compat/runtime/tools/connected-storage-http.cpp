/* SPDX-License-Identifier: LGPL-2.1-or-later */
#include <windows.h>
#include <winhttp.h>
#include "ConnectedStorageProtocol.h"
using connected_storage::Json;
static std::wstring wide(const std::string&s){int n=MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,s.data(),int(s.size()),nullptr,0);if(n<=0)return {};std::wstring out(n,L'\0');MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,s.data(),int(s.size()),out.data(),n);return out;}
HRESULT connected_request(const std::string&host,const std::string&method,const std::string&path,const std::vector<std::pair<std::string,std::string>>&fields,const std::string&input,size_t maximum,DWORD*status,Json*response_headers,std::string*body) {
 HRESULT hr=E_FAIL;HINTERNET session=nullptr,connection=nullptr,request=nullptr;*status=0;body->clear();
 std::wstring headers;for(const auto& field:fields){if(!connected_storage::text(field.first,128)||!connected_storage::text(field.second,128*1024))return E_INVALIDARG;headers+=wide(field.first)+L": "+wide(field.second)+L"\r\n";}
 session=WinHttpOpen(L"FlightdeckConnectedStorage/1",WINHTTP_ACCESS_TYPE_NO_PROXY,WINHTTP_NO_PROXY_NAME,WINHTTP_NO_PROXY_BYPASS,0);if(!session)goto done;
 if(!WinHttpSetTimeouts(session,15000,15000,15000,20000))goto done;
 connection=WinHttpConnect(session,wide(host).c_str(),INTERNET_DEFAULT_HTTPS_PORT,0);if(!connection)goto done;
 {auto target=wide(path);request=WinHttpOpenRequest(connection,wide(method).c_str(),target.c_str(),nullptr,WINHTTP_NO_REFERER,WINHTTP_DEFAULT_ACCEPT_TYPES,WINHTTP_FLAG_SECURE|WINHTTP_FLAG_ESCAPE_DISABLE);if(!request)goto done;}
 {DWORD redirect=WINHTTP_OPTION_REDIRECT_POLICY_NEVER,disable=WINHTTP_DISABLE_COOKIES;if(!WinHttpSetOption(request,WINHTTP_OPTION_REDIRECT_POLICY,&redirect,sizeof(redirect))||!WinHttpSetOption(request,WINHTTP_OPTION_DISABLE_FEATURE,&disable,sizeof(disable)))goto done;}
 if(!WinHttpSendRequest(request,headers.c_str(),DWORD(headers.size()),input.empty()?WINHTTP_NO_REQUEST_DATA:const_cast<char*>(input.data()),DWORD(input.size()),DWORD(input.size()),0)||!WinHttpReceiveResponse(request,nullptr))goto done;
 {DWORD n=sizeof(*status);if(!WinHttpQueryHeaders(request,WINHTTP_QUERY_STATUS_CODE|WINHTTP_QUERY_FLAG_NUMBER,WINHTTP_HEADER_NAME_BY_INDEX,status,&n,WINHTTP_NO_HEADER_INDEX))goto done;}
 if(*status<200||*status>=300){
  // Error diagnostics are a fixed vocabulary, never the remote message/code,
  // URL, account identifier or token. Error content does not leave this TU.
  if(*status==400||*status==401||*status==403) {
   std::string error;for(;;){char bytes[1024];DWORD n=0;if(!WinHttpReadData(request,bytes,sizeof(bytes),&n))break;if(!n)break;if(error.size()+n>8192)break;error.append(bytes,n);}
   for(char&c:error)if(c>='A'&&c<='Z')c=char(c+'a'-'A');
   std::string kinds;for(const char*word:{"signature","token","expired","privilege","permission","unauthorized","forbidden","lock","owner","device","title","user","scope","contract","header","language","format","invalid","missing","denied","claim","certificate","proof","policy"}) {
    if(error.find(word)!=std::string::npos){if(!kinds.empty())kinds+=',';kinds+=word;}
   }
   (*response_headers)["x-flightdeck-error-kinds"]=kinds;
   auto parsed=Json::parse(error,nullptr,false);
   if(parsed.is_object())for(const char*key:{"xerr","errorcode","code"}) {
    auto i=parsed.find(key);if(i!=parsed.end()&&i->is_number_unsigned())(*response_headers)["x-flightdeck-error-number"]=std::to_string(i->get<uint64_t>());
   }
   if(!error.empty())SecureZeroMemory(error.data(),error.size());
  }
  hr=S_OK;goto done;
 }
 for(const auto& pair:std::vector<std::pair<const wchar_t*,const char*>>{{L"Content-Type","content-type"},{L"Content-Length","content-length"},{L"Content-Encoding","content-encoding"},{L"ETag","etag"}}) {
  wchar_t value[2048];DWORD n=sizeof(value);if(WinHttpQueryHeaders(request,WINHTTP_QUERY_CUSTOM,pair.first,value,&n,WINHTTP_NO_HEADER_INDEX)) {
   std::string s;for(DWORD i=0;i<n/sizeof(wchar_t)&&value[i];++i){if(value[i]<32||value[i]>126){hr=HRESULT_FROM_WIN32(ERROR_INVALID_DATA);goto done;}s+=char(value[i]);}
   (*response_headers)[pair.second]=s;
  }else if(GetLastError()!=ERROR_WINHTTP_HEADER_NOT_FOUND)goto done;
 }
 if(response_headers->contains("content-encoding")&&(*response_headers)["content-encoding"]!="identity"){hr=HRESULT_FROM_WIN32(ERROR_INVALID_DATA);goto done;}
 for(;;){char data[16384];DWORD n=0;if(!WinHttpReadData(request,data,sizeof(data),&n))goto done;if(!n)break;if(body->size()+n>maximum){hr=HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER);goto done;}body->append(data,n);}
 hr=S_OK;
 done: if(hr==E_FAIL)hr=HRESULT_FROM_WIN32(GetLastError()?GetLastError():ERROR_GEN_FAILURE);if(FAILED(hr))body->clear();
 if(!headers.empty())SecureZeroMemory(headers.data(),headers.size()*sizeof(wchar_t));if(request)WinHttpCloseHandle(request);if(connection)WinHttpCloseHandle(connection);if(session)WinHttpCloseHandle(session);return hr;
}

HRESULT connected_get(const std::string&path,const std::string&pfn,bool titlehub,const char*token,const char*signature,size_t maximum,DWORD*status,Json*response_headers,std::string*body) {
 std::vector<std::pair<std::string,std::string>> headers={{"x-xbl-contract-version",titlehub?"2":"107"},{titlehub?"Accept-Language":"x-xbl-pfn",titlehub?"en-US":pfn},{"Accept-Encoding","identity"},{"Authorization",token},{"Signature",signature}};
 return connected_request(titlehub?"titlehub.xboxlive.com":"titlestorage.xboxlive.com","GET",path,headers,{},maximum,status,response_headers,body);
}

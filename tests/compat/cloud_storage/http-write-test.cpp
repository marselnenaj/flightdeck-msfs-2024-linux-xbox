/* SPDX-License-Identifier: LGPL-2.1-or-later */
// Calls the real WinHTTP adapter with synthetic imports: no network/account.
#include <windows.h>
#include <winhttp.h>
#include <cassert>
#include <cstring>
#include <iostream>
#include <string>
static std::wstring host,method,target,headers;
static std::string sent,response;
static DWORD response_status=200;static size_t cursor=0;static bool redirects=false,cookies=false;
static HINTERNET fake_open(LPCWSTR,DWORD,LPCWSTR,LPCWSTR,DWORD){return reinterpret_cast<HINTERNET>(1);}
static BOOL fake_timeouts(HINTERNET,int,int,int,int){return TRUE;}
static HINTERNET fake_connect(HINTERNET,LPCWSTR h,INTERNET_PORT port,DWORD){assert(port==443);host=h;return reinterpret_cast<HINTERNET>(2);}
static HINTERNET fake_request(HINTERNET,LPCWSTR m,LPCWSTR p,LPCWSTR,LPCWSTR,LPCWSTR*,DWORD flags){assert(flags&WINHTTP_FLAG_SECURE);method=m;target=p;cursor=0;return reinterpret_cast<HINTERNET>(3);}
static BOOL fake_option(HINTERNET,DWORD key,LPVOID value,DWORD size){assert(size==sizeof(DWORD));if(key==WINHTTP_OPTION_REDIRECT_POLICY)redirects=*static_cast<DWORD*>(value)==WINHTTP_OPTION_REDIRECT_POLICY_NEVER;else if(key==WINHTTP_OPTION_DISABLE_FEATURE)cookies=(*static_cast<DWORD*>(value)&WINHTTP_DISABLE_COOKIES)!=0;else assert(false);return TRUE;}
static BOOL fake_send(HINTERNET,LPCWSTR h,DWORD n,LPVOID body,DWORD bytes,DWORD total,DWORD_PTR){headers.assign(h,n);assert(bytes==total);sent.assign(bytes?static_cast<char*>(body):"",bytes);return TRUE;}
static BOOL fake_receive(HINTERNET,LPVOID){return TRUE;}
static BOOL fake_query(HINTERNET,DWORD query,LPCWSTR name,LPVOID out,LPDWORD size,LPDWORD){if(query&WINHTTP_QUERY_FLAG_NUMBER){*static_cast<DWORD*>(out)=response_status;*size=sizeof(DWORD);return TRUE;}if(std::wstring(name)==L"ETag"){const wchar_t value[]=L"synthetic-etag";assert(*size>=sizeof(value));memcpy(out,value,sizeof(value));*size=sizeof(value);return TRUE;}SetLastError(ERROR_WINHTTP_HEADER_NOT_FOUND);return FALSE;}
static BOOL fake_read(HINTERNET,LPVOID out,DWORD maximum,LPDWORD size){*size=DWORD(std::min(size_t(maximum),response.size()-cursor));memcpy(out,response.data()+cursor,*size);cursor+=*size;return TRUE;}
static BOOL fake_close(HINTERNET){return TRUE;}
#define WinHttpOpen fake_open
#define WinHttpSetTimeouts fake_timeouts
#define WinHttpConnect fake_connect
#define WinHttpOpenRequest fake_request
#define WinHttpSetOption fake_option
#define WinHttpSendRequest fake_send
#define WinHttpReceiveResponse fake_receive
#define WinHttpQueryHeaders fake_query
#define WinHttpReadData fake_read
#define WinHttpCloseHandle fake_close
#include "connected-storage-http.cpp"
int main(){unsigned count=0;auto check=[&](bool v){assert(v);++count;};DWORD status;Json received;std::string body;
 response="{\"ownerChangeId\":\"synthetic\"}";
 auto hr=connected_request("titlestorage.xboxlive.com","PUT","/fixed/lock?friendlyName=Flightdeck",{{"Authorization","synthetic-token"},{"Signature","synthetic-signature"},{"x-xbl-lock-ext","synthetic-device_123"}}, {},1024,&status,&received,&body);
 check(SUCCEEDED(hr)&&status==200&&body==response);check(method==L"PUT"&&target==L"/fixed/lock?friendlyName=Flightdeck");check(host==L"titlestorage.xboxlive.com");check(redirects&&cookies);check(headers.find(L"x-xbl-lock-ext: synthetic-device_123\r\n")!=std::wstring::npos);check(received["etag"]=="synthetic-etag");
 std::string input("a\0b\xff",4);response.clear();response_status=201;received=Json::object();
 hr=connected_request("account.blob.core.windows.net","PUT","/c/b?comp=block&blockid=AAAAAA%3D%3D&sig=synthetic",{{"x-ms-blob-type","BlockBlob"}},input,10,&status,&received,&body);
 check(SUCCEEDED(hr)&&status==201&&body.empty());check(sent==input);check(headers.find(L"Authorization")==std::wstring::npos&&headers.find(L"Signature")==std::wstring::npos);check(host==L"account.blob.core.windows.net");
 input="{\"atoms\":[]}";hr=connected_request("titlestorage.xboxlive.com","PUT","/fixed/savedgames/test",{{"Content-Type","application/json"}},input,10,&status,&received,&body);check(SUCCEEDED(hr)&&sent==input);check(method==L"PUT");
 response_status=409;response="private server content synthetic-secret";hr=connected_request("titlestorage.xboxlive.com","DELETE","/fixed/lock",{}, {},10,&status,&received,&body);check(SUCCEEDED(hr)&&status==409&&body.empty());check(method==L"DELETE");
 response_status=200;response="too long";hr=connected_request("titlestorage.xboxlive.com","GET","/fixed",{}, {},2,&status,&received,&body);check(FAILED(hr)&&body.empty());
 response_status=403;response="{\"message\":\"owner synthetic-secret\"}";received=Json::object();hr=connected_request("titlestorage.xboxlive.com","PUT","/fixed/lock",{}, {},100,&status,&received,&body);check(SUCCEEDED(hr)&&body.empty());check(received.dump().find("synthetic-secret")==std::string::npos);check(received["x-flightdeck-error-kinds"]=="owner");
 std::cout<<count<<" HTTP adapter checks PASS\n";
}

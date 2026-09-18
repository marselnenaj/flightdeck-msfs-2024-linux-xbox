/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
// Original, narrow read-only protocol. No user-selected origin or HTTP method.
#include <cstdint>
#include <set>
#include <string>
#include <stdexcept>
#include "vendor/nlohmann_json.hpp"
namespace connected_storage {
using Json=nlohmann::json;
constexpr size_t frame_limit=256*1024, json_limit=4*1024*1024, blob_limit=64*1024*1024;
struct Invalid:std::runtime_error { Invalid():std::runtime_error("invalid_request"){} };
inline bool text(const std::string&s,size_t maximum) {
 if(s.empty()||s.size()>maximum)return false;
 for(unsigned char c:s)if(c<32||c==127)return false;
 return true;
}
inline bool guid(const std::string&s) {
 if(s.size()!=36)return false;
 for(size_t i=0;i<s.size();++i) {
  char c=s[i];if(i==8||i==13||i==18||i==23){if(c!='-')return false;}
  else if(!((c>='0'&&c<='9')||(c>='a'&&c<='f')))return false;
 }return true;
}
inline bool pfn(const std::string&s) {
 if(!text(s,255))return false;
 for(unsigned char c:s)if(!((c>='a'&&c<='z')||(c>='A'&&c<='Z')||(c>='0'&&c<='9')||c=='.'||c=='_'||c=='-'))return false;
 return true;
}
inline bool atom_guid(std::string s) {
 // GUID syntax is case-insensitive, but the service's blob path is not.
 // Normalize only a validation copy and retain original bytes for requests.
 for(char&c:s)if(c>='A'&&c<='F')c=char(c+'a'-'A');
 return guid(s);
}
inline void keys(const Json&j,std::set<std::string> allowed) {
 if(!j.is_object())throw Invalid();
 for(auto i=j.begin();i!=j.end();++i)if(!allowed.count(i.key()))throw Invalid();
}
inline std::string string(const Json&j,const char*k) {
 auto i=j.find(k);if(i==j.end()||!i->is_string())throw Invalid();return i->get<std::string>();
}
inline uint64_t number(const Json&j,const char*k,uint64_t max) {
 auto i=j.find(k);if(i==j.end()||!i->is_number_unsigned())throw Invalid();
 auto n=i->get<uint64_t>();if(n>max)throw Invalid();return n;
}
inline std::string percent(const std::string&s) {
 static const char h[]="0123456789ABCDEF";std::string out;
 for(unsigned char c:s) {
  if((c>='a'&&c<='z')||(c>='A'&&c<='Z')||(c>='0'&&c<='9')||c=='-'||c=='_'||c=='.'||c=='~')out+=char(c);
  else{out+='%';out+=h[c>>4];out+=h[c&15];}
 }return out;
}
struct Init {std::string config,scid,family;uint32_t title;std::string device_file;};
inline Init initialize(const Json&j) {
 keys(j,{"op","config","scid","pfn","title_id","device_file"});if(string(j,"op")!="init")throw Invalid();
 Init out{string(j,"config"),string(j,"scid"),string(j,"pfn"),uint32_t(number(j,"title_id",UINT32_MAX))};
 if(!out.title||out.config.empty()||out.config.size()>128*1024||out.config.find('\0')!=std::string::npos||(!out.scid.empty()&&!guid(out.scid))||!pfn(out.family))throw Invalid();
 if(j.contains("device_file")){out.device_file=string(j,"device_file");if(!text(out.device_file,32760)||out.device_file.compare(0,3,"Z:/"))throw Invalid();}
 return out;
}
// TitleHub is used only when the installed config omits SCID. Do not turn a
// truncated history or an ambiguous matching title into a guessed identity.
inline std::string title_scid(const Json&j,uint32_t title,const std::string&family,const std::string&xuid) {
 if(!j.is_object()||!j.contains("titles")||!j["titles"].is_array()||j["titles"].size()>4096)throw Invalid();
 for(const char*k:{"continuationToken","nextLink"})if(j.contains(k)&&!j[k].is_null()&&j[k]!="")throw Invalid();
 if(j.contains("pagingInfo")) {
  const auto&p=j["pagingInfo"];if(!p.is_object())throw Invalid();
  if(p.contains("continuationToken")&&!p["continuationToken"].is_null()&&p["continuationToken"]!="")throw Invalid();
  if(p.contains("totalItems")&&(!p["totalItems"].is_number_unsigned()||p["totalItems"].get<uint64_t>()!=j["titles"].size()))throw Invalid();
 }
 for(const char*k:{"xuid","xboxUserId"})if(j.contains(k)&&j[k]!=xuid)throw Invalid();
 std::string found;unsigned matches=0;
 for(const auto&item:j["titles"]) {
  if(!item.is_object())throw Invalid();
  auto t=item.find("titleId"),p=item.find("pfn");if(t==item.end()||p==item.end()||!p->is_string())continue;
  bool match=t->is_number_unsigned()?t->get<uint64_t>()==title:t->is_string()&&t->get<std::string>()==std::to_string(title);
  if(!match||p->get<std::string>()!=family)continue;
  found=string(item,"serviceConfigId");for(char&c:found)if(c>='A'&&c<='F')c=char(c+'a'-'A');
  if(!guid(found)||++matches>1)throw Invalid();
 }
 if(matches!=1)throw Invalid();return found;
}

struct Read {std::string path;size_t maximum;};
inline Read request(const Json&j,const std::string&base) {
 const auto op=string(j,"op");
 if(op=="index") {
  keys(j,{"op","max_bytes","skip_items","continuation_token"});
  std::string path=base;
  if(j.contains("skip_items")||j.contains("continuation_token")) {
   auto skip=number(j,"skip_items",4096);auto token=string(j,"continuation_token");
   if(!skip||!text(token,8192))throw Invalid();
   path+="?skipItems="+std::to_string(skip)+"&continuationToken="+percent(token);
  }
  return {path,size_t(number(j,"max_bytes",json_limit))};
 }
 if(op=="container") {
  keys(j,{"op","wire_name","max_bytes"});auto name=string(j,"wire_name");
  if(!text(name,1024)||name=="."||name=="..")throw Invalid();
  // The final comma is the ConnectedStorage wire type separator, not part of
  // the container name. Preserve that exact reserved suffix on the HTTP wire.
  const std::string suffix=",savedgame";
  if(name.size()<=suffix.size()||name.compare(name.size()-suffix.size(),suffix.size(),suffix))throw Invalid();
  return {base+"/"+percent(name.substr(0,name.size()-suffix.size()))+suffix,size_t(number(j,"max_bytes",json_limit))};
 }
 if(op=="atom") {
  keys(j,{"op","atom","max_bytes"});auto atom=string(j,"atom");if(!atom_guid(atom))throw Invalid();
  return {base+"/"+atom+",binary",size_t(number(j,"max_bytes",blob_limit))};
 }
 throw Invalid();
}
}

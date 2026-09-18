/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
#include "ConnectedStorageProtocol.h"
namespace connected_storage {
// Independent implementation of the observed Microsoft/Xodus wire contract.
// No caller-supplied origin, method, headers, lock identity or SAS is accepted.
inline std::string container_name(const std::string&wire) {
 const std::string suffix=",savedgame";
 if(!text(wire,1024)||wire.size()<=suffix.size()||wire.compare(wire.size()-suffix.size(),suffix.size(),suffix))throw Invalid();
 auto name=wire.substr(0,wire.size()-suffix.size());if(name=="."||name=="..")throw Invalid();return name;
}
struct Write {std::string op,path,method,body;size_t input=0,maximum=json_limit;};
inline Write write_request(const Json&j,const std::string&base) {
 Write w;w.op=string(j,"op");w.maximum=size_t(number(j,"max_bytes",json_limit));
 if(w.op=="lease_acquire"||w.op=="lease_renew"||w.op=="lease_release") {
  keys(j,{"op","max_bytes","body_bytes"});if(number(j,"body_bytes",0))throw Invalid();
  w.path=base+"/lock";w.method=w.op=="lease_release"?"DELETE":"PUT";
  if(w.op!="lease_release")w.path+="?friendlyName=Flightdeck";
 } else if(w.op=="atom_upload") {
  keys(j,{"op","max_bytes","body_bytes"});w.input=size_t(number(j,"body_bytes",blob_limit));
 } else if(w.op=="container_put"||w.op=="container_delete") {
  auto name=container_name(string(j,"wire_name"));w.path=base+"/savedgames/"+percent(name);
  if(w.op=="container_delete") {keys(j,{"op","wire_name","max_bytes","body_bytes"});number(j,"body_bytes",0);w.method="DELETE";}
  else {
   keys(j,{"op","wire_name","display_name","modified","atoms","max_bytes","body_bytes"});number(j,"body_bytes",0);
   auto display=string(j,"display_name"),modified=string(j,"modified");
   if((!display.empty()&&!text(display,1024))||modified.size()!=20)throw Invalid();
   for(size_t i=0;i<modified.size();++i){char c=modified[i];if(i==4||i==7){if(c!='-')throw Invalid();}else if(i==10){if(c!='T')throw Invalid();}else if(i==13||i==16){if(c!=':')throw Invalid();}else if(i==19){if(c!='Z')throw Invalid();}else if(c<'0'||c>'9')throw Invalid();}
   auto atoms=j.find("atoms");if(atoms==j.end()||!atoms->is_object()||atoms->size()>16384)throw Invalid();
   Json list=Json::array();for(auto it=atoms->begin();it!=atoms->end();++it){if(!text(it.key(),256)||!it->is_string()||!atom_guid(it->get<std::string>()))throw Invalid();list.push_back({{"name",it.key()},{"atom",*it}});}
   w.body=Json({{"atoms",list}}).dump();if(w.body.size()>json_limit)throw Invalid();
   w.method="PUT";w.path+="?clientFileTime="+percent(modified);if(!display.empty())w.path+="&displayName="+percent(display);
  }
 } else throw Invalid();return w;
}
inline int unhex(char c){return c>='0'&&c<='9'?c-'0':c>='A'&&c<='F'?c-'A'+10:c>='a'&&c<='f'?c-'a'+10:-1;}
inline std::string unpercent(const std::string&s) {
 std::string out;for(size_t i=0;i<s.size();++i){if(s[i]=='%'){if(i+2>=s.size()||unhex(s[i+1])<0||unhex(s[i+2])<0)throw Invalid();out+=char(unhex(s[i+1])*16+unhex(s[i+2]));i+=2;}else out+=s[i];}return out;
}
struct Azure {std::string host,path;};
inline Azure azure_url(const std::string&url) {
 // Only a storage-account HTTPS SAS supplied by the authenticated allocation
 // response. No credentials, ports, fragments, IP literals, redirects or caller URL.
 constexpr char prefix[]="https://";if(url.compare(0,8,prefix)||!text(url,16384))throw Invalid();
 auto slash=url.find('/',8);if(slash==std::string::npos)throw Invalid();auto host=url.substr(8,slash-8);
 const std::string suffix=".blob.core.windows.net";
 if(host.size()<=suffix.size()||host.compare(host.size()-suffix.size(),suffix.size(),suffix))throw Invalid();
 auto account=host.substr(0,host.size()-suffix.size());if(account.size()<3||account.size()>24)throw Invalid();
 for(char c:account)if(!((c>='a'&&c<='z')||(c>='0'&&c<='9')))throw Invalid();
 auto path=url.substr(slash);if(path.find('#')!=std::string::npos||path.find('\\')!=std::string::npos)throw Invalid();
 auto q=path.find('?');if(q==std::string::npos||q<2||path.find('?',q+1)!=std::string::npos)throw Invalid();
 std::set<std::string> seen;bool signature=false;for(size_t p=q+1;p<=path.size();){auto e=path.find('&',p);if(e==std::string::npos)e=path.size();auto pair=path.substr(p,e-p);auto eq=pair.find('=');if(eq==std::string::npos)throw Invalid();auto key=unpercent(pair.substr(0,eq));auto value=unpercent(pair.substr(eq+1));
  auto lower=key;for(char&c:lower)if(c>='A'&&c<='Z')c=char(c+32);
  if(!seen.insert(lower).second||!text(key,64)||!text(value,8192)||lower=="comp"||lower=="blockid")throw Invalid();
  if(key=="sig")signature=true;p=e+1;
 }if(!signature)throw Invalid();return {host,path};
}
struct Lease {bool held=false,uncertain=false;std::string owner;uint64_t quota=0;
 void lost(){held=false;uncertain=true;}
 void result(unsigned status,const Json&body,bool renewal) {
  bool previously_held=held;auto previous_owner=owner;lost();
  if(status!=200&&status!=201)return;
  auto next=string(body,"ownerChangeId");if(!text(next,256))throw Invalid();
  uint64_t limit=256ULL*1024*1024;if(body.contains("quotaBytes"))limit=number(body,"quotaBytes",UINT64_MAX);
  if(!limit)throw Invalid();
  if(renewal&&(status!=200||!previously_held||next!=previous_owner)){lost();return;}
  owner=next;quota=limit;held=true;uncertain=false;
 }
};
}

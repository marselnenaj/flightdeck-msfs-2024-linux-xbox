/* SPDX-License-Identifier: LGPL-2.1-or-later */
#include "ConnectedStorageWrite.h"
#include <cassert>
#include <iostream>
using namespace connected_storage;
int main(){unsigned count=0;auto ok=[&](bool v){assert(v);++count;};auto bad=[&](auto fn){try{fn();assert(false);}catch(const Invalid&){++count;}};
 const std::string id="12345678-1234-1234-1234-123456789ABC";
 Json acquire={{"op","lease_acquire"},{"max_bytes",100u},{"body_bytes",0u}};
 auto a=write_request(acquire,"/scope");ok(a.method=="PUT"&&a.path=="/scope/lock?friendlyName=Flightdeck");
 auto release=acquire;release["op"]="lease_release";ok(write_request(release,"/scope").method=="DELETE");
 for(const char*k:{"force","breakLock","url","headers","method"}){auto j=acquire;j[k]="x";bad([&]{write_request(j,"/scope");});}
 auto payload=acquire;payload["body_bytes"]=1u;bad([&]{write_request(payload,"/scope");});
 payload["op"]="atom_upload";ok(write_request(payload,"/scope").input==1);
 payload["body_bytes"]=uint64_t(blob_limit)+1;bad([&]{write_request(payload,"/scope");});
 Json put={{"op","container_put"},{"max_bytes",100u},{"body_bytes",0u},{"wire_name","a/b,savedgame"},{"display_name","Name ?"},{"modified","2026-09-18T12:00:00Z"},{"atoms",{{"blob",id}}}};
 auto w=write_request(put,"/scope");ok(w.method=="PUT");ok(w.path=="/scope/savedgames/a%2Fb?clientFileTime=2026-09-18T12%3A00%3A00Z&displayName=Name%20%3F");
 ok(Json::parse(w.body)==Json({{"atoms",Json::array({{{"name","blob"},{"atom",id}}})}}));
 for(const auto&wire:{"x,binary","..,savedgame",",savedgame","x\n,savedgame"}){auto j=put;j["wire_name"]=wire;bad([&]{write_request(j,"/scope");});}
 auto j=put;j["modified"]="bad";bad([&]{write_request(j,"/scope");});j=put;j["atoms"]["blob"]="invalid";bad([&]{write_request(j,"/scope");});
 j=put;j["display_name"]="header\r\nX: injected";bad([&]{write_request(j,"/scope");});
 j={{"op","container_delete"},{"wire_name","name,savedgame"},{"body_bytes",0u},{"max_bytes",0u}};ok(write_request(j,"/scope").path=="/scope/savedgames/name");
 Lease lease;lease.result(409,Json(),false);ok(!lease.held&&lease.uncertain);
 auto response=Json({{"ownerChangeId","synthetic-owner"},{"quotaBytes",100u}});
 lease.result(201,response,false);ok(lease.held&&lease.quota==100);
 lease.result(200,response,true);ok(lease.held);
 lease.result(201,response,true);ok(!lease.held&&lease.uncertain);
 lease.result(201,response,false);lease.result(409,Json(),true);ok(!lease.held);
 lease.result(201,response,false);auto other=response;other["ownerChangeId"]="other";lease.result(200,other,true);ok(!lease.held);
 lease.result(201,response,false);other=response;other.erase("ownerChangeId");bad([&]{lease.result(200,other,true);});ok(!lease.held);
 other=response;other["quotaBytes"]=0u;bad([&]{lease.result(201,other,false);});ok(!lease.held);
 const std::string sas="https://account123.blob.core.windows.net/container/blob?sv=2024-01-01&sp=w&sig=synthetic%2Bsignature";
 auto azure=azure_url(sas);ok(azure.host=="account123.blob.core.windows.net");ok(azure.path.find("sig=")!=std::string::npos);
 for(auto url:{"http://account123.blob.core.windows.net/c/b?sig=x","https://evil.invalid/c/b?sig=x","https://account123.blob.core.windows.net.evil.invalid/c/b?sig=x","https://account123.blob.core.windows.net:443/c/b?sig=x","https://user@account123.blob.core.windows.net/c/b?sig=x","https://account123.blob.core.windows.net/c/b?sig=x#fragment","https://account123.blob.core.windows.net/c/b?sig=x&COMP=block","https://account123.blob.core.windows.net/c/b?sig=x&blockId=y","https://account123.blob.core.windows.net/c/b?sig=x&sig=y","https://account123.blob.core.windows.net/c/b?sp=w","https://account123.blob.core.windows.net/c/b?sig=%0A","https://account123.blob.core.windows.net/c/b?sig=%"})bad([&]{azure_url(url);});
 std::cout<<count<<" write protocol checks PASS\n";
}

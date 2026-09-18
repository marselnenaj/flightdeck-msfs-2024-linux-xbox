/* SPDX-License-Identifier: LGPL-2.1-or-later */
#include "ConnectedStorageProtocol.h"
#include <cassert>
#include <iostream>
using namespace connected_storage;
int main(){unsigned passed=0;auto ok=[&](bool v){assert(v);++passed;};auto bad=[&](Json j){try{request(j,"/fixed");assert(false);}catch(const Invalid&){++passed;}};
 auto init=initialize(Json::parse(R"({"op":"init","config":"<Game/>","scid":"12345678-1234-1234-1234-123456789abc","pfn":"Test.Game_abc","title_id":123})"));ok(init.title==123);
 ok(guid("12345678-1234-1234-1234-123456789abc"));ok(!guid("../x"));ok(!pfn("x\r\nAuthorization: token"));
 ok(request(Json::parse(R"({"op":"index","max_bytes":4096})"),"/fixed").path=="/fixed");
 ok(request(Json::parse(R"({"op":"index","max_bytes":4096,"skip_items":2,"continuation_token":"opaque/+=="})"),"/fixed").path=="/fixed?skipItems=2&continuationToken=opaque%2F%2B%3D%3D");
 for(auto s:{R"({"op":"index","max_bytes":1,"skip_items":0,"continuation_token":"x"})",R"({"op":"index","max_bytes":1,"skip_items":4097,"continuation_token":"x"})",R"({"op":"index","max_bytes":1,"skip_items":1})",R"({"op":"index","max_bytes":1,"continuation_token":"x"})",R"({"op":"index","max_bytes":1,"skip_items":1,"continuation_token":"x\nHeader:y"})"})bad(Json::parse(s));
 ok(request(Json::parse(R"({"op":"container","wire_name":"a/b,savedgame","max_bytes":4096})"),"/fixed").path=="/fixed/a%2Fb,savedgame");
 bad(Json::parse(R"({"op":"container","wire_name":"unexpected,binary","max_bytes":1})"));
 ok(request(Json::parse(R"({"op":"container","wire_name":"a,b%/?#,savedgame","max_bytes":1})"),"/fixed").path=="/fixed/a%2Cb%25%2F%3F%23,savedgame");
 ok(request(Json::parse(R"({"op":"atom","atom":"12345678-1234-1234-1234-123456789ABC","max_bytes":1})"),"/fixed").path=="/fixed/12345678-1234-1234-1234-123456789ABC,binary");
 ok(request(Json::parse(R"({"op":"atom","atom":"12345678-1234-1234-1234-123456789abc","max_bytes":4096})"),"/fixed").path=="/fixed/12345678-1234-1234-1234-123456789abc,binary");
 for(auto s:{R"({"op":"lock","max_bytes":1})",R"({"op":"index","url":"https://evil.invalid","max_bytes":1})",R"({"op":"index","method":"PUT","max_bytes":1})",R"({"op":"index","max_bytes":-1})",R"({"op":"index","max_bytes":4194305})",R"({"op":"atom","atom":"../lock","max_bytes":1})",R"({"op":"container","wire_name":"..","max_bytes":1})",R"({"op":"container","wire_name":"x\nHeader:y","max_bytes":1})"})bad(Json::parse(s));
 try{initialize(Json::parse(R"({"op":"init","config":"<Game/>","scid":"x","pfn":"Test","title_id":123})"));assert(false);}catch(const Invalid&){++passed;}

 auto history=Json::parse(R"({"xuid":"123","titles":[{"titleId":"123","pfn":"Test.Game_abc","serviceConfigId":"12345678-1234-1234-1234-123456789ABC"}]})");
 ok(title_scid(history,123,"Test.Game_abc","123")=="12345678-1234-1234-1234-123456789abc");
 auto badhistory=[&](Json value){try{title_scid(value,123,"Test.Game_abc","123");assert(false);}catch(const Invalid&){++passed;}};
 auto other=history;other["xuid"]="124";badhistory(other);
 other=history;other["titles"].push_back(other["titles"][0]);badhistory(other);
 other=history;other["titles"][0]["pfn"]="Wrong.Title";badhistory(other);
 other=history;other["titles"][0]["titleId"]="124";badhistory(other);
 other=history;other["continuationToken"]="more";badhistory(other);
 other=history;other["pagingInfo"]={{"continuationToken","more"}};badhistory(other);
 other=history;other["titles"][0]["serviceConfigId"]="invalid";badhistory(other);
 std::cout<<passed<<" protocol checks PASS\n";
}

// SPDX-License-Identifier: LGPL-2.1-or-later
#include "StoreCatalog.h"
#include <vendor/nlohmann_json.hpp>
#include <cstdio>
#include <cstring>
using Json=nlohmann::json;
static int checks,failures;
static void check(const char *name,bool ok){++checks;failures+=!ok;std::printf("catalog case=%s pass=%d\n",name,ok);}
static Json fixture() {
    auto localized=[](bool sku){return Json{{"Language","en"},{"Markets",Json::array({"AT"})},
        {sku?"SkuTitle":"ProductTitle","Synthetic coins"},{sku?"SkuDescription":"ProductDescription","Synthetic description"}};};
    return Json{{"Product",{{"ProductId","ABCD1234EFGH"},{"ProductKind","UnmanagedConsumable"},
        {"LocalizedProperties",Json::array({localized(false)})},{"Properties",{{"InAppOfferToken","synthetic.offer"}}},
        {"DisplaySkuAvailabilities",Json::array({{{"Sku",{{"ProductId","ABCD1234EFGH"},{"SkuId","0001"},{"LocalizedProperties",Json::array({localized(true)})}}},
            {"Availabilities",Json::array({{{"AvailabilityId","SYNTHETIC-AVAILABILITY"},{"SkuId","0001"},
                {"Actions",Json::array({"Purchase","Redeem"})},{"Markets",Json::array({"AT"})},
                {"Conditions",{{"StartDate","2020-01-01T00:00:00Z"},{"EndDate","2040-01-01T00:00:00Z"},
                    {"ClientConditions",{{"AllowedPlatforms",Json::array({{{"PlatformName","Windows.Desktop"}}})}}}}},
                {"OrderManagementData",{{"Price",{{"ListPrice",10.25},{"MSRP",12.50},{"CurrencyCode","EUR"}}}}}
            }})}}})}
    }}};
}
// Public test entry point intentionally supports only embedded synthetic data.
int main(){
    using namespace xodus_catalog;
    Json f=fixture();std::string bytes=f.dump();Product p;
    check("valid-public-facts",parse(bytes,"ABCD1234EFGH",false,&p)==S_OK&&p.kind==16&&p.skus.size()==1&&p.localized[0].title=="Synthetic coins"&&p.raw_json.empty());
    const auto &a=p.skus[0].availabilities[0];
    check("prices-preserved-not-zeroed",a.has_price&&a.price.list_price==10.25&&a.price.base_price==12.50&&a.price.currency=="EUR");
    check("actions-markets-platforms",a.actions.size()==2&&a.markets[0]=="AT"&&a.platforms[0]=="Windows.Desktop");
    check("optional-raw-json-exact",parse(" \n"+bytes+"\n","ABCD1234EFGH",true,&p)==S_OK&&p.raw_json==" \n"+bytes+"\n");
    check("wrong-id-clears-result",FAILED(parse(bytes,"OTHER1234567",true,&p))&&p.id.empty()&&p.raw_json.empty());
    check("null-output",parse(bytes,"ABCD1234EFGH",false,nullptr)==E_POINTER);
    check("invalid-input-id",parse(bytes,"../invalid",false,&p)==E_INVALIDARG);
    check("empty-json",parse("","ABCD1234EFGH",false,&p)==E_INVALIDARG);
    check("oversized-document",parse(std::string(4*1024*1024+1,' '),"ABCD1234EFGH",false,&p)==E_INVALIDARG);
    auto duplicate=bytes;auto offset=duplicate.find("\"ProductId\":\"ABCD1234EFGH\"");duplicate.insert(offset,"\"ProductId\":\"OTHER1234567\",");
    check("duplicate-json-key",FAILED(parse(duplicate,"ABCD1234EFGH",false,&p)));
    auto mutate=[&](const char *name,Json input){check(name,FAILED(parse(input.dump(),"ABCD1234EFGH",false,&p))&&p.id.empty());};
    Json bad=f;bad["Product"]["ProductKind"]="Unknown";mutate("unknown-kind",bad);
    bad=f;bad["Product"]["LocalizedProperties"][0]["ProductTitle"]=std::string("bad\0name",8);mutate("embedded-nul",bad);
    bad=f;bad["Product"]["DisplaySkuAvailabilities"][0]["Sku"]["ProductId"]="OTHER1234567";mutate("foreign-sku",bad);
    bad=f;bad["Product"]["DisplaySkuAvailabilities"][0]["Sku"]["SkuId"]="bad/";mutate("invalid-sku-id",bad);
    bad=f;bad["Product"]["DisplaySkuAvailabilities"][0]["Availabilities"][0]["OrderManagementData"]["Price"]["ListPrice"]=-1;mutate("negative-price",bad);
    bad=f;bad["Product"]["DisplaySkuAvailabilities"][0]["Availabilities"][0]["OrderManagementData"]["Price"]["MSRP"]="12.50";mutate("wrong-price-type",bad);
    bad=f;bad["Product"]["DisplaySkuAvailabilities"][0]["Availabilities"][0]["OrderManagementData"]["Price"]["CurrencyCode"]="???";mutate("invalid-currency",bad);
    bad=f;bad["Product"]["DisplaySkuAvailabilities"][0]["Availabilities"][0]["OrderManagementData"]["Price"]=nullptr;
    check("missing-price-remains-unknown",parse(bad.dump(),"ABCD1234EFGH",false,&p)==S_OK&&!p.skus[0].availabilities[0].has_price);
    bad=f;bad["Product"]["DisplaySkuAvailabilities"][0]["Availabilities"]=Json::array();
    check("no-availabilities-is-catalog-fact",parse(bad.dump(),"ABCD1234EFGH",false,&p)==S_OK&&p.skus[0].availabilities.empty());
    volatile LONG cancel=1;
    check("cancel-no-network",fetch("ABCD1234EFGH","AT","en-US",false,&cancel,&p)==E_ABORT&&p.id.empty());
    check("host-path-injection-rejected",fetch("../escape","AT","en-US",false,nullptr,&p)==E_INVALIDARG);
    check("market-injection-rejected",fetch("ABCD1234EFGH","AT&token=x","en-US",false,nullptr,&p)==E_INVALIDARG);
    check("locale-injection-rejected",fetch("ABCD1234EFGH","AT","en-US&x=y",false,nullptr,&p)==E_INVALIDARG);
    std::printf("catalog checks=%d failures=%d external_requests=0\n",checks,failures);
    return failures?1:0;
}

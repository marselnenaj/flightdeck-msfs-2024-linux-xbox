// SPDX-License-Identifier: LGPL-2.1-or-later
#define main individual_catalog_probe_unused
#include "catalog-test.cpp"
#undef main
#include "StoreCatalogBatch.h"
#include <atomic>
#include <thread>
static std::atomic<int> fetch_calls{0},in_flight{0},maximum{0},fetch_mode{0};
static HRESULT synthetic_fetch(const std::string&id,const std::string&,const std::string&,
    bool raw,volatile LONG *cancel,xodus_catalog::Product*out,ULONGLONG deadline){
    ++fetch_calls;int n=++in_flight,old=maximum.load();while(n>old&&!maximum.compare_exchange_weak(old,n)){}
    struct Finished{~Finished(){--in_flight;}} finished;
    for(int i=0;i<10||fetch_mode==2;++i){
        if(InterlockedCompareExchange(cancel,0,0))return E_ABORT;
        if(GetTickCount64()>=deadline)return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
        if(fetch_mode==1&&id=="ABCD1234EFGH")return E_ACCESSDENIED;
        Sleep(2);
    }
    Json f=fixture();f["Product"]["ProductId"]=id;f["Product"]["DisplaySkuAvailabilities"][0]["Sku"]["ProductId"]=id;
    return xodus_catalog::parse(f.dump(),id,raw,out);
}
int main(){
    using namespace xodus_catalog;
    const std::vector<std::string> ids={"ABCD1234EFGH","OTHER1234567","ABCD1234EFGH","THIRD1234567","FOURT1234567","FIFTH1234567"};
    std::vector<Product> out;volatile LONG cancel=0;CatalogReader reader(synthetic_fetch);
    check("bounded-parallel-read",reader.read(ids,"AT","en-US",&cancel,&out)==S_OK&&out.size()==5&&fetch_calls==5&&maximum<=4&&maximum>1&&in_flight==0);
    check("dedupe-order",out[0].id==ids[0]&&out[1].id==ids[1]&&out[2].id==ids[3]&&out[4].id==ids[5]);
    check("raw-docs-private-cache-only",std::all_of(out.begin(),out.end(),[](const Product&p){return p.raw_json.empty();}));
    int before=fetch_calls;check("cache-reuses-public-documents",reader.read(ids,"AT","en-US",&cancel,&out)==S_OK&&fetch_calls==before&&out.size()==5);
    check("locale-separated",reader.read(ids,"AT","en-GB",&cancel,&out)==S_OK&&fetch_calls==before+5);
    before=fetch_calls;cancel=1;check("pre-cancel",reader.read(ids,"AT","en-US",&cancel,&out)==E_ABORT&&out.empty()&&fetch_calls==before);cancel=0;
    check("expired-deadline",reader.read(ids,"AT","en-US",&cancel,&out,GetTickCount64()-1)==HRESULT_FROM_WIN32(ERROR_TIMEOUT)&&out.empty()&&fetch_calls==before);
    check("no-path-injection",reader.read({"../escape"},"AT","en-US",&cancel,&out)==E_INVALIDARG&&out.empty()&&fetch_calls==before);
    check("no-empty-success",reader.read({},"AT","en-US",&cancel,&out)==E_INVALIDARG&&out.empty());
    check("max-100",reader.read(std::vector<std::string>(101,ids[0]),"AT","en-US",&cancel,&out)==E_INVALIDARG&&out.empty());
    check("null-output",reader.read(ids,"AT","en-US",&cancel,nullptr)==E_POINTER);
    fetch_mode=1;CatalogReader failed(synthetic_fetch);before=fetch_calls;
    check("failure-cancels-remaining-no-partial",failed.read(ids,"AT","en-US",&cancel,&out)==E_ACCESSDENIED&&out.empty()&&fetch_calls-before<=4&&in_flight==0);
    fetch_mode=2;CatalogReader timed(synthetic_fetch);auto start=GetTickCount64();
    check("shared-deadline",timed.read(ids,"AT","en-US",&cancel,&out,start+40)==HRESULT_FROM_WIN32(ERROR_TIMEOUT)&&out.empty()&&GetTickCount64()-start<1000&&in_flight==0);
    CatalogReader aborted(synthetic_fetch);std::thread stop([&](){Sleep(30);InterlockedExchange(&cancel,1);});
    auto hr=aborted.read(ids,"AT","en-US",&cancel,&out);stop.join();
    check("external-cancel-joins-workers",hr==E_ABORT&&out.empty()&&in_flight==0);cancel=0;
    std::printf("catalog-batch checks=%d failures=%d external_requests=0\n",checks,failures);return failures?1:0;
}

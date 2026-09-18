// SPDX-License-Identifier: MIT
// Synthetic Python -> native GameSave -> Python interchange test. No account.
#include "../../compat/runtime/src/GameSaveLocalCore.h"
#include <cstdio>
#include <cstring>

int wmain(int argc,wchar_t** argv) {
    if(argc!=2)return 2;
    local_save::Options options;
    options.enabled=true;
    options.root=argv[1];
    options.namespace_key=std::string(64,'a');
    local_save::Handle provider=nullptr,container=nullptr,update=nullptr;
    if(local_save::initialize(options,false,&provider)!=S_OK)return 3;
    if(local_save::create_container(provider,"profile",&container)!=S_OK)return 4;
    std::vector<std::string> names{"data"};
    std::vector<local_save::Blob> blobs;
    if(local_save::read_blobs(container,&names,&blobs)!=S_OK||blobs.size()!=1)return 5;
    const uint8_t expected[]={0x00,0xff,0x42};
    if(blobs[0].data.size()!=3||std::memcmp(blobs[0].data.data(),expected,3))return 6;
    if(local_save::create_update(container,"Native reply",&update)!=S_OK)return 7;
    const uint8_t reply[]={0x50,0x45,0x00,0xff};
    if(local_save::write_blob(update,"native_result",reply,sizeof(reply))!=S_OK)return 8;
    if(local_save::submit_update(update)!=S_OK)return 9;
    local_save::close_update(update);
    local_save::close_container(container);
    local_save::close_provider(provider);
    std::puts("SUMMARY Python fixture read; native transaction committed");
    return 0;
}

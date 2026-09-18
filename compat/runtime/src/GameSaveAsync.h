/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
#include "../abi/gamesave_abi.h"

HRESULT WINAPI XodusGameSaveInitializeProviderAsync(XUserHandle,const char*,bool,XAsyncBlock*);
HRESULT WINAPI XodusGameSaveInitializeProviderResult(XAsyncBlock*,XGameSaveProviderHandle*);
HRESULT WINAPI XodusGameSaveGetRemainingQuotaAsync(XGameSaveProviderHandle,XAsyncBlock*);
HRESULT WINAPI XodusGameSaveGetRemainingQuotaResult(XAsyncBlock*,int64_t*);
HRESULT WINAPI XodusGameSaveDeleteContainerAsync(XGameSaveProviderHandle,const char*,XAsyncBlock*);
HRESULT WINAPI XodusGameSaveDeleteContainerResult(XAsyncBlock*);
HRESULT WINAPI XodusGameSaveReadBlobDataAsync(XGameSaveContainerHandle,const char**,uint32_t,XAsyncBlock*);
HRESULT WINAPI XodusGameSaveReadBlobDataResult(XAsyncBlock*,size_t,XGameSaveBlob*,uint32_t*);
HRESULT WINAPI XodusGameSaveSubmitUpdateAsync(XGameSaveUpdateHandle,XAsyncBlock*);
HRESULT WINAPI XodusGameSaveSubmitUpdateResult(XAsyncBlock*);
void XodusGameSaveAsyncShutdown();

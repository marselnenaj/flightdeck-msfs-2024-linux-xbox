/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
#include "../abi/gamesave_abi.h"
#include "GameSaveLocalCore.h"

// Externally implemented COM ABI from the pinned xgame.idl. Keep the interface
// externally visible so whole-TU devirtualization cannot assume no implementer.
struct XodusGameSaveTitleRuntime : IUnknown {
    virtual HRESULT WINAPI XGameGetXboxTitleId(UINT32*)=0;
};

bool XodusGameSaveLocalEnabled();
bool IsGameSaveRuntimeClass(const GUID*);
HRESULT QueryGameSaveRuntime(const GUID*,REFIID,void**);
void ShutdownGameSaveRuntime();

HRESULT WINAPI XodusGameSaveInitializeProvider(XUserHandle,const char*,bool,XGameSaveProviderHandle*);
HRESULT XodusGameSaveDuplicateUser(XUserHandle,XUserHandle*);
void XodusGameSaveCloseUser(XUserHandle);
HRESULT XodusGameSaveCopyBlobNames(const char* const*,uint32_t,std::vector<std::string>*);
HRESULT XodusGameSavePackedBlobSize(const std::vector<local_save::Blob>&,SIZE_T*);
HRESULT XodusGameSavePackBlobs(const std::vector<local_save::Blob>&,SIZE_T,XGameSaveBlob*,uint32_t*);

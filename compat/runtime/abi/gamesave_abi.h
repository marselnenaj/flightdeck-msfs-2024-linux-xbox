/* SPDX-License-Identifier: LGPL-2.1-or-later */
// x64 private GameSave ABI: exact original-DLL slot order + public Microsoft declarations.
// See ABI-NOTES.md and public-signatures.json; no GameSave implementation or cloud claims.
#pragma once
#include <windows.h>
#include <unknwn.h>
#include <cstdint>
#include <cstddef>
#include <ctime>
#include <xuser.h>
#include <xasync.h>

struct XGameSaveProvider;
struct XGameSaveContainer;
struct XGameSaveUpdate;
using XGameSaveProviderHandle = XGameSaveProvider*;
using XGameSaveContainerHandle = XGameSaveContainer*;
using XGameSaveUpdateHandle = XGameSaveUpdate*;
struct XGameSaveBlobInfo { const char* name; uint32_t size; };
struct XGameSaveBlob { XGameSaveBlobInfo info; uint8_t* data; };
struct XGameSaveContainerInfo {
    const char* name;
    const char* displayName;
    uint32_t blobCount;
    uint64_t totalSize;
    time_t lastModifiedTime;
    bool needsSync;
};
typedef bool (CALLBACK XGameSaveBlobInfoCallback)(const XGameSaveBlobInfo*, void*);
typedef bool (CALLBACK XGameSaveContainerInfoCallback)(const XGameSaveContainerInfo*, void*);

inline constexpr GUID CLSID_XGameSaveImpl = {0x704c3f58,0xe629,0x4cc2,{0xb1,0x97,0x30,0x51,0x1b,0x99,0x6f,0xe2}};
inline constexpr GUID IID_IXGameSaveImpl = CLSID_XGameSaveImpl;
inline constexpr GUID IID_IXGameSaveImpl2 = {0x704c3f58,0xe629,0x4cc2,{0xb1,0x97,0x30,0x51,0x1b,0x99,0x6e,0xe2}};
inline constexpr GUID IID_IXGameSaveImpl3 = {0x1bfff3af,0xf14a,0x40a3,{0x8e,0x35,0x9a,0xda,0x90,0x65,0x93,0xf9}};
struct IXGameSaveImpl;
struct IXGameSaveImpl2;
// All three original IIDs return this same 33-slot pointer. Earlier revision
// method-count boundaries are intentionally not invented; only III is complete.
MIDL_INTERFACE("1bfff3af-f14a-40a3-8e35-9ada906593f9") IXGameSaveImpl3 : public IUnknown {
    // Slot 3, offset 0x18.
    virtual HRESULT WINAPI XGameSaveInitializeProvider(XUserHandle requestingUser, const char* configurationId, bool syncOnDemand, XGameSaveProviderHandle* provider) = 0;
    // Slot 4, offset 0x20.
    virtual HRESULT WINAPI XGameSaveInitializeProviderAsync(XUserHandle requestingUser, const char* configurationId, bool syncOnDemand, XAsyncBlock* async) = 0;
    // Slot 5, offset 0x28.
    virtual HRESULT WINAPI XGameSaveInitializeProviderResult(XAsyncBlock* async, XGameSaveProviderHandle* provider) = 0;
    // Slot 6, offset 0x30.
    virtual void WINAPI XGameSaveCloseProvider(XGameSaveProviderHandle provider) = 0;
    // Slot 7, offset 0x38.
    virtual HRESULT WINAPI XGameSaveGetRemainingQuota(XGameSaveProviderHandle provider, int64_t* remainingQuota) = 0;
    // Slot 8, offset 0x40.
    virtual HRESULT WINAPI XGameSaveGetRemainingQuotaAsync(XGameSaveProviderHandle provider, XAsyncBlock* async) = 0;
    // Slot 9, offset 0x48.
    virtual HRESULT WINAPI XGameSaveGetRemainingQuotaResult(XAsyncBlock* async, int64_t* remainingQuota) = 0;
    // Slot 10, offset 0x50.
    virtual HRESULT WINAPI XGameSaveDeleteContainer(XGameSaveProviderHandle provider, const char* containerName) = 0;
    // Slot 11, offset 0x58.
    virtual HRESULT WINAPI XGameSaveDeleteContainerAsync(XGameSaveProviderHandle provider, const char* containerName, XAsyncBlock* async) = 0;
    // Slot 12, offset 0x60.
    virtual HRESULT WINAPI XGameSaveDeleteContainerResult(XAsyncBlock* async) = 0;
    // Slot 13, offset 0x68.
    virtual HRESULT WINAPI XGameSaveGetContainerInfo(XGameSaveProviderHandle provider, const char* containerName, void* context, XGameSaveContainerInfoCallback* callback) = 0;
    // Slot 14, offset 0x70.
    virtual HRESULT WINAPI XGameSaveEnumerateContainerInfo(XGameSaveProviderHandle provider, void* context, XGameSaveContainerInfoCallback* callback) = 0;
    // Slot 15, offset 0x78.
    virtual HRESULT WINAPI XGameSaveEnumerateContainerInfoByName(XGameSaveProviderHandle provider, const char* containerNamePrefix, void* context, XGameSaveContainerInfoCallback* callback) = 0;
    // Slot 16, offset 0x80.
    virtual HRESULT WINAPI XGameSaveCreateContainer(XGameSaveProviderHandle provider, const char* containerName, XGameSaveContainerHandle* containerContext) = 0;
    // Slot 17, offset 0x88.
    virtual void WINAPI XGameSaveCloseContainer(XGameSaveContainerHandle context) = 0;
    // Slot 18, offset 0x90.
    virtual HRESULT WINAPI XGameSaveEnumerateBlobInfo(XGameSaveContainerHandle container, void* context, XGameSaveBlobInfoCallback* callback) = 0;
    // Slot 19, offset 0x98.
    virtual HRESULT WINAPI XGameSaveEnumerateBlobInfoByName(XGameSaveContainerHandle container, const char* blobNamePrefix, void* context, XGameSaveBlobInfoCallback* callback) = 0;
    // Slot 20, offset 0xa0.
    virtual HRESULT WINAPI XGameSaveReadBlobData(XGameSaveContainerHandle container, const char** blobNames, uint32_t* countOfBlobs, size_t blobsSize, XGameSaveBlob* blobData) = 0;
    // Slot 21, offset 0xa8.
    virtual HRESULT WINAPI XGameSaveReadBlobDataAsync(XGameSaveContainerHandle container, const char** blobNames, uint32_t countOfBlobs, XAsyncBlock* async) = 0;
    // Slot 22, offset 0xb0.
    virtual HRESULT WINAPI XGameSaveReadBlobDataResult(XAsyncBlock* async, size_t blobsSize, XGameSaveBlob* blobData, uint32_t* countOfBlobs) = 0;
    // Slot 23, offset 0xb8.
    virtual HRESULT WINAPI XGameSaveCreateUpdate(XGameSaveContainerHandle container, const char* containerDisplayName, XGameSaveUpdateHandle* updateContext) = 0;
    // Slot 24, offset 0xc0.
    virtual void WINAPI XGameSaveCloseUpdate(XGameSaveUpdateHandle context) = 0;
    // Slot 25, offset 0xc8.
    virtual HRESULT WINAPI XGameSaveSubmitBlobWrite(XGameSaveUpdateHandle updateContext, const char* blobName, const uint8_t* data, size_t byteCount) = 0;
    // Slot 26, offset 0xd0.
    virtual HRESULT WINAPI XGameSaveSubmitBlobDelete(XGameSaveUpdateHandle updateContext, const char* blobName) = 0;
    // Slot 27, offset 0xd8.
    virtual HRESULT WINAPI XGameSaveSubmitUpdate(XGameSaveUpdateHandle updateContext) = 0;
    // Slot 28, offset 0xe0.
    virtual HRESULT WINAPI XGameSaveSubmitUpdateAsync(XGameSaveUpdateHandle updateContext, XAsyncBlock* async) = 0;
    // Slot 29, offset 0xe8.
    virtual HRESULT WINAPI XGameSaveSubmitUpdateResult(XAsyncBlock* async) = 0;
    // Slot 30, offset 0xf0.
    virtual HRESULT WINAPI XGameSaveFilesGetFolderWithUiAsync(XUserHandle requestingUser, const char * configurationId, XAsyncBlock * async) = 0;
    // Slot 31, offset 0xf8.
    virtual HRESULT WINAPI XGameSaveFilesGetFolderWithUiResult(XAsyncBlock * async, size_t folderSize, char * folderResult) = 0;
    // Slot 32, offset 0x100.
    virtual HRESULT WINAPI XGameSaveFilesGetRemainingQuota(XUserHandle userContext, const char * configurationId, int64_t * remainingQuota) = 0;
};
#ifdef __CRT_UUID_DECL
__CRT_UUID_DECL(IXGameSaveImpl,0x704c3f58,0xe629,0x4cc2,0xb1,0x97,0x30,0x51,0x1b,0x99,0x6f,0xe2)
__CRT_UUID_DECL(IXGameSaveImpl2,0x704c3f58,0xe629,0x4cc2,0xb1,0x97,0x30,0x51,0x1b,0x99,0x6e,0xe2)
__CRT_UUID_DECL(IXGameSaveImpl3,0x1bfff3af,0xf14a,0x40a3,0x8e,0x35,0x9a,0xda,0x90,0x65,0x93,0xf9)
#endif

// C-style view for verified slot offsets and bridge/probe dispatch.
struct XodusGameSaveVTable {
    HRESULT (WINAPI *QueryInterface)(IXGameSaveImpl3*,REFIID,void**);
    ULONG (WINAPI *AddRef)(IXGameSaveImpl3*);
    ULONG (WINAPI *Release)(IXGameSaveImpl3*);
    HRESULT (WINAPI *XGameSaveInitializeProvider)(IXGameSaveImpl3*, XUserHandle requestingUser, const char* configurationId, bool syncOnDemand, XGameSaveProviderHandle* provider);
    HRESULT (WINAPI *XGameSaveInitializeProviderAsync)(IXGameSaveImpl3*, XUserHandle requestingUser, const char* configurationId, bool syncOnDemand, XAsyncBlock* async);
    HRESULT (WINAPI *XGameSaveInitializeProviderResult)(IXGameSaveImpl3*, XAsyncBlock* async, XGameSaveProviderHandle* provider);
    void (WINAPI *XGameSaveCloseProvider)(IXGameSaveImpl3*, XGameSaveProviderHandle provider);
    HRESULT (WINAPI *XGameSaveGetRemainingQuota)(IXGameSaveImpl3*, XGameSaveProviderHandle provider, int64_t* remainingQuota);
    HRESULT (WINAPI *XGameSaveGetRemainingQuotaAsync)(IXGameSaveImpl3*, XGameSaveProviderHandle provider, XAsyncBlock* async);
    HRESULT (WINAPI *XGameSaveGetRemainingQuotaResult)(IXGameSaveImpl3*, XAsyncBlock* async, int64_t* remainingQuota);
    HRESULT (WINAPI *XGameSaveDeleteContainer)(IXGameSaveImpl3*, XGameSaveProviderHandle provider, const char* containerName);
    HRESULT (WINAPI *XGameSaveDeleteContainerAsync)(IXGameSaveImpl3*, XGameSaveProviderHandle provider, const char* containerName, XAsyncBlock* async);
    HRESULT (WINAPI *XGameSaveDeleteContainerResult)(IXGameSaveImpl3*, XAsyncBlock* async);
    HRESULT (WINAPI *XGameSaveGetContainerInfo)(IXGameSaveImpl3*, XGameSaveProviderHandle provider, const char* containerName, void* context, XGameSaveContainerInfoCallback* callback);
    HRESULT (WINAPI *XGameSaveEnumerateContainerInfo)(IXGameSaveImpl3*, XGameSaveProviderHandle provider, void* context, XGameSaveContainerInfoCallback* callback);
    HRESULT (WINAPI *XGameSaveEnumerateContainerInfoByName)(IXGameSaveImpl3*, XGameSaveProviderHandle provider, const char* containerNamePrefix, void* context, XGameSaveContainerInfoCallback* callback);
    HRESULT (WINAPI *XGameSaveCreateContainer)(IXGameSaveImpl3*, XGameSaveProviderHandle provider, const char* containerName, XGameSaveContainerHandle* containerContext);
    void (WINAPI *XGameSaveCloseContainer)(IXGameSaveImpl3*, XGameSaveContainerHandle context);
    HRESULT (WINAPI *XGameSaveEnumerateBlobInfo)(IXGameSaveImpl3*, XGameSaveContainerHandle container, void* context, XGameSaveBlobInfoCallback* callback);
    HRESULT (WINAPI *XGameSaveEnumerateBlobInfoByName)(IXGameSaveImpl3*, XGameSaveContainerHandle container, const char* blobNamePrefix, void* context, XGameSaveBlobInfoCallback* callback);
    HRESULT (WINAPI *XGameSaveReadBlobData)(IXGameSaveImpl3*, XGameSaveContainerHandle container, const char** blobNames, uint32_t* countOfBlobs, size_t blobsSize, XGameSaveBlob* blobData);
    HRESULT (WINAPI *XGameSaveReadBlobDataAsync)(IXGameSaveImpl3*, XGameSaveContainerHandle container, const char** blobNames, uint32_t countOfBlobs, XAsyncBlock* async);
    HRESULT (WINAPI *XGameSaveReadBlobDataResult)(IXGameSaveImpl3*, XAsyncBlock* async, size_t blobsSize, XGameSaveBlob* blobData, uint32_t* countOfBlobs);
    HRESULT (WINAPI *XGameSaveCreateUpdate)(IXGameSaveImpl3*, XGameSaveContainerHandle container, const char* containerDisplayName, XGameSaveUpdateHandle* updateContext);
    void (WINAPI *XGameSaveCloseUpdate)(IXGameSaveImpl3*, XGameSaveUpdateHandle context);
    HRESULT (WINAPI *XGameSaveSubmitBlobWrite)(IXGameSaveImpl3*, XGameSaveUpdateHandle updateContext, const char* blobName, const uint8_t* data, size_t byteCount);
    HRESULT (WINAPI *XGameSaveSubmitBlobDelete)(IXGameSaveImpl3*, XGameSaveUpdateHandle updateContext, const char* blobName);
    HRESULT (WINAPI *XGameSaveSubmitUpdate)(IXGameSaveImpl3*, XGameSaveUpdateHandle updateContext);
    HRESULT (WINAPI *XGameSaveSubmitUpdateAsync)(IXGameSaveImpl3*, XGameSaveUpdateHandle updateContext, XAsyncBlock* async);
    HRESULT (WINAPI *XGameSaveSubmitUpdateResult)(IXGameSaveImpl3*, XAsyncBlock* async);
    HRESULT (WINAPI *XGameSaveFilesGetFolderWithUiAsync)(IXGameSaveImpl3*, XUserHandle requestingUser, const char * configurationId, XAsyncBlock * async);
    HRESULT (WINAPI *XGameSaveFilesGetFolderWithUiResult)(IXGameSaveImpl3*, XAsyncBlock * async, size_t folderSize, char * folderResult);
    HRESULT (WINAPI *XGameSaveFilesGetRemainingQuota)(IXGameSaveImpl3*, XUserHandle userContext, const char * configurationId, int64_t * remainingQuota);
};
static_assert(sizeof(void*)==8 && sizeof(size_t)==8 && sizeof(time_t)==8,"x64 Windows ABI only");
static_assert(sizeof(bool)==1 && sizeof(BOOLEAN)==1,"byte boolean ABI");
static_assert(sizeof(XGameSaveBlobInfo)==16 && alignof(XGameSaveBlobInfo)==8);
static_assert(offsetof(XGameSaveBlobInfo,name)==0 && offsetof(XGameSaveBlobInfo,size)==8);
static_assert(sizeof(XGameSaveBlob)==24 && offsetof(XGameSaveBlob,data)==16);
static_assert(sizeof(XGameSaveContainerInfo)==48 && alignof(XGameSaveContainerInfo)==8);
static_assert(offsetof(XGameSaveContainerInfo,name)==0 && offsetof(XGameSaveContainerInfo,displayName)==8);
static_assert(offsetof(XGameSaveContainerInfo,blobCount)==16 && offsetof(XGameSaveContainerInfo,totalSize)==24);
static_assert(offsetof(XGameSaveContainerInfo,lastModifiedTime)==32 && offsetof(XGameSaveContainerInfo,needsSync)==40);
static_assert(sizeof(XodusGameSaveVTable)==33*8);
static_assert(offsetof(XodusGameSaveVTable,QueryInterface)==0*8);
static_assert(offsetof(XodusGameSaveVTable,AddRef)==1*8);
static_assert(offsetof(XodusGameSaveVTable,Release)==2*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveInitializeProvider)==3*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveInitializeProviderAsync)==4*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveInitializeProviderResult)==5*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveCloseProvider)==6*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveGetRemainingQuota)==7*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveGetRemainingQuotaAsync)==8*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveGetRemainingQuotaResult)==9*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveDeleteContainer)==10*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveDeleteContainerAsync)==11*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveDeleteContainerResult)==12*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveGetContainerInfo)==13*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveEnumerateContainerInfo)==14*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveEnumerateContainerInfoByName)==15*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveCreateContainer)==16*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveCloseContainer)==17*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveEnumerateBlobInfo)==18*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveEnumerateBlobInfoByName)==19*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveReadBlobData)==20*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveReadBlobDataAsync)==21*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveReadBlobDataResult)==22*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveCreateUpdate)==23*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveCloseUpdate)==24*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveSubmitBlobWrite)==25*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveSubmitBlobDelete)==26*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveSubmitUpdate)==27*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveSubmitUpdateAsync)==28*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveSubmitUpdateResult)==29*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveFilesGetFolderWithUiAsync)==30*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveFilesGetFolderWithUiResult)==31*8);
static_assert(offsetof(XodusGameSaveVTable,XGameSaveFilesGetRemainingQuota)==32*8);

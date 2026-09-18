/* SPDX-License-Identifier: LGPL-2.1-or-later */
// Narrow diagnostics around the unchanged original runtime. LGPL-2.1-or-later.
#include "RuntimeDiagnostics.h"
#include "NetworkingState.h"
#include <atomic>
#include <mutex>

class FeatureDiagnostics final : public IXGameRuntimeFeatureImpl {
    std::atomic<ULONG> refs{1};
    std::once_flag initialized;
    IXGameRuntimeFeatureImpl *original=nullptr;
    HRESULT initialization=E_UNEXPECTED;
    std::atomic<unsigned> calls{0};
public:
    HRESULT initialize(){
        std::call_once(initialized,[&]{const GUID id=__uuidof(IXGameRuntimeFeatureImpl);initialization=QueryOriginalApi(&id,id,reinterpret_cast<void**>(&original));});
        return initialization;
    }
    HRESULT WINAPI QueryInterface(REFIID iid,void **out) override {
        if(!out)return E_POINTER;*out=nullptr;
        if(iid!=__uuidof(IUnknown)&&iid!=__uuidof(IXGameRuntimeFeatureImpl))return E_NOINTERFACE;
        *out=static_cast<IXGameRuntimeFeatureImpl*>(this);AddRef();return S_OK;
    }
    ULONG WINAPI AddRef() override{return ++refs;}
    ULONG WINAPI Release() override{return --refs;}
    BOOLEAN WINAPI XGameRuntimeIsFeatureAvailable(XGameRuntimeFeature feature) override {
        const BOOLEAN available=original->XGameRuntimeIsFeatureAvailable(feature);
        if(calls.fetch_add(1)<64)std::fprintf(stderr,"xodus-runtime: FeatureAvailable feature=%u original=%u\n",static_cast<unsigned>(feature),static_cast<unsigned>(available));
        return available;
    }
};

class SystemDiagnostics final : public IXSystemImpl5 {
    std::atomic<ULONG> refs{1};
    std::once_flag initialized;
    IXSystemImpl5 *original=nullptr;
    HRESULT initialization=E_UNEXPECTED;
    std::atomic<unsigned> calls[6]{};
    HRESULT log(unsigned slot,const char *name,HRESULT result){
        if(calls[slot].fetch_add(1)<8)std::fprintf(stderr,"xodus-runtime: System.%s result=%08lx\n",name,static_cast<ULONG>(result));
        return result;
    }
public:
    HRESULT initialize(){
        std::call_once(initialized,[&]{const GUID id=__uuidof(IXSystemImpl);initialization=QueryOriginalApi(&id,__uuidof(IXSystemImpl5),reinterpret_cast<void**>(&original));});
        return initialization;
    }
    HRESULT WINAPI QueryInterface(REFIID iid,void **out) override {
        if(!out)return E_POINTER;*out=nullptr;
        if(iid!=__uuidof(IUnknown)&&iid!=__uuidof(IXSystemImpl)&&iid!=__uuidof(IXSystemImpl2)&&
           iid!=__uuidof(IXSystemImpl3)&&iid!=__uuidof(IXSystemImpl4)&&iid!=__uuidof(IXSystemImpl5))return E_NOINTERFACE;
        *out=static_cast<IXSystemImpl5*>(this);AddRef();return S_OK;
    }
    ULONG WINAPI AddRef() override{return ++refs;}
    ULONG WINAPI Release() override{return --refs;}
    HRESULT WINAPI XSystemGetConsoleId(INT32 size,char *buffer,SIZE_T *used) override{return log(0,"GetConsoleId",original->XSystemGetConsoleId(size,buffer,used));}
    HRESULT WINAPI XSystemGetXboxLiveSandboxId(INT32 size,char *buffer,SIZE_T *used) override{
        // The shipped GDK thunk preserves public (size, buffer, optional used)
        // ordering, and XSAPI passes used=nullptr with a real 16-byte buffer.
        // The old Wine implementation incorrectly requires the optional count.
        // Preserve the caller's actual output buffer and all original failures.
        SIZE_T ignored_used=0;
        return log(1,"GetXboxLiveSandboxId",original->XSystemGetXboxLiveSandboxId(size,buffer,used?used:&ignored_used));
    }
    HRESULT WINAPI XSystemGetAppSpecificDeviceId(INT32 size,char *buffer,SIZE_T *used) override{return log(2,"GetAppSpecificDeviceId",original->XSystemGetAppSpecificDeviceId(size,buffer,used));}
    HRESULT WINAPI XSystemHandleTrack(XSystemHandleCallback callback,void *context) override{return log(3,"HandleTrack",original->XSystemHandleTrack(callback,context));}
    BOOLEAN WINAPI XSystemIsHandleValid(XSystemHandle handle) override{
        const BOOLEAN valid=original->XSystemIsHandleValid(handle);
        if(calls[4].fetch_add(1)<8)std::fprintf(stderr,"xodus-runtime: System.IsHandleValid original=%u\n",static_cast<unsigned>(valid));
        return valid;
    }
    void WINAPI XSystemAllowFullDownloadBandwidth(BOOLEAN enable) override{
        original->XSystemAllowFullDownloadBandwidth(enable);
        if(calls[5].fetch_add(1)<8)std::fprintf(stderr,"xodus-runtime: System.AllowFullDownloadBandwidth returned\n");
    }
};
static FeatureDiagnostics features;
static SystemDiagnostics system_diagnostics;
bool IsRuntimeDiagnosticsClass(const GUID *clsid){return *clsid==__uuidof(IXGameRuntimeFeatureImpl)||*clsid==__uuidof(IXSystemImpl);}
HRESULT QueryRuntimeDiagnostics(const GUID *clsid,REFIID iid,void **out){
    if(!clsid||!out)return E_POINTER;*out=nullptr;
    if(*clsid==__uuidof(IXGameRuntimeFeatureImpl)){HRESULT hr=features.initialize();return FAILED(hr)?hr:features.QueryInterface(iid,out);}
    if(*clsid==__uuidof(IXSystemImpl)){HRESULT hr=system_diagnostics.initialize();return FAILED(hr)?hr:system_diagnostics.QueryInterface(iid,out);}
    return E_NOINTERFACE;
}

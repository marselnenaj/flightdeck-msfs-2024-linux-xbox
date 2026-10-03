/* SPDX-License-Identifier: MIT */
/* Own synthetic PE: no game, account, authentication or entitlement operation. */
#define COBJMACROS
#include <windows.h>
#include <unknwn.h>
#include <stdio.h>
#include <string.h>

typedef HRESULT (WINAPI *Query)(const GUID *, const GUID *, void **);
static const GUID threading_iid = {0x073b7dcb,0x1fcf,0x4030,{0x94,0xbe,0xe3,0xc9,0xeb,0x62,0x34,0x28}};

int main(int argc, char **argv)
{
    WCHAR path[32768], *name;
    HMODULE proxy, original, builtin, helper;
    IUnknown *threading = NULL;
    Query query;
    int (*marker)(void);
    HRESULT hr;
    if (argc != 2 || strcmp(argv[1], "-FastLaunch")) {
        puts("MSFS FastLaunch argument did not reach the Windows process"); return 8;
    }
    /* UI engines and delayed imports also resolve from the working directory.
     * The previous probe covered only the executable's temporary directory. */
    if (!GetCurrentDirectoryW(32768, path) || wcslen(path) > 32000) return 6;
    wcscat(path, L"\\nested\\probe.dll");
    helper = LoadLibraryW(path);
    if (!helper || !(marker = (void *)GetProcAddress(helper, "probe")) || marker() != 42) {
        printf("Working-directory mapped DLL failed: %lu\n", GetLastError()); return 7;
    }
    FreeLibrary(helper);
    if (!GetModuleFileNameW(NULL, path, 32768) || !(name = wcsrchr(path, '\\'))) return 1;
    wcscpy(name + 1, L"nested\\probe.dll");
    helper = LoadLibraryW(path);
    if (!helper || !(marker = (void *)GetProcAddress(helper, "probe")) || marker() != 42) {
        printf("Mapped DLL failed: %lu\n", GetLastError()); return 2;
    }
    proxy = LoadLibraryW(L"xgameruntime.dll");
    original = LoadLibraryW(L"xgameruntime_original.dll");
    builtin = LoadLibraryW(L"xodus_store_test.dll");
    if (!proxy || !original || !builtin) { printf("Store DLL load failed: %lu\n", GetLastError()); return 3; }
    query = (void *)GetProcAddress(proxy, "QueryApiImpl");
    if (!query) return 4;
    hr = query(&threading_iid, &threading_iid, (void **)&threading);
    if (FAILED(hr) || !threading) { printf("Threading interface failed: %#lx\n", (unsigned long)hr); return 5; }
    IUnknown_Release(threading);
    puts("PASS: FastLaunch argument, memfd executable, working-directory and module-directory DLLs, Store libraries and threading interface");
    return 0;
}

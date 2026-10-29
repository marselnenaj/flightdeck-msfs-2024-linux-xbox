// SPDX-License-Identifier: MIT
// Account-free Wine loader check: load an existing IL-only runtime assembly.
#include <windows.h>
#include <stdio.h>

int wmain(int argc, wchar_t **argv)
{
    if (argc != 2) return 2;
    SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
    HMODULE module = LoadLibraryW(argv[1]);
    DWORD error = module ? ERROR_SUCCESS : GetLastError();
    fprintf(stderr, "flightdeck-dotnet-probe: loaded=%u error=%lu\n",
        module != NULL, (unsigned long)error);
    if (module) FreeLibrary(module);
    return module ? 0 : 1;
}

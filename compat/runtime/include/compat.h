/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
#define __WINESRC__ 1
#include <windows.h>
#include <unknwn.h>
#include <inspectable.h>
#include <assert.h>
#include <stdint.h>
#include <cstdio>
#include <cstddef>
#include <algorithm>
#include <condition_variable>
using std::nullptr_t;
#ifndef E_ILLEGAL_METHOD_CALL
#define E_ILLEGAL_METHOD_CALL ((HRESULT)0x8000000E)
#endif
#include <cstring>
#include <xgameerr.h>
#include <xasync.h>
#include <xasyncprovider.h>
#define WINE_DEFAULT_DEBUG_CHANNEL(x)
#define TRACE(...) do {} while (0)
#define WARN(...) std::fprintf(stderr, __VA_ARGS__)
#define FIXME(...) std::fprintf(stderr, __VA_ARGS__)
inline const char *debugstr_guid(const GUID *) { return "GUID"; }
#define RETURN_HR(hr)                                           TRACE("Returning HR %#lx\n", hr); return(hr)
#define RETURN_LAST_ERROR()                                     return HRESULT_FROM_WIN32(GetLastError())
#define RETURN_WIN32(win32err)                                  return HRESULT_FROM_WIN32(win32err)

#define RETURN_IF_FAILED(hr)                                    do { HRESULT __hrRet = hr; if (FAILED(__hrRet)) { RETURN_HR(__hrRet); }} while (0)
#define RETURN_IF_WIN32_BOOL_FALSE(win32BOOL)                   do { BOOL __boolRet = win32BOOL; if (!__boolRet) { RETURN_LAST_ERROR(); }} while (0)
#define RETURN_IF_NULL_ALLOC(ptr)                               do { if ((ptr) == nullptr) { RETURN_HR(E_OUTOFMEMORY); }} while (0)
#define RETURN_HR_IF(hr, condition)                             do { if (condition) { RETURN_HR(hr); }} while (0)
#define RETURN_HR_IF_FALSE(hr, condition)                       do { if (!(condition)) { RETURN_HR(hr); }} while (0)
#define RETURN_LAST_ERROR_IF(condition)                         do { if (condition) { RETURN_LAST_ERROR(); }} while (0)
#define RETURN_LAST_ERROR_IF_NULL(ptr)                          do { if ((ptr) == nullptr) { RETURN_LAST_ERROR(); }} while (0)

#define LOG_IF_FAILED(hr)                                       do { HRESULT __hrRet = hr; if (FAILED(__hrRet)) { TRACE("libHttpClient error %s: 0x%#lx", #hr, __hrRet); }} while (0)

#define FAIL_FAST_MSG(fmt, ...)                        \
    TRACE(fmt, ##__VA_ARGS__);                         \
    assert(false);                                     \

#define FAIL_FAST_IF_FAILED(hr)                                 do { HRESULT __hrRet = hr; if (FAILED(__hrRet)) { FAIL_FAST_MSG("%s 0x%#lx", #hr, __hrRet); }} while (0)


extern IXThreadingImpl *x_threading_impl;

/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
#include "NetworkingState.h"
#include <wincrypt.h>

HRESULT NetworkSecurityQuery(LPCWSTR url, XAsyncBlock *async);
HRESULT NetworkSecurityQueryUtf8(LPCSTR url, XAsyncBlock *async);
HRESULT NetworkSecurityResultSize(XAsyncBlock *async, SIZE_T *size);
HRESULT NetworkSecurityResult(XAsyncBlock *async, SIZE_T size, SIZE_T *used, UINT8 *buffer, XNetworkingSecurityInformation **info);
HRESULT NetworkSecurityVerify(void *request, const XNetworkingSecurityInformation *info);
void NetworkSecurityShutdown();
// Caller supplies only a successfully authenticated title NSAL document.
// The input is copied synchronously; no caller-owned pointer survives return.
extern "C" HRESULT WINAPI XodusPublishTitleEndpointPolicy(UINT32 titleId, const char *json, SIZE_T length);
#ifdef NETWORK_SECURITY_TESTING
HRESULT NetworkSecurityTestPolicy(const char *document, LPCWSTR url, XNetworkingSecurityInformation *result);
HRESULT NetworkSecurityTestCertificate(PCCERT_CONTEXT certificate, LPCWSTR host, const FILETIME *time=nullptr);
HRESULT NetworkSecurityTestCurrentPolicy(LPCWSTR url, XNetworkingSecurityInformation *result);
HRESULT NetworkSecurityTestReset(const char *defaults);
void NetworkSecurityTestSetPublishBarrier(void(*barrier)());
#endif

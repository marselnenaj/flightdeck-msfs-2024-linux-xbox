/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
#include "compat.h"
#include <xstore.h>

HRESULT XodusStoreQueryGameLicenseAsync(XStoreContextHandle, XAsyncBlock *);
HRESULT XodusStoreQueryGameLicenseResult(XAsyncBlock *, XStoreGameLicense *);
HRESULT XodusStoreQueryLicenseTokenAsync(XStoreContextHandle, const char **, SIZE_T, const char *, XAsyncBlock *);
HRESULT XodusStoreQueryLicenseTokenResultSize(XAsyncBlock *, SIZE_T *);
HRESULT XodusStoreQueryLicenseTokenResult(XAsyncBlock *, SIZE_T, char *);
HRESULT XodusStoreQueryEntitledProductsAsync(XStoreContextHandle, XStoreProductKind, UINT32, XAsyncBlock *);
HRESULT XodusStoreQueryEntitledProductsResult(XAsyncBlock *, XStoreProductQueryHandle *);
HRESULT XodusStoreQueryProductsAsync(XStoreContextHandle, XStoreProductKind,
    const char **, SIZE_T, const char **, SIZE_T, XAsyncBlock *);
HRESULT XodusStoreQueryProductsResult(XAsyncBlock *, XStoreProductQueryHandle *);
HRESULT XodusStoreEnumerateProductsQuery(XStoreProductQueryHandle, void *, XStoreProductQueryCallback *);
BOOLEAN XodusStoreProductsQueryHasMorePages(XStoreProductQueryHandle);
HRESULT XodusStoreProductsQueryNextPageAsync(XStoreProductQueryHandle, XAsyncBlock *);
HRESULT XodusStoreProductsQueryNextPageResult(XAsyncBlock *, XStoreProductQueryHandle *);
void XodusStoreCloseProductsQueryHandle(XStoreProductQueryHandle);
void XodusStoreQueriesContextClosed(void *context);
void XodusStoreQueriesShutdown();

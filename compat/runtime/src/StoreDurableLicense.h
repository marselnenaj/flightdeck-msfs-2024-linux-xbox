/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
#include "compat.h"
#include "StoreContext.h"
#include <xstore.h>

HRESULT XodusStoreDurableAcquire(const XodusStoreContextRef &context, const char *store_id,
    volatile LONG *cancelled, XStoreLicenseHandle *out);
BOOLEAN XodusStoreIsLicenseValid(XStoreLicenseHandle handle);
void XodusStoreCloseLicenseHandle(XStoreLicenseHandle handle);
HRESULT XodusStoreRegisterPackageLicenseLost(XStoreLicenseHandle handle, XTaskQueueHandle queue,
    void *context, XStorePackageLicenseLostCallback *callback, XTaskQueueRegistrationToken *token);
BOOLEAN XodusStoreUnregisterPackageLicenseLost(XStoreLicenseHandle handle,
    XTaskQueueRegistrationToken token, BOOLEAN wait);
void XodusStoreDurableContextClosed(void *context);
void XodusStoreDurableShutdown();
#ifdef STORE_DURABLES_TESTING
void XodusStoreDurableTestAdvance(UINT64 milliseconds);
void XodusStoreDurableTestPoll();
SIZE_T XodusStoreDurableTestCount();
#endif

/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
#include "compat.h"
#include <xstore.h>

HRESULT XodusStoreRegisterGameLicenseChanged(void *store, XTaskQueueHandle queue,
    void *context, XStoreGameLicenseChangedCallback *callback,
    XTaskQueueRegistrationToken *token);
BOOLEAN XodusStoreUnregisterGameLicenseChanged(void *store,
    XTaskQueueRegistrationToken token, BOOLEAN wait);
void XodusStoreLicenseEventsContextClosed(void *store);
void XodusStoreLicenseEventsShutdown();

#ifdef STORE_EVENTS_TESTING
void XodusStoreLicenseEventsTestPoll();
void XodusStoreLicenseEventsTestArmTimer();
#endif

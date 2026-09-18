/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
#include "compat.h"
#include <xnetworking.h>
HRESULT QueryOriginalApi(const GUID *clsid, REFIID iid, void **out);
extern IXNetworkingImpl *x_networking_impl;
#ifdef NETWORKING_TESTING
using ConnectivityReader=HRESULT(*)(XNetworkingConnectivityHint*);
void NetworkTestSetReader(ConnectivityReader reader);
void NetworkTestPoll();
void NetworkTestArmTimer();
#endif

void NetworkRuntimeShutdown();

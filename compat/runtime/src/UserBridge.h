/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
#include "compat.h"

bool UserRuntimeEnabled();
bool IsUserRuntimeClass(const GUID *clsid);
HRESULT InitializeUserRuntime(ULONG gdk, ULONG services, char mode, const void *options);
HRESULT QueryUserRuntime(const GUID *clsid, REFIID iid, void **out);
HRESULT ShutdownUserRuntime();

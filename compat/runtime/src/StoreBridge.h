/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
#include "compat.h"
bool IsStoreRuntimeClass(const GUID *clsid);
HRESULT QueryStoreRuntime(const GUID *clsid,REFIID iid,void **out);
void ShutdownStoreRuntime();

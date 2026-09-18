/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
#include "compat.h"
#include <xgameruntimefeature.h>
#include <xsystem.h>
bool IsRuntimeDiagnosticsClass(const GUID *clsid);
HRESULT QueryRuntimeDiagnostics(const GUID *clsid,REFIID iid,void **out);

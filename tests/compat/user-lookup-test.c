/* SPDX-License-Identifier: LGPL-2.1-or-later */
#define COBJMACROS
#include <windows.h>
#include <unknwn.h>
#include <stdio.h>
#include <xgameerr.h>
#include "UserSessionCache.h"

/* Synthetic user objects exercise the production lookup and cache. Only the
 * fields read by the lookup are needed; no authentication/Store provider exists
 * in this binary. Production compiles the same include against its real XUser. */
struct XUser {
    IUnknown IUser_iface;
    LONG refs;
    UINT64 xuid;
    BOOL xuid_syntax_valid;
    SRWLOCK xstsLock;
    char store_account_context[65];
    UINT64 store_account_xuid;
};
typedef struct XUser *XUserHandle;
#include "UserLookup.inc"
static char current_account[65];
static HRESULT account_status = S_OK;
static HRESULT xodus_store_current_account_context(char output[65])
{
    memcpy(output, current_account, 65);
    return account_status;
}
#include "UserStoreAccount.inc"

static int checks, failures;
static void check(const char *name, BOOL ok)
{
    ++checks;
    if (!ok) ++failures;
    printf("%s %s\n", ok ? "PASS" : "FAIL", name);
}
static HRESULT WINAPI query(IUnknown *iface, REFIID iid, void **out)
{
    if (out) *out = NULL;
    return E_NOINTERFACE;
}
static ULONG WINAPI retain(IUnknown *iface)
{
    return InterlockedIncrement(&CONTAINING_RECORD(iface, struct XUser, IUser_iface)->refs);
}
static ULONG WINAPI release(IUnknown *iface)
{
    return InterlockedDecrement(&CONTAINING_RECORD(iface, struct XUser, IUser_iface)->refs);
}
static IUnknownVtbl vtable = {query, retain, release};
int main(void)
{
    struct XUser user = {{&vtable}, 1, 123456789, TRUE, SRWLOCK_INIT};
    XUserHandle found = (XUserHandle)1, second = NULL;
    check("null-output", x_user_find_by_id(user.xuid, NULL) == E_POINTER);
    check("no-added-user", x_user_find_by_id(user.xuid, &found) == E_GAMEUSER_USER_NOT_FOUND && !found);
    check("cache-retains-user", xodus_default_user_store(&user.IUser_iface) == S_OK && user.refs == 2);
    check("null-is-not-store-user", !x_user_is_store_account(NULL));
    check("unbound-is-not-store-user", !x_user_is_store_account(&user));
    memset(current_account, 'a', 64);
    memcpy(user.store_account_context, current_account, 65);
    user.store_account_xuid = user.xuid;
    check("same-authenticated-store-account", x_user_is_store_account(&user));
    ++user.store_account_xuid;
    check("rebound-xbox-user-is-not-store-user", !x_user_is_store_account(&user));
    --user.store_account_xuid;
    current_account[0] = 'b';
    check("account-switch-is-mismatch", !x_user_is_store_account(&user));
    current_account[0] = 'a'; account_status = E_FAIL;
    check("account-service-failure-is-not-match", !x_user_is_store_account(&user));
    account_status = S_OK;
    user.xuid_syntax_valid = FALSE;
    check("invalid-user-is-not-store-user", !x_user_is_store_account(&user));
    user.xuid_syntax_valid = TRUE;
    found = (XUserHandle)1;
    check("unknown-id-no-fallback", x_user_find_by_id(user.xuid + 1, &found) == E_GAMEUSER_USER_NOT_FOUND && !found && user.refs == 2);
    found = (XUserHandle)1;
    check("zero-id-clears-output", x_user_find_by_id(0, &found) == E_GAMEUSER_USER_NOT_FOUND && !found && user.refs == 2);
    user.xuid_syntax_valid = FALSE;
    check("invalid-authenticated-id-rejected", x_user_find_by_id(user.xuid, &found) == E_GAMEUSER_USER_NOT_FOUND && !found && user.refs == 2);
    user.xuid_syntax_valid = TRUE;
    check("known-id-owned-handle", x_user_find_by_id(user.xuid, &found) == S_OK && found == &user && user.refs == 3);
    check("independent-second-reference", x_user_find_by_id(user.xuid, &second) == S_OK && second == found && user.refs == 4);
    IUnknown_Release(&second->IUser_iface);
    check("closing-one-handle-preserves-other", user.refs == 3 && found->xuid == user.xuid);
    xodus_default_user_shutdown();
    check("shutdown-releases-cache-only", user.refs == 2 && found->xuid == user.xuid);
    second = (XUserHandle)1;
    check("shutdown-rejects-lookup", x_user_find_by_id(user.xuid, &second) == E_ABORT && !second && user.refs == 2);
    IUnknown_Release(&found->IUser_iface);
    check("all-lookup-references-released", user.refs == 1);
    printf("SUMMARY checks=%d failures=%d account_calls=0\n", checks, failures);
    return failures ? 1 : 0;
}

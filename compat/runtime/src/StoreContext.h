/* SPDX-License-Identifier: LGPL-2.1-or-later */
#ifndef XODUS_LOCAL_STORE_CONTEXT_H
#define XODUS_LOCAL_STORE_CONTEXT_H
#include <windows.h>
#include <memory>

struct XStoreGameLicense;
struct XStoreProduct;
/* All pointed-to data belongs to the provider until release_product_page. */
struct XodusStoreProductPage
{
    UINT32 structure_size;
    UINT32 product_count;
    const XStoreProduct *products;
    const char *continuation;
};

/* Integration must bind these callbacks to the actual configured Xodus MSA
 * broker account. An arbitrary game XUser is not the PC Store account selector.
 * The provider state and code must outlive every created context. Every nonnull
 * acquired pointer is one owned opaque account reference (NOT an XUser),
 * released by release_account. Acquiring it asserts no license validity. */
struct XodusStoreAccountProvider
{
    void *state;
    HRESULT (WINAPI *acquire_account)(void *state, void **owned_account);
    void (WINAPI *release_account)(void *state, void *owned_account);
    HRESULT (WINAPI *query_game_license)(void *state, void *account,
                                       volatile LONG *cancelled, XStoreGameLicense *license) = nullptr;
    HRESULT (WINAPI *query_entitled_products)(void *state, void *account, UINT32 kinds,
        UINT32 page_size, const char *continuation, volatile LONG *cancelled,
        XodusStoreProductPage **owned_page) = nullptr;
    void (WINAPI *release_product_page)(void *state, XodusStoreProductPage *owned_page) = nullptr;
    /* Token allocation belongs to the provider until release_license_token.
     * size includes exactly one final NUL. No token bytes are diagnostics. */
    HRESULT (WINAPI *query_license_token)(void *state, void *account,
        const char *const *product_ids, SIZE_T count, const char *custom,
        volatile LONG *cancelled, char **owned_token, SIZE_T *size) = nullptr;
    void (WINAPI *release_license_token)(void *state, char *owned_token, SIZE_T size) = nullptr;
    /* Explicit catalog requests require both listing data and an authentic
     * collection join. An anonymous catalog alone cannot populate this page.
     * All input pointers are borrowed for this call only. Output ownership is
     * identical to query_entitled_products/release_product_page. */
    HRESULT (WINAPI *query_products)(void *state, void *account, UINT32 kinds,
        const char *const *ids, SIZE_T id_count,
        const char *const *actions, SIZE_T action_count, const char *continuation,
        volatile LONG *cancelled, XodusStoreProductPage **owned_page) = nullptr;
    /* Current Microsoft-signed exact Durable grant, capped to a 60-second
     * online observation. Catalog/Collections ownership alone is insufficient. */
    HRESULT (WINAPI *query_durable_license)(void *state, void *account, const char *store_id,
        volatile LONG *cancelled, XStoreGameLicense *license) = nullptr;
    /* S_OK only after an authenticated exact revision match for the complete
     * registered package scope. Unknown scopes/revisions must remain errors. */
    HRESULT (WINAPI *check_package_updates)(void *state, void *account,
        volatile LONG *cancelled) = nullptr;
};

class XodusStoreContextState;
using XodusStoreContextRef = std::shared_ptr<XodusStoreContextState>;
HRESULT XodusStoreContextRetain(void *context, XodusStoreContextRef *reference);
bool XodusStoreContextIsOpen(const XodusStoreContextRef &reference);
const XodusStoreAccountProvider *XodusStoreContextProvider(const XodusStoreContextRef &reference);
void *XodusStoreContextAccount(const XodusStoreContextRef &reference);
HRESULT XodusStoreReadGameLicenseObservation(const XodusStoreContextRef &reference,
                                           XStoreGameLicense *license);

/* Lifecycle core only: these are not exported GDK entry points or a complete
 * IXStore implementation. They make no entitlement/license claims. */
HRESULT XodusStoreContextCreate(const XodusStoreAccountProvider *provider,
                               const void *ignored_pc_user, void **context);
void XodusStoreContextClose(void *context);
void XodusStoreContextShutdown();
#endif

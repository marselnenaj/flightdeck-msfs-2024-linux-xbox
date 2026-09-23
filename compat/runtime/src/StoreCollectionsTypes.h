/* SPDX-License-Identifier: LGPL-2.1-or-later */
#ifndef XODUS_STORE_COLLECTIONS_TYPES_H
#define XODUS_STORE_COLLECTIONS_TYPES_H
#include <windows.h>

/* Private, versioned broker/provider ABI. No token or account identifiers are
 * returned. Every pointer belongs to the snapshot until ReleaseCollections. */
struct XodusStoreCollectionRequestItem {
    char store_id[18];
    UINT32 kind;
};
struct XodusStoreCollectionItem {
    char store_id[18];
    UINT32 kind;
    INT64 acquired_date, start_date, end_date;
    UINT32 quantity, is_trial, trial_seconds;
    const char *campaign_id;
    const char *developer_offer_id;
};
struct XodusStoreCollectionSnapshot {
    UINT32 structure_size;
    UINT32 item_count, absent_count, unknown_count;
    UINT32 direct_coverage, satisfying_coverage, shared_coverage;
    INT64 observed_at, expires_at;
    const XodusStoreCollectionItem *items;
    const char *const *absent_ids;
    const char *const *unknown_ids;
    char continuation[65];
};
#ifdef __cplusplus
extern "C" {
#endif
HRESULT WINAPI XodusStoreQueryCollections(void *owned_account,
    const XodusStoreCollectionRequestItem *products, SIZE_T count,
    volatile LONG *cancelled, XodusStoreCollectionSnapshot **owned_snapshot);
void WINAPI XodusStoreReleaseCollections(XodusStoreCollectionSnapshot *snapshot);
HRESULT WINAPI XodusStoreQueryInventory(void *owned_account, UINT32 kinds,
    UINT32 page_size, const char *market, const char *continuation,
    volatile LONG *cancelled, XodusStoreCollectionSnapshot **owned_snapshot);
#ifdef __cplusplus
}
#endif
#endif

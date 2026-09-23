// SPDX-License-Identifier: LGPL-2.1-or-later
#pragma once
#include "StoreCatalogBatch.h"
#include "StoreCatalogCoinMapper.h"

namespace xodus_catalog {
struct CollectionsProvider {
  HRESULT(WINAPI *query)(void *, const XodusStoreCollectionRequestItem *,
                         SIZE_T, volatile LONG *,
                         XodusStoreCollectionSnapshot **);
  void(WINAPI *release)(XodusStoreCollectionSnapshot *);
};
struct InventoryProvider {
  HRESULT(WINAPI *query)(void *, UINT32, UINT32, const char *, const char *,
                         volatile LONG *, XodusStoreCollectionSnapshot **);
  void(WINAPI *release)(XodusStoreCollectionSnapshot *);
};
HRESULT query_entitled(CatalogReader &reader, const InventoryProvider &provider,
                       void *store_account, const std::string &parent,
                       const std::string &market, const std::string &language,
                       UINT32 kinds, UINT32 page_size, const char *cursor,
                       volatile LONG *cancelled, XodusStoreProductPage **out);
// Inputs are borrowed only for this call. A successful page owns all output
// strings/arrays and must be released using release_coin_page.
HRESULT query_coins(CatalogReader &reader, const CollectionsProvider &provider,
                    void *store_account, const std::string &parent,
                    const std::string &market, const std::string &language,
                    UINT32 kinds, const char *const *ids, SIZE_T count,
                    const char *const *actions, SIZE_T action_count,
                    const char *cursor, volatile LONG *cancelled,
                    XodusStoreProductPage **out);
// Optional explicit market/language overrides select public catalog prices;
// they never select the Microsoft account or imply ownership.
HRESULT catalog_locale(std::string *market, std::string *language);
} // namespace xodus_catalog

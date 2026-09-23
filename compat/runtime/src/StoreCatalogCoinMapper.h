// SPDX-License-Identifier: LGPL-2.1-or-later
#pragma once
#include "StoreCatalog.h"
#include "StoreCollectionsTypes.h"
#include "StoreContext.h"
#include <memory>

namespace xodus_catalog {
struct CoinCatalogPlan;
using CoinPlan = std::shared_ptr<const CoinCatalogPlan>;
// Product.raw_json must contain the exact validated catalog document. The
// supported first subset is non-subscription consumables with explicit title
// association, simple public pricing and no downloadable/media payload.
HRESULT plan_coins(const std::vector<Product> &catalog,
                   const std::vector<std::string> &ids, UINT32 kinds,
                   const std::vector<std::string> &actions,
                   const std::string &parent, const std::string &market,
                   const std::string &language, INT64 now, CoinPlan *out,
                   std::vector<XodusStoreCollectionRequestItem> *requests,
                   bool entitled = false);
HRESULT coin_page(const CoinPlan &plan,
                  const XodusStoreCollectionSnapshot *collection, INT64 now,
                  XodusStoreProductPage **out, const char *continuation = nullptr);
// Returns false for a page owned by another provider. Never dereferences it.
bool release_coin_page(XodusStoreProductPage *page);
} // namespace xodus_catalog

// SPDX-License-Identifier: LGPL-2.1-or-later
#pragma once
#include "StoreCatalog.h"
#include <memory>
namespace xodus_catalog {
using FetchProduct = HRESULT (*)(const std::string &, const std::string &,
                                 const std::string &, bool, volatile LONG *,
                                 Product *, ULONGLONG);
class CatalogReader {
  struct State;
  std::unique_ptr<State> state;

public:
  explicit CatalogReader(FetchProduct source = fetch_until);
  ~CatalogReader();
  CatalogReader(const CatalogReader &) = delete;
  // Returns unique products in first-requested order. SKU filtering stays at
  // the caller. The entire query has one deadline, at most four HTTP workers,
  // and one cancellation signal; no partially successful output is exposed.
  HRESULT read(const std::vector<std::string> &ids, const std::string &market,
               const std::string &language, volatile LONG *cancelled,
               std::vector<Product> *out, ULONGLONG deadline = 0,
               bool keep_raw = false);
};
} // namespace xodus_catalog

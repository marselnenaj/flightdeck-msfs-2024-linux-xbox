// SPDX-License-Identifier: LGPL-2.1-or-later
#include "StoreCatalogProvider.h"
#include <algorithm>
#include <atomic>
#include <cstdio>
#include <memory>
#include <xstore.h>

namespace xodus_catalog {
namespace {
bool cancelled_now(volatile LONG *value) {
  return value && InterlockedCompareExchange(value, 0, 0);
}
INT64 now_utc() {
  FILETIME ft;
  GetSystemTimeAsFileTime(&ft);
  ULARGE_INTEGER value;
  value.LowPart = ft.dwLowDateTime;
  value.HighPart = ft.dwHighDateTime;
  return static_cast<INT64>(value.QuadPart / 10000000) - 11644473600LL;
}
HRESULT diagnose(const char *stage, HRESULT hr) {
  static std::atomic<unsigned> calls{0};
  if (calls.fetch_add(1) < 128)
    std::fprintf(stderr, "[xodus-store-catalog] stage=%s hr=%08lx\n", stage,
                 static_cast<ULONG>(hr));
  return hr;
}
bool root_id(const std::string &value) {
  return value.size() == 12 &&
         std::all_of(value.begin(), value.end(), [](char c) {
           return (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9');
         });
}
bool override_text(const char *name, char *buffer, DWORD size, bool *present) {
  SetLastError(ERROR_SUCCESS);
  DWORD length = GetEnvironmentVariableA(name, buffer, size);
  *present = length || GetLastError() != ERROR_ENVVAR_NOT_FOUND;
  return !*present || (length && length < size);
}
} // namespace

HRESULT catalog_locale(std::string *market, std::string *language) {
  if (!market || !language)
    return E_POINTER;
  market->clear();
  language->clear();
  char country[3]{}, locale[32]{};
  bool country_override = false, language_override = false;
  if (!override_text("XODUS_STORE_MARKET", country, sizeof(country),
                     &country_override) ||
      !override_text("XODUS_STORE_LANGUAGE", locale, sizeof(locale),
                     &language_override))
    return E_INVALIDARG;
  if (!country_override && !GetGeoInfoA(GetUserGeoID(GEOCLASS_NATION), GEO_ISO2,
                                        country, sizeof(country), 0))
    return E_NOTIMPL;
  if (!language_override) {
    WCHAR name[LOCALE_NAME_MAX_LENGTH]{};
    if (!GetUserDefaultLocaleName(name, LOCALE_NAME_MAX_LENGTH) ||
        !WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, name, -1, locale,
                             sizeof(locale), nullptr, nullptr))
      return E_NOTIMPL;
  }
  const std::string m(country), l(locale);
  if (m.size() != 2 ||
      !std::all_of(m.begin(), m.end(),
                   [](char c) { return c >= 'A' && c <= 'Z'; }) ||
      l.empty() || l.size() > 31 ||
      !std::all_of(l.begin(), l.end(), [](char c) {
        return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') ||
               (c >= '0' && c <= '9') || c == '-';
      }))
    return E_INVALIDARG;
  *market = m;
  *language = l;
  return S_OK;
}

HRESULT query_coins(CatalogReader &reader, const CollectionsProvider &provider,
                    void *store_account, const std::string &parent,
                    const std::string &market, const std::string &language,
                    UINT32 kinds, const char *const *ids, SIZE_T count,
                    const char *const *actions, SIZE_T action_count,
                    const char *cursor, volatile LONG *cancelled,
                    XodusStoreProductPage **out) {
  if (!out)
    return E_POINTER;
  *out = nullptr;
  if (!provider.query || !provider.release)
    return E_NOTIMPL;
  if (!store_account || !cancelled || !ids || !count || count > 100 ||
      action_count > 64 || (action_count && !actions) || !root_id(parent))
    return E_INVALIDARG;
  if (cursor && *cursor)
    return E_NOTIMPL; // This bounded explicit request returns one complete
                      // page.
  if (cancelled_now(cancelled))
    return E_ABORT;
  try {
    std::vector<std::string> requested, roots, filters;
    for (SIZE_T i = 0; i < count; ++i) {
      if (!ids[i])
        return E_INVALIDARG;
      const SIZE_T length = strnlen(ids[i], 18);
      if ((length != 12 && length != 17) || !root_id(std::string(ids[i], 12)))
        return E_INVALIDARG;
      if (length == 17) {
        if (ids[i][12] != '/' ||
            !std::all_of(ids[i] + 13, ids[i] + 17, [](char c) {
              return (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9');
            }))
          return E_INVALIDARG;
      }
      requested.emplace_back(ids[i], length);
      roots.emplace_back(ids[i], 12);
    }
    for (SIZE_T i = 0; i < action_count; ++i) {
      if (!actions[i] || strnlen(actions[i], 64) == 64)
        return E_INVALIDARG;
      filters.emplace_back(actions[i]);
    }
    std::vector<Product> catalog;
    HRESULT hr = reader.read(roots, market, language, cancelled, &catalog,
                             GetTickCount64() + 30000, true);
    if (FAILED(hr))
      return diagnose("catalog", hr);
    CoinPlan plan;
    std::vector<XodusStoreCollectionRequestItem> requests;
    hr = plan_coins(catalog, requested, kinds, filters, parent, market,
                    language, now_utc(), &plan, &requests);
    if (FAILED(hr))
      return diagnose("mapping", hr);
    if (cancelled_now(cancelled))
      return E_ABORT;
    XodusStoreCollectionSnapshot *raw = nullptr;
    // The broker receives the already-bound Store account. Catalog data alone
    // never makes an ownership assertion; publisher wallet balances are outside
    // this contract, including for developer-managed consumables.
    if (!requests.empty())
      hr = provider.query(store_account, requests.data(), requests.size(),
                          cancelled, &raw);
    std::unique_ptr<XodusStoreCollectionSnapshot, decltype(provider.release)>
        snapshot(raw, provider.release);
    if (FAILED(hr))
      return diagnose("collections", hr);
    if (cancelled_now(cancelled))
      return E_ABORT;
    hr = coin_page(plan, snapshot.get(), now_utc(), out);
    if (SUCCEEDED(hr) && cancelled_now(cancelled)) {
      release_coin_page(*out);
      *out = nullptr;
      return E_ABORT;
    }
    if (SUCCEEDED(hr)) {
      static std::atomic<unsigned> result_calls{0};
      if (result_calls.fetch_add(1) < 64) {
        UINT32 skus = 0;
        for (UINT32 i = 0; i < (*out)->product_count; ++i)
          skus += (*out)->products[i].skusCount;
        std::fprintf(
            stderr,
            "[xodus-store-catalog] stage=page products=%u skus=%u hr=%08lx\n",
            (*out)->product_count, skus, static_cast<ULONG>(hr));
      }
    }
    return diagnose("result", hr);
  } catch (const std::bad_alloc &) {
    return E_OUTOFMEMORY;
  } catch (...) {
    return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
  }
}
} // namespace xodus_catalog

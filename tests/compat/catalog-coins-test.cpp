// SPDX-License-Identifier: LGPL-2.1-or-later
#define main original_catalog_main
#include "catalog-test.cpp"
#undef main
#include "StoreCatalogProvider.h"
#include <atomic>
#include <xstore.h>
using namespace xodus_catalog;
static Json source;
static std::atomic<int> fetches{0}, joins{0}, releases{0};
static int response_mode = 0;
static bool fetch_fails = false;
static INT64 now_utc() {
  FILETIME ft;
  GetSystemTimeAsFileTime(&ft);
  ULARGE_INTEGER t;
  t.LowPart = ft.dwLowDateTime;
  t.HighPart = ft.dwHighDateTime;
  return t.QuadPart / 10000000 - 11644473600LL;
}
static Json complete_fixture() {
  Json f = fixture();
  auto &p = f["Product"], &s = p["DisplaySkuAvailabilities"][0]["Sku"];
  p["MarketProperties"] =
      Json::array({{{"Markets", Json::array({"AT"})},
                    {"RelatedProducts",
                     Json::array({{{"RelatedProductId", "PARENT123456"},
                                   {"RelationshipType", "addOnParent"}},
                                  {{"RelatedProductId", "PARENT123456"},
                                   {"RelationshipType", "SellableBy"}}})}}});
  s["Properties"] = {{"IsTrial", false},
                     {"Packages", Json::array()},
                     {"BundledSkus", nullptr}};
  s["RecurrencePolicy"] = nullptr;
  s["SubscriptionPolicyId"] = nullptr;
  auto &a = p["DisplaySkuAvailabilities"][0]["Availabilities"][0];
  a["Conditions"]["StartDate"] = "1753-01-01T00:00:00Z";
  a["Conditions"]["ClientConditions"]["AllowedPlatforms"][0]["MinVersion"] = 0;
  a["Conditions"]["ClientConditions"]["AllowedPlatforms"][0]["MaxVersion"] =
      2147483647;
  Json mobile = a;
  mobile["AvailabilityId"] = "MOBILE-ONLY";
  mobile["Actions"] = Json::array();
  mobile["Conditions"]["ClientConditions"]["AllowedPlatforms"][0]
        ["PlatformName"] = "Windows.Mobile";
  mobile["OrderManagementData"]["Price"] = {
      {"ListPrice", 0}, {"MSRP", 0}, {"CurrencyCode", "USD"}};
  p["DisplaySkuAvailabilities"][0]["Availabilities"].push_back(mobile);
  return f;
}
static HRESULT mock_fetch(const std::string &id, const std::string &market,
                          const std::string &language, bool keep,
                          volatile LONG *cancel, Product *out, ULONGLONG) {
  ++fetches;
  if (fetch_fails)
    return E_FAIL;
  if (*cancel)
    return E_ABORT;
  if (market != "AT" || language != "en-US")
    return E_INVALIDARG;
  return parse(source.dump(), id, keep, out);
}
struct Snapshot {
  XodusStoreCollectionSnapshot value{};
  std::vector<XodusStoreCollectionItem> items;
  std::vector<std::string> strings;
  std::vector<const char *> absent, unknown;
};
static HRESULT WINAPI mock_join(void *account,
                                const XodusStoreCollectionRequestItem *requests,
                                SIZE_T count, volatile LONG *cancel,
                                XodusStoreCollectionSnapshot **out) {
  ++joins;
  *out = nullptr;
  if (account != reinterpret_cast<void *>(0x1234) || count != 1 ||
      strcmp(requests[0].store_id, "ABCD1234EFGH/0001") ||
      requests[0].kind != 16)
    return E_INVALIDARG;
  if (response_mode == 2)
    return E_NOTIMPL;
  auto snap = std::make_unique<Snapshot>();
  auto &v = snap->value;
  v.structure_size = sizeof(v);
  v.direct_coverage = v.satisfying_coverage = 1;
  v.observed_at = now_utc();
  v.expires_at = v.observed_at + 30;
  snap->strings.push_back(requests[0].store_id);
  if (response_mode == 1) {
    XodusStoreCollectionItem item{};
    memcpy(item.store_id, requests[0].store_id, 18);
    item.kind = 16;
    item.acquired_date = item.start_date = v.observed_at - 100;
    item.end_date = v.observed_at + 1000;
    item.quantity = 7;
    item.campaign_id = "synthetic-campaign";
    item.developer_offer_id = "synthetic-offer";
    snap->items.push_back(item);
    v.item_count = 1;
    v.items = snap->items.data();
  } else if (response_mode == 3) {
    snap->unknown.push_back(snap->strings[0].c_str());
    v.unknown_count = 1;
    v.unknown_ids = snap->unknown.data();
  } else {
    snap->absent.push_back(snap->strings[0].c_str());
    v.absent_count = 1;
    v.absent_ids = snap->absent.data();
  }
  if (response_mode == 4)
    InterlockedExchange(cancel, 1);
  if (response_mode == 5)
    v.expires_at = v.observed_at;
  *out = &snap.release()->value;
  return response_mode == 6 ? E_FAIL : S_OK;
}
static void WINAPI mock_release(XodusStoreCollectionSnapshot *value) {
  ++releases;
  delete reinterpret_cast<Snapshot *>(value);
}
static HRESULT query(XodusStoreProductPage **out, UINT32 kinds = 31,
                     const char *action = nullptr) {
  CatalogReader reader(mock_fetch);
  const CollectionsProvider provider{mock_join, mock_release};
  volatile LONG cancel = 0;
  const char *id = "ABCD1234EFGH";
  return query_coins(reader, provider, reinterpret_cast<void *>(0x1234),
                     "PARENT123456", "AT", "en-US", kinds, &id, 1,
                     action ? &action : nullptr, action ? 1 : 0, nullptr,
                     &cancel, out);
}
int main() {
  source = complete_fixture();
  XodusStoreProductPage *page = nullptr;
  check("reader-mapper-join-page", query(&page) == S_OK && page &&
                                       page->product_count == 1 && joins == 1 &&
                                       releases == 1);
  if (page) {
    const auto &p = page->products[0];
    check("desktop-price-ignores-mobile-zero",
          p.price.price == 10.25f && strcmp(p.price.currencyCode, "EUR") == 0 &&
              p.skusCount == 1 && p.skus[0].availabilitiesCount == 1);
    check("store-absence-not-wallet-balance",
          !p.isInUserCollection && !p.skus[0].isInUserCollection &&
              p.skus[0].collectionData.quantity == 0);
    source = nullptr;
    check("page-outlives-catalog-plan-and-snapshot",
          strcmp(p.title, "Synthetic coins") == 0 &&
              strcmp(p.skus[0].skuId, "0001") == 0 &&
              strcmp(p.skus[0].availabilities[0].availabilityId,
                     "SYNTHETIC-AVAILABILITY") == 0);
    check("release-owned-page", release_coin_page(page));
    check("release-twice-not-owned", !release_coin_page(page));
  }
  source = complete_fixture();
  response_mode = 1;
  page = nullptr;
  check("actual-owned-item",
        query(&page) == S_OK && page && page->products[0].isInUserCollection &&
            page->products[0].skus[0].collectionData.quantity == 7);
  if (page) {
    check("owned-strings-copied-before-release",
          strcmp(page->products[0].skus[0].collectionData.campaignId,
                 "synthetic-campaign") == 0);
    release_coin_page(page);
  }
  for (int mode : {2, 3, 4, 5, 6}) {
    const int before_release = releases.load();
    response_mode = mode;
    page = reinterpret_cast<XodusStoreProductPage *>(1);
    check(mode == 2   ? "provider-unsupported-no-empty-success"
          : mode == 3 ? "unknown-ownership-fails"
          : mode == 4 ? "cancel-after-join-releases"
          : mode == 5 ? "expired-snapshot-fails"
                      : "failed-provider-with-owned-output",
          FAILED(query(&page)) && !page);
    check("failure-cancel-owned-snapshot-released",
          releases == before_release + (mode == 2 ? 0 : 1));
  }
  response_mode = 0;
  auto negative = [&](const char *name, Json value) {
    source = std::move(value);
    page = reinterpret_cast<XodusStoreProductPage *>(1);
    check(name, FAILED(query(&page)) && !page);
  };
  Json bad = complete_fixture();
  bad["Product"]["MarketProperties"][0]["RelatedProducts"][0]
     ["RelatedProductId"] = "OTHER1234567";
  negative("foreign-title", bad);
  bad = complete_fixture();
  bad["Product"]["MarketProperties"][0]["RelatedProducts"][1]
     ["RelatedProductId"] = "OTHER1234567";
  negative("foreign-sellable-by", bad);
  bad = complete_fixture();
  bad["Product"]["ProductKind"] = "Durable";
  negative("shared-durable-remains-unsupported", bad);
  bad = complete_fixture();
  bad["Product"]["DisplaySkuAvailabilities"][0]["Sku"]["Properties"]
     ["IsTrial"] = true;
  negative("trial-sku-not-in-subset", bad);
  bad = complete_fixture();
  bad["Product"]["DisplaySkuAvailabilities"][0]["Sku"]["SubscriptionPolicyId"] =
      "synthetic";
  negative("subscription-not-in-subset", bad);
  bad = complete_fixture();
  bad["Product"]["DisplaySkuAvailabilities"][0]["Sku"]["Properties"]
     ["Packages"] = Json::array({"synthetic"});
  negative("download-not-in-subset", bad);
  bad = complete_fixture();
  bad["Product"]["DisplaySkuAvailabilities"][0]["Availabilities"][0]
     ["Conditions"]["UnknownConstraint"] = true;
  negative("unknown-desktop-constraint", bad);
  bad = complete_fixture();
  bad["Product"]["DisplaySkuAvailabilities"][0]["Availabilities"][0]
     ["OrderManagementData"]["Price"]["MSRP"] = 1e40;
  negative("float-overflow", bad);
  bad = complete_fixture();
  bad["Product"]["DisplaySkuAvailabilities"][0]["Availabilities"][0]
     ["Conditions"]["StartDate"] = "1753-01-01T00:00:00.Z";
  negative("empty-fraction", bad);
  source = complete_fixture();
  int before = joins;
  page = nullptr;
  check("actual-kind-filter-empty", query(&page, 1) == S_OK && page &&
                                        page->product_count == 0 &&
                                        joins == before);
  if (page)
    release_coin_page(page);
  page = nullptr;
  check("actual-action-filter-empty", query(&page, 31, "Fulfill") == S_OK &&
                                          page && page->product_count == 0 &&
                                          joins == before);
  if (page)
    release_coin_page(page);
  fetch_fails = true;
  page = nullptr;
  check("catalog-error-no-join",
        query(&page) == E_FAIL && !page && joins == before);
  fetch_fails = false;
  SetEnvironmentVariableA("XODUS_STORE_MARKET", "AT");
  SetEnvironmentVariableA("XODUS_STORE_LANGUAGE", "en-US");
  std::string m, l;
  check("explicit-public-catalog-region",
        catalog_locale(&m, &l) == S_OK && m == "AT" && l == "en-US");
  SetEnvironmentVariableA("XODUS_STORE_MARKET", "AT&query");
  check("invalid-region",
        catalog_locale(&m, &l) == E_INVALIDARG && m.empty() && l.empty());
  SetEnvironmentVariableA("XODUS_STORE_MARKET", "AT");
  SetEnvironmentVariableA("XODUS_STORE_LANGUAGE", "");
  check("empty-language", catalog_locale(&m, &l) == E_INVALIDARG);
  std::printf("coin-provider checks=%d failures=%d external_requests=0\n",
              checks, failures);
  return failures ? 1 : 0;
}

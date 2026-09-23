// SPDX-License-Identifier: LGPL-2.1-or-later
#include "StoreCatalogCoinMapper.h"
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <deque>
#include <limits>
#include <map>
#include <mutex>
#include <set>
#include <vendor/nlohmann_json.hpp>
#include <xstore.h>

namespace xodus_catalog {
namespace {
using Json = nlohmann::json;
struct Offer {
  std::string id, currency;
  double base = 0, price = 0;
  INT64 end = 0;
};
struct Coin {
  std::string id, sku, offer_token, title, description, language, sku_title,
      sku_description, sku_language;
  std::vector<Image> images, sku_images;
  std::vector<std::string> keywords;
  std::vector<Offer> offers;
  bool download = false;
};
bool string_is(const Json &v, const char *s) {
  return v.is_string() && v.get<std::string>() == s;
}
bool empty_media(const Json &o, const char *field) {
  return !o.count(field) || o.at(field).is_null() ||
         (o.at(field).is_array() && o.at(field).empty());
}
bool boolean_is(const Json &o, const char *field, bool wanted) {
  return o.count(field) && o.at(field).is_boolean() &&
         o.at(field).get<bool>() == wanted;
}
bool contains(const Json &array, const std::string &v) {
  if (!array.is_array())
    return false;
  for (const auto &e : array)
    if (e.is_string() && e.get<std::string>() == v)
      return true;
  return false;
}
const Text *language_text(const std::vector<Text> &values,
                          const std::string &language,
                          const std::string &market) {
  const Text *fallback = nullptr;
  const auto short_name = language.substr(0, language.find('-'));
  for (const auto &v : values) {
    if (std::find(v.markets.begin(), v.markets.end(), market) ==
        v.markets.end())
      continue;
    if (v.language == language)
      return &v;
    if (v.language == short_name)
      fallback = &v;
  }
  return fallback;
}
bool utc(const std::string &s, INT64 *out) {
  if (s.size() < 20 || s.back() != 'Z' || s[4] != '-' || s[7] != '-' ||
      s[10] != 'T' || s[13] != ':' || s[16] != ':')
    return false;
  auto num = [&](size_t pos, size_t n) -> int {
    int v = 0;
    for (size_t i = pos; i < pos + n; ++i) {
      if (s[i] < '0' || s[i] > '9')
        return -1;
      v = v * 10 + s[i] - '0';
    }
    return v;
  };
  if (s.size() > 20) {
    if (s[19] != '.' || s.size() < 22 || s.size() > 30)
      return false;
    for (size_t i = 20; i + 1 < s.size(); ++i)
      if (s[i] < '0' || s[i] > '9')
        return false;
  }
  int y = num(0, 4), mo = num(5, 2), d = num(8, 2), h = num(11, 2),
      mi = num(14, 2), se = num(17, 2);
  if (y < 1601 || y > 9999 || mo < 1 || mo > 12 || d < 1 || d > 31 || h < 0 ||
      h > 23 || mi < 0 || mi > 59 || se < 0 || se > 59)
    return false;
  SYSTEMTIME st{};
  st.wYear = y;
  st.wMonth = mo;
  st.wDay = d;
  st.wHour = h;
  st.wMinute = mi;
  st.wSecond = se;
  FILETIME ft;
  if (!SystemTimeToFileTime(&st, &ft))
    return false;
  SYSTEMTIME checked{};
  if (!FileTimeToSystemTime(&ft, &checked) || checked.wYear != y ||
      checked.wMonth != mo || checked.wDay != d)
    return false;
  ULARGE_INTEGER v;
  v.LowPart = ft.dwLowDateTime;
  v.HighPart = ft.dwHighDateTime;
  *out = static_cast<INT64>(v.QuadPart / 10000000) - 11644473600LL;
  return true;
}
bool associated(const Json &p, const std::string &parent,
                const std::string &market, const char *relationship) {
  if (!p.count("MarketProperties") || !p.at("MarketProperties").is_array())
    return false;
  for (const auto &m : p.at("MarketProperties")) {
    if (!m.is_object() || !m.count("Markets") ||
        !contains(m.at("Markets"), market) || !m.count("RelatedProducts") ||
        !m.at("RelatedProducts").is_array())
      continue;
    for (const auto &r : m.at("RelatedProducts"))
      if (r.is_object() && r.count("RelatedProductId") &&
          r.count("RelationshipType") &&
          string_is(r.at("RelatedProductId"), parent.c_str()) &&
          string_is(r.at("RelationshipType"), relationship))
        return true;
  }
  return false;
}
bool unrestricted(const Json &a) {
  const auto &c = a.at("Conditions");
  if (!c.is_object())
    return false;
  for (auto it = c.begin(); it != c.end(); ++it)
    if (it.key() != "StartDate" && it.key() != "EndDate" &&
        it.key() != "ClientConditions" && it.key() != "ResourceSetIds")
      return false;
  if (!c.count("ClientConditions") || !c.at("ClientConditions").is_object())
    return false;
  const auto &client = c.at("ClientConditions");
  if (client.size() != 1 || !client.count("AllowedPlatforms") ||
      !client.at("AllowedPlatforms").is_array())
    return false;
  bool desktop = false;
  for (const auto &platform : client.at("AllowedPlatforms")) {
    if (!platform.is_object() || !platform.count("PlatformName") ||
        !string_is(platform.at("PlatformName"), "Windows.Desktop"))
      continue;
    if (!platform.count("MinVersion") || !platform.count("MaxVersion") ||
        platform.at("MinVersion") != 0 ||
        platform.at("MaxVersion") != 2147483647)
      return false;
    desktop = true;
  }
  if (!desktop)
    return false;
  if (a.count("OrderManagementData")) {
    const auto &o = a.at("OrderManagementData");
    if (o.count("PIFilter") && !o.at("PIFilter").is_null()) {
      const auto &f = o.at("PIFilter");
      if (!f.is_object())
        return false;
      for (auto it = f.begin(); it != f.end(); ++it)
        if (!it.value().is_array() || !it.value().empty())
          return false;
    }
  }
  return true;
}
// The page owns all arrays and strings. Public ABI pointers borrow these until
// release_coin_page; they never refer to the catalog or broker response.
struct PageOwner {
  XodusStoreProductPage page{};
  std::vector<XStoreProduct> products;
  std::vector<XStoreSku> skus;
  std::vector<std::vector<XStoreAvailability>> offers;
  std::vector<std::vector<XStoreImage>> images, sku_images;
  std::vector<std::vector<const char *>> keywords;
  std::deque<std::string> text;
  const char *keep(const std::string &s) {
    text.push_back(s);
    return text.back().c_str();
  }
  char *keep_mutable(const std::string &s) {
    text.push_back(s);
    return text.back().data();
  }
  void copy_images(const std::vector<Image> &source,
                   std::vector<XStoreImage> *destination) {
    destination->resize(source.size());
    for (size_t i = 0; i < source.size(); ++i) {
      auto &out = (*destination)[i];
      out.uri = keep(source[i].uri);
      out.width = source[i].width;
      out.height = source[i].height;
      out.caption = source[i].caption.empty() ? nullptr : keep(source[i].caption);
      out.imagePurposeTag = keep(source[i].purpose);
    }
  }
  HRESULT price(const Offer &o, XStorePrice *out) {
    if (!std::isfinite(o.price) || !std::isfinite(o.base) || o.price < 0 ||
        o.base < o.price || o.base > std::numeric_limits<float>::max())
      return E_NOTIMPL;
    *out = {};
    out->basePrice = static_cast<float>(o.base);
    out->price = static_cast<float>(o.price);
    out->currencyCode = keep(o.currency);
    int a =
        std::snprintf(out->formattedBasePrice, sizeof(out->formattedBasePrice),
                      "%s %.2f", o.currency.c_str(), o.base);
    int b = std::snprintf(out->formattedPrice, sizeof(out->formattedPrice),
                          "%s %.2f", o.currency.c_str(), o.price);
    if (a < 0 || a >= int(sizeof(out->formattedBasePrice)) || b < 0 ||
        b >= int(sizeof(out->formattedPrice)))
      return E_NOTIMPL;
    // This subset is explicitly nonrecurring; no recurring charge or sale
    // end applies when there is no price reduction.
    out->isOnSale = o.price < o.base;
    out->saleEndDate = out->isOnSale ? o.end : 0;
    return S_OK;
  }
};
std::mutex pages_mutex;
std::map<XodusStoreProductPage *, std::unique_ptr<PageOwner>> pages;
} // namespace
struct CoinCatalogPlan {
  std::vector<Coin> coins;
  std::map<std::string, UINT32> kinds;
  bool entitled = false;
};

HRESULT plan_coins(const std::vector<Product> &catalog,
                   const std::vector<std::string> &ids, UINT32 kinds,
                   const std::vector<std::string> &actions,
                   const std::string &parent, const std::string &market,
                   const std::string &language, INT64 now, CoinPlan *out,
                   std::vector<XodusStoreCollectionRequestItem> *requests,
                   bool entitled) {
  if (!out || !requests)
    return E_POINTER;
  out->reset();
  requests->clear();
  if ((!entitled && ids.empty()) || ids.size() > 100 || !kinds || (kinds & ~31u))
    return E_INVALIDARG;
  if (actions.size() > 1)
    return E_NOTIMPL;
  try {
    auto plan = std::make_shared<CoinCatalogPlan>();
    plan->entitled = entitled;
    std::set<std::string> processed;
    for (const auto &requested : ids) {
      std::string id = requested.substr(0, 12),
                  sku = requested.size() == 17 ? requested.substr(13) : "";
      if ((requested.size() != 12 && requested.size() != 17) ||
          (requested.size() == 17 && requested[12] != '/'))
        return E_INVALIDARG;
      auto found = std::find_if(catalog.begin(), catalog.end(),
                                [&](const Product &p) { return p.id == id; });
      if (found == catalog.end())
        return HRESULT_FROM_WIN32(ERROR_NOT_FOUND);
      const Product &product = *found;
      if (!(kinds & product.kind))
        continue;
      // Package-free Durables can be described by the same catalog shape.
      // Their ownership is reported only with a positive Collections record;
      // absence is unknown because device-shared licenses are not covered.
      if (product.kind != 1 && product.kind != 2 && product.kind != 16 && !(entitled && product.kind == 4))
        return E_NOTIMPL;
      if (product.raw_json.empty())
        return E_INVALIDARG;
      auto json = Json::parse(product.raw_json);
      const auto &p = json.at("Product");
      if (!(entitled && product.id == parent) && !associated(p, parent, market, "addOnParent"))
        return E_NOTIMPL;
      auto selected = std::find_if(product.skus.begin(), product.skus.end(),
          [&](const Sku &candidate) { return sku.empty() || candidate.id == sku; });
      if ((!entitled && product.skus.size() != 1) || selected == product.skus.end() || (entitled && sku.empty()))
        return E_NOTIMPL;
      const auto selected_index = static_cast<size_t>(selected - product.skus.begin());
      const auto &s = p.at("DisplaySkuAvailabilities")[selected_index].at("Sku");
      const auto &props = s.at("Properties");
      if (!boolean_is(props, "IsTrial", false) || !props.count("Packages") ||
          !props.at("Packages").is_array() || (!entitled && !props.at("Packages").empty()) ||
          !empty_media(props, "BundledSkus") || !s.count("RecurrencePolicy") ||
          !s.at("RecurrencePolicy").is_null() ||
          !s.count("SubscriptionPolicyId") ||
          !s.at("SubscriptionPolicyId").is_null())
        return E_NOTIMPL;
      for (const auto &localized : p.at("LocalizedProperties"))
        if (!entitled && !empty_media(localized, "Videos"))
          return E_NOTIMPL;
      for (const auto &localized : s.at("LocalizedProperties"))
        if (!entitled && !empty_media(localized, "Videos"))
          return E_NOTIMPL;
      const auto *pt = language_text(product.localized, language, market);
      const auto *st =
          language_text(selected->localized, language, market);
      if (!pt || !st)
        return E_NOTIMPL;
      Coin coin;
      coin.id = id;
      coin.sku = selected->id;
      coin.download = !props.at("Packages").empty();
      coin.offer_token = product.offer_token;
      coin.title = pt->title;
      coin.description = pt->description;
      coin.language = pt->language;
      coin.sku_title = st->title;
      coin.sku_description = st->description;
      coin.sku_language = st->language;
      coin.images = pt->images;
      coin.sku_images = st->images;
      coin.keywords = pt->keywords;
      const auto &raw_offers =
          p.at("DisplaySkuAvailabilities")[selected_index].at("Availabilities");
      for (size_t i = 0; i < selected->availabilities.size(); ++i) {
        const auto &a = selected->availabilities[i];
        INT64 start, end;
        if (std::find(a.markets.begin(), a.markets.end(), market) ==
            a.markets.end())
          continue;
        if (!actions.empty() && std::find(a.actions.begin(), a.actions.end(),
                                          actions[0]) == a.actions.end())
          continue;
        // Mobile-only offers are not candidate prices for a desktop game.
        if (std::find(a.platforms.begin(), a.platforms.end(),
                      "Windows.Desktop") == a.platforms.end())
          continue;
        if (std::find(a.actions.begin(), a.actions.end(), "Purchase") !=
                a.actions.end() &&
            !associated(p, parent, market, "SellableBy")) {
          if (entitled) continue;
          return E_NOTIMPL;
        }
        if (!utc(a.start_date, &start) || !utc(a.end_date, &end))
          return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
        if (start > now || end <= now)
          continue;
        if (!a.has_price || !unrestricted(raw_offers.at(i))) {
          if (entitled) continue;
          return E_NOTIMPL;
        }
        if (std::abs(std::round(a.price.list_price * 100) -
                     a.price.list_price * 100) > 0.00001 ||
            std::abs(std::round(a.price.base_price * 100) -
                     a.price.base_price * 100) > 0.00001)
          return E_NOTIMPL;
        coin.offers.push_back(Offer{a.id, a.price.currency, a.price.base_price,
                                    a.price.list_price, end});
      }
      if (coin.offers.empty() && !entitled) {
        if (!actions.empty())
          continue;
        return E_NOTIMPL;
      }
      for (const auto &o : coin.offers)
        if (o.currency != coin.offers[0].currency ||
            o.price != coin.offers[0].price || o.base != coin.offers[0].base) {
          if (!entitled) return E_NOTIMPL;
          // Personalized/ambiguous prices are not needed to list an owned SKU.
          coin.offers.clear(); break;
        }
      const auto exact = id + "/" + coin.sku;
      if (!processed.insert(exact).second)
        continue;
      plan->kinds.emplace(exact, product.kind);
      plan->coins.push_back(std::move(coin));
    }
    if (entitled) std::sort(plan->coins.begin(), plan->coins.end(), [](const Coin &a, const Coin &b) {
      return a.id == b.id ? a.sku < b.sku : a.id < b.id;
    });
    for (const auto &p : plan->kinds) {
      XodusStoreCollectionRequestItem item{};
      memcpy(item.store_id, p.first.c_str(), 18);
      item.kind = p.second;
      requests->push_back(item);
    }
    *out = std::move(plan);
    return S_OK;
  } catch (const std::bad_alloc &) {
    return E_OUTOFMEMORY;
  } catch (...) {
    return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
  }
}
HRESULT coin_page(const CoinPlan &plan,
                  const XodusStoreCollectionSnapshot *collection, INT64 now,
                  XodusStoreProductPage **out, const char *continuation) {
  if (!out)
    return E_POINTER;
  *out = nullptr;
  if (!plan)
    return E_INVALIDARG;
  try {
    // Collection rights belong to the bound Microsoft Store account. They are
    // not the publisher-managed in-game currency or wallet balance.
    std::map<std::string, const XodusStoreCollectionItem *> owned;
    std::set<std::string> absent, seen;
    if (!plan->coins.empty()) {
      if (!collection || collection->structure_size != sizeof(*collection) ||
          collection->direct_coverage != 1 ||
          collection->satisfying_coverage != 1 || collection->unknown_count ||
          collection->shared_coverage > 1 || collection->observed_at < 0 ||
          collection->expires_at <= now || collection->observed_at > now ||
          collection->expires_at - collection->observed_at > 30)
        return E_NOTIMPL;
      if (collection->item_count > 100 || collection->absent_count > 100 ||
          (collection->item_count && !collection->items) ||
          (collection->absent_count && !collection->absent_ids))
        return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
      for (UINT32 i = 0; i < collection->item_count; ++i) {
        const auto &item = collection->items[i];
        if (!memchr(item.store_id, 0, sizeof(item.store_id)))
          return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
        auto k = plan->kinds.find(item.store_id);
        if (k == plan->kinds.end() || k->second != item.kind ||
            item.acquired_date < 0 || item.acquired_date > now ||
            item.start_date < 0 || item.start_date > now ||
            item.end_date <= now || item.is_trial > 1 ||
            (!item.is_trial && item.trial_seconds) ||
            !seen.insert(item.store_id).second)
          return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
        owned[item.store_id] = &item;
      }
      for (UINT32 i = 0; i < collection->absent_count; ++i) {
        const auto *id = collection->absent_ids[i];
        if (!id || strnlen(id, 18) != 17 || !plan->kinds.count(id) ||
            (plan->kinds.at(id) != 1 && plan->kinds.at(id) != 16) ||
            !seen.insert(id).second)
          return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
        absent.insert(id);
      }
      if (seen.size() != plan->kinds.size())
        return E_NOTIMPL;
      if (plan->entitled && owned.size() != plan->coins.size())
        return E_NOTIMPL;
    }
    auto page = std::make_unique<PageOwner>();
    const auto count = plan->coins.size();
    size_t product_count = count;
    if (plan->entitled) {
      product_count = 0;
      for (size_t i = 0; i < count; ++i)
        if (!i || plan->coins[i].id != plan->coins[i-1].id) ++product_count;
    }
    page->products.resize(product_count);
    page->skus.resize(count);
    page->offers.resize(count);
    page->images.resize(count);
    page->sku_images.resize(count);
    page->keywords.resize(count);
    size_t product_index = 0;
    for (size_t i = 0; i < count; ++i) {
      const auto &c = plan->coins[i];
      const auto key = c.id + "/" + c.sku;
      bool first = !plan->entitled || !i || c.id != plan->coins[i-1].id;
      if (i && first) ++product_index;
      auto &product = page->products[product_index];
      auto &sku = page->skus[i];
      if (first) {
      product.storeId = page->keep(c.id);
      product.title = page->keep(c.title);
      product.description = page->keep(c.description);
      product.language = page->keep(c.language);
      product.inAppOfferToken = page->keep(c.offer_token);
      product.linkUri = page->keep_mutable(
          "https://www.microsoft.com/store/productId/" + c.id);
      product.productKind = static_cast<XStoreProductKind>(plan->kinds.at(key));
      product.isInUserCollection = owned.count(key) != 0;
      product.skusCount = 0;
      product.skus = &sku;
      page->copy_images(c.images, &page->images[i]);
      product.imagesCount = page->images[i].size();
      product.images = page->images[i].data();
      for (const auto &keyword : c.keywords)
        page->keywords[i].push_back(page->keep(keyword));
      product.keywordsCount = page->keywords[i].size();
      product.keywords = page->keywords[i].data();
      product.price.currencyCode = page->keep("");
      }
      ++product.skusCount;
      product.hasDigitalDownload = product.hasDigitalDownload || c.download;
      sku.skuId = page->keep(c.sku);
      sku.title = page->keep(c.sku_title);
      sku.description = page->keep(c.sku_description);
      sku.language = page->keep(c.sku_language);
      sku.isInUserCollection = product.isInUserCollection;
      page->copy_images(c.sku_images, &page->sku_images[i]);
      sku.imagesCount = page->sku_images[i].size();
      sku.images = page->sku_images[i].data();
      HRESULT hr = S_OK;
      sku.price.currencyCode = page->keep("");
      if (!c.offers.empty()) {
        hr = page->price(c.offers[0], &sku.price);
        if (FAILED(hr)) return hr;
        if (first) product.price = sku.price;
      }
      if (sku.isInUserCollection) {
        const auto &data = *owned.at(key);
        auto &v = sku.collectionData;
        v.acquiredDate = data.acquired_date;
        v.startDate = data.start_date;
        v.endDate = data.end_date;
        v.quantity = data.quantity;
        v.isTrial = data.is_trial;
        v.trialTimeRemainingInSeconds = data.trial_seconds;
        v.campaignId =
            data.campaign_id ? page->keep(data.campaign_id) : nullptr;
        v.developerOfferId = data.developer_offer_id
                                 ? page->keep(data.developer_offer_id)
                                 : nullptr;
      }
      page->offers[i].resize(c.offers.size());
      for (size_t a = 0; a < c.offers.size(); ++a) {
        auto &dest = page->offers[i][a];
        dest.availabilityId = page->keep(c.offers[a].id);
        dest.endDate = c.offers[a].end;
        hr = page->price(c.offers[a], &dest.price);
        if (FAILED(hr))
          return hr;
      }
      sku.availabilitiesCount = page->offers[i].size();
      sku.availabilities = page->offers[i].data();
    }
    page->page.structure_size = sizeof(page->page);
    page->page.product_count = product_count;
    page->page.products = page->products.data();
    page->page.continuation = continuation && *continuation ? page->keep(continuation) : nullptr;
    auto result = &page->page;
    {
      std::lock_guard<std::mutex> lock(pages_mutex);
      pages.emplace(result, std::move(page));
    }
    *out = result;
    return S_OK;
  } catch (const std::bad_alloc &) {
    return E_OUTOFMEMORY;
  } catch (...) {
    return HRESULT_FROM_WIN32(ERROR_INVALID_DATA);
  }
}
bool release_coin_page(XodusStoreProductPage *page) {
  std::unique_ptr<PageOwner> owned;
  {
    std::lock_guard<std::mutex> lock(pages_mutex);
    auto it = pages.find(page);
    if (it == pages.end())
      return false;
    owned = std::move(it->second);
    pages.erase(it);
  }
  return true;
}
} // namespace xodus_catalog

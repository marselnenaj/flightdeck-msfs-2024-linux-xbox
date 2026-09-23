// SPDX-License-Identifier: LGPL-2.1-or-later
#pragma once
#include <string>
#include <vector>
#include <windows.h>

namespace xodus_catalog {
// Anonymous catalog facts only. Deliberately no ownership/collection flags,
// balance, title-association decision, or personalized purchase availability.
struct Image {
  std::string uri, caption, purpose;
  UINT32 width = 0, height = 0;
};
struct Text {
  std::string language, title, description;
  std::vector<std::string> markets;
  std::vector<Image> images;
  std::vector<std::string> keywords;
};
struct Price {
  double list_price = 0, base_price = 0;
  std::string currency;
};
struct Availability {
  std::string id, start_date, end_date;
  std::vector<std::string> actions, markets, platforms;
  bool has_price = false;
  Price price;
};
struct Sku {
  std::string id;
  std::vector<Text> localized;
  std::vector<Availability> availabilities;
};
struct Product {
  std::string id, offer_token;
  UINT32 kind = 0;
  std::vector<Text> localized;
  std::vector<Sku> skus;
  // Exact public response bytes only when requested. No invented GDK JSON API.
  std::string raw_json;
};
HRESULT parse(const std::string &json, const std::string &expected_id,
              bool keep_raw, Product *out);
HRESULT fetch(const std::string &id, const std::string &market,
              const std::string &language, bool keep_raw,
              volatile LONG *cancelled, Product *out);
HRESULT fetch_until(const std::string &id, const std::string &market,
                    const std::string &language, bool keep_raw,
                    volatile LONG *cancelled, Product *out, ULONGLONG deadline);
} // namespace xodus_catalog

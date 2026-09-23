// SPDX-License-Identifier: LGPL-2.1-or-later
// Public DisplayCatalog reader. Never a substitute for Store collection data.
#include "StoreCatalog.h"
#include <algorithm>
#include <cmath>
#include <set>
#include <stdexcept>
#include <vendor/nlohmann_json.hpp>
#include <winhttp.h>

namespace xodus_catalog {
namespace {
using Json = nlohmann::json;
constexpr size_t max_document = 4 * 1024 * 1024, max_text = 65536;
HRESULT invalid() { return HRESULT_FROM_WIN32(ERROR_INVALID_DATA); }
HRESULT last_error() {
  DWORD e = GetLastError();
  return e ? HRESULT_FROM_WIN32(e) : E_FAIL;
}
bool upper_or_digit(char c) {
  return (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9');
}
bool product_id(const std::string &s) {
  return s.size() == 12 && std::all_of(s.begin(), s.end(), upper_or_digit);
}
bool is_cancelled(volatile LONG *cancel) {
  return cancel && InterlockedCompareExchange(cancel, 0, 0);
}
std::string text(const Json &value, size_t max = max_text) {
  if (!value.is_string())
    throw std::invalid_argument("string");
  auto s = value.get<std::string>();
  if (s.size() > max || s.find('\0') != std::string::npos ||
      (!s.empty() &&
       !MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, s.data(),
                            static_cast<int>(s.size()), nullptr, 0)))
    throw std::invalid_argument("text");
  return s;
}
std::vector<std::string> strings(const Json &a, size_t limit = 512) {
  if (!a.is_array() || a.size() > limit)
    throw std::invalid_argument("array");
  std::vector<std::string> result;
  for (const auto &v : a)
    result.push_back(text(v, 128));
  return result;
}
std::vector<Text> localized(const Json &array, const char *title,
                            const char *description) {
  if (!array.is_array() || array.empty() || array.size() > 256)
    throw std::invalid_argument("locales");
  std::vector<Text> result;
  for (const auto &item : array) {
    Text entry;
    entry.language = text(item.at("Language"), 64);
    entry.title = text(item.at(title));
    entry.description = text(item.at(description));
    entry.markets = strings(item.at("Markets"));
    if (item.count("Images") && !item.at("Images").is_null()) {
      const auto &images = item.at("Images");
      if (!images.is_array() || images.size() > 128)
        throw std::invalid_argument("images");
      for (const auto &raw : images) {
        Image image;
        image.uri = text(raw.at("Uri"), 2048);
        if (image.uri.rfind("//", 0) == 0)
          image.uri.insert(0, "https:");
        if (image.uri.rfind("https://", 0) != 0 ||
            image.uri.size() <= 8 ||
            std::any_of(image.uri.begin(), image.uri.end(), [](unsigned char c) {
              return c <= 32 || c == 127 || c == '\\';
            }))
          throw std::invalid_argument("image uri");
        const auto &width = raw.at("Width"), &height = raw.at("Height");
        if (!width.is_number_unsigned() || !height.is_number_unsigned() ||
            width.get<UINT64>() == 0 || height.get<UINT64>() == 0 ||
            width.get<UINT64>() > 16384 || height.get<UINT64>() > 16384)
          throw std::invalid_argument("image size");
        image.width = width.get<UINT32>();
        image.height = height.get<UINT32>();
        if (raw.count("Caption") && !raw.at("Caption").is_null())
          image.caption = text(raw.at("Caption"), 1024);
        image.purpose = text(raw.at("ImagePurpose"), 128);
        entry.images.push_back(std::move(image));
      }
    }
    if (item.count("SearchTitles") && !item.at("SearchTitles").is_null()) {
      const auto &titles = item.at("SearchTitles");
      if (!titles.is_array() || titles.size() > 128)
        throw std::invalid_argument("search titles");
      for (const auto &raw : titles)
        entry.keywords.push_back(text(raw.at("SearchTitleString"), 256));
    }
    result.push_back(std::move(entry));
  }
  return result;
}
double amount(const Json &n) {
  if (!n.is_number())
    throw std::invalid_argument("amount");
  double value = n.get<double>();
  if (!std::isfinite(value) || value < 0)
    throw std::invalid_argument("amount");
  return value;
}
Json document(const std::string &bytes) {
  std::vector<std::set<std::string>> keys;
  auto callback = [&](int depth, Json::parse_event_t event, Json &value) {
    if (depth > 64)
      throw std::invalid_argument("depth");
    if (event == Json::parse_event_t::object_start)
      keys.emplace_back();
    else if (event == Json::parse_event_t::object_end)
      keys.pop_back();
    else if (event == Json::parse_event_t::key &&
             !keys.back().insert(value.get<std::string>()).second)
      throw std::invalid_argument("duplicate");
    return true;
  };
  return Json::parse(bytes, callback);
}
struct Internet {
  HINTERNET value;
  explicit Internet(HINTERNET h) : value(h) {}
  ~Internet() {
    if (value)
      WinHttpCloseHandle(value);
  }
  Internet(const Internet &) = delete;
};
} // namespace
HRESULT parse(const std::string &bytes, const std::string &expected_id,
              bool keep_raw, Product *out) {
  if (!out)
    return E_POINTER;
  *out = {};
  if (!product_id(expected_id) || bytes.empty() || bytes.size() > max_document)
    return E_INVALIDARG;
  try {
    const auto root = document(bytes);
    const auto &p = root.at("Product");
    Product result;
    result.id = text(p.at("ProductId"), 12);
    if (result.id != expected_id)
      return invalid();
    const auto kind = text(p.at("ProductKind"), 32);
    result.kind = kind == "Consumable"            ? 1
                  : kind == "Durable"             ? 2
                  : kind == "Game"                ? 4
                  : kind == "Pass"                ? 8
                  : kind == "UnmanagedConsumable" ? 16
                                                  : 0;
    if (!result.kind)
      return E_NOTIMPL;
    result.localized = localized(p.at("LocalizedProperties"), "ProductTitle",
                                 "ProductDescription");
    const auto &props = p.at("Properties");
    if (props.count("InAppOfferToken") &&
        !props.at("InAppOfferToken").is_null())
      result.offer_token = text(props.at("InAppOfferToken"));
    const auto &skus = p.at("DisplaySkuAvailabilities");
    if (!skus.is_array() || skus.empty() || skus.size() > 256)
      return invalid();
    std::set<std::string> seen_skus;
    for (const auto &display : skus) {
      const auto &s = display.at("Sku");
      if (text(s.at("ProductId"), 12) != result.id)
        return invalid();
      Sku sku;
      sku.id = text(s.at("SkuId"), 4);
      if (sku.id.size() != 4 ||
          !std::all_of(sku.id.begin(), sku.id.end(), upper_or_digit) ||
          !seen_skus.insert(sku.id).second)
        return invalid();
      sku.localized =
          localized(s.at("LocalizedProperties"), "SkuTitle", "SkuDescription");
      const auto &avails = display.at("Availabilities");
      if (!avails.is_array() || avails.size() > 512)
        return invalid();
      std::set<std::string> seen_avails;
      for (const auto &a : avails) {
        Availability avail;
        avail.id = text(a.at("AvailabilityId"), 128);
        if (avail.id.empty() || !seen_avails.insert(avail.id).second ||
            text(a.at("SkuId"), 4) != sku.id)
          return invalid();
        avail.actions = strings(a.at("Actions"), 64);
        avail.markets = strings(a.at("Markets"));
        const auto &conditions = a.at("Conditions");
        avail.start_date = text(conditions.at("StartDate"), 64);
        avail.end_date = text(conditions.at("EndDate"), 64);
        if (conditions.count("ClientConditions") &&
            !conditions.at("ClientConditions").is_null()) {
          const auto &client = conditions.at("ClientConditions");
          if (client.count("AllowedPlatforms") &&
              !client.at("AllowedPlatforms").is_null()) {
            const auto &platforms = client.at("AllowedPlatforms");
            if (!platforms.is_array() || platforms.size() > 64)
              return invalid();
            for (const auto &platform : platforms)
              avail.platforms.push_back(text(platform.at("PlatformName"), 128));
          }
        }
        if (a.count("OrderManagementData") &&
            !a.at("OrderManagementData").is_null()) {
          const auto &omd = a.at("OrderManagementData");
          if (omd.count("Price") && !omd.at("Price").is_null()) {
            const auto &price = omd.at("Price");
            avail.price.list_price = amount(price.at("ListPrice"));
            avail.price.base_price = amount(price.at("MSRP"));
            avail.price.currency = text(price.at("CurrencyCode"), 3);
            if (avail.price.currency.size() != 3 ||
                !std::all_of(avail.price.currency.begin(),
                             avail.price.currency.end(),
                             [](char c) { return c >= 'A' && c <= 'Z'; }))
              return invalid();
            avail.has_price = true;
          }
        }
        sku.availabilities.push_back(std::move(avail));
      }
      result.skus.push_back(std::move(sku));
    }
    if (keep_raw)
      result.raw_json = bytes;
    *out = std::move(result);
    return S_OK;
  } catch (const std::bad_alloc &) {
    return E_OUTOFMEMORY;
  } catch (...) {
    return invalid();
  }
}
HRESULT fetch_until(const std::string &id, const std::string &market,
                    const std::string &language, bool keep_raw,
                    volatile LONG *cancel, Product *out, ULONGLONG deadline) {
  if (!out)
    return E_POINTER;
  *out = {};
  if (!product_id(id) || market.size() != 2 ||
      !std::all_of(market.begin(), market.end(),
                   [](char c) { return c >= 'A' && c <= 'Z'; }) ||
      language.empty() || language.size() > 64 ||
      !std::all_of(language.begin(), language.end(), [](char c) {
        return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') ||
               (c >= '0' && c <= '9') || c == '-';
      }))
    return E_INVALIDARG;
  if (is_cancelled(cancel))
    return E_ABORT;
  try {
    const ULONGLONG now = GetTickCount64();
    if (!deadline)
      deadline = now + 30000;
    else
      deadline = std::min(deadline, now + 30000);
    if (now >= deadline)
      return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
    Internet session(WinHttpOpen(
        L"Xodus-Public-Catalog/1", WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
        WINHTTP_NO_PROXY_NAME, WINHTTP_NO_PROXY_BYPASS, 0));
    if (!session.value)
      return last_error();
    const int timeout =
        static_cast<int>(std::min<ULONGLONG>(5000, deadline - now));
    if (!WinHttpSetTimeouts(session.value, timeout, timeout, timeout, timeout))
      return last_error();
    DWORD tls = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2;
    if (!WinHttpSetOption(session.value, WINHTTP_OPTION_SECURE_PROTOCOLS, &tls,
                          sizeof(tls)))
      return last_error();
    Internet connection(WinHttpConnect(
        session.value, L"displaycatalog.mp.microsoft.com", 443, 0));
    if (!connection.value)
      return last_error();
    const std::string path =
        "/v7.0/products/" + id + "?market=" + market + "&languages=" + language;
    const std::wstring wide(path.begin(), path.end());
    Internet request(WinHttpOpenRequest(
        connection.value, L"GET", wide.c_str(), nullptr, WINHTTP_NO_REFERER,
        WINHTTP_DEFAULT_ACCEPT_TYPES, WINHTTP_FLAG_SECURE));
    if (!request.value)
      return last_error();
    DWORD redirect = WINHTTP_OPTION_REDIRECT_POLICY_NEVER,
          disable = WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION;
    if (!WinHttpSetOption(request.value, WINHTTP_OPTION_REDIRECT_POLICY,
                          &redirect, sizeof(redirect)) ||
        !WinHttpSetOption(request.value, WINHTTP_OPTION_DISABLE_FEATURE,
                          &disable, sizeof(disable)))
      return last_error();
    if (is_cancelled(cancel))
      return E_ABORT;
    if (GetTickCount64() >= deadline)
      return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
    if (!WinHttpSendRequest(request.value, WINHTTP_NO_ADDITIONAL_HEADERS, 0,
                            WINHTTP_NO_REQUEST_DATA, 0, 0, 0))
      return last_error();
    if (is_cancelled(cancel))
      return E_ABORT;
    if (GetTickCount64() >= deadline)
      return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
    if (!WinHttpReceiveResponse(request.value, nullptr))
      return last_error();
    DWORD status = 0, size = sizeof(status);
    if (!WinHttpQueryHeaders(request.value,
                             WINHTTP_QUERY_STATUS_CODE |
                                 WINHTTP_QUERY_FLAG_NUMBER,
                             WINHTTP_HEADER_NAME_BY_INDEX, &status, &size,
                             WINHTTP_NO_HEADER_INDEX))
      return last_error();
    if (status != 200)
      return HRESULT_FROM_WIN32(ERROR_WINHTTP_INVALID_SERVER_RESPONSE);
    std::string bytes;
    char buffer[8192];
    for (;;) {
      if (is_cancelled(cancel))
        return E_ABORT;
      if (GetTickCount64() >= deadline)
        return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
      DWORD read = 0;
      if (!WinHttpReadData(request.value, buffer, sizeof(buffer), &read))
        return last_error();
      if (!read)
        break;
      if (bytes.size() + read > max_document)
        return HRESULT_FROM_WIN32(ERROR_FILE_TOO_LARGE);
      bytes.append(buffer, read);
    }
    Product result;
    HRESULT hr = parse(bytes, id, keep_raw, &result);
    if (is_cancelled(cancel))
      return E_ABORT;
    if (SUCCEEDED(hr))
      *out = std::move(result);
    return hr;
  } catch (const std::bad_alloc &) {
    return E_OUTOFMEMORY;
  } catch (...) {
    return E_FAIL;
  }
}
HRESULT fetch(const std::string &id, const std::string &market,
              const std::string &language, bool keep_raw, volatile LONG *cancel,
              Product *out) {
  return fetch_until(id, market, language, keep_raw, cancel, out, 0);
}
} // namespace xodus_catalog

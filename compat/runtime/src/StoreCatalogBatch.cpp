// SPDX-License-Identifier: LGPL-2.1-or-later
#include "StoreCatalogBatch.h"
#include <algorithm>
#include <atomic>
#include <map>
#include <mutex>
#include <set>
#include <thread>

namespace xodus_catalog {
struct CatalogReader::State {
  FetchProduct fetch;
  struct Cached {
    std::string bytes;
    ULONGLONG expires;
  };
  std::mutex mutex;
  std::map<std::string, Cached> cache;
  size_t cache_bytes = 0;
  explicit State(FetchProduct source) : fetch(source) {}
  std::string get(const std::string &key) {
    std::lock_guard<std::mutex> lock(mutex);
    auto it = cache.find(key);
    if (it == cache.end())
      return {};
    if (it->second.expires <= GetTickCount64()) {
      cache_bytes -= it->second.bytes.size();
      cache.erase(it);
      return {};
    }
    return it->second.bytes;
  }
  void put(const std::string &key, const std::string &bytes) {
    if (bytes.empty() || bytes.size() > 4 * 1024 * 1024)
      return;
    std::lock_guard<std::mutex> lock(mutex);
    auto old = cache.find(key);
    if (old != cache.end()) {
      cache_bytes -= old->second.bytes.size();
      cache.erase(old);
    }
    while (!cache.empty() && (cache.size() >= 256 ||
                              cache_bytes + bytes.size() > 16 * 1024 * 1024)) {
      cache_bytes -= cache.begin()->second.bytes.size();
      cache.erase(cache.begin());
    }
    cache.emplace(key, Cached{bytes, GetTickCount64() + 60000});
    cache_bytes += bytes.size();
  }
};
CatalogReader::CatalogReader(FetchProduct fetch)
    : state(std::make_unique<State>(fetch)) {}
CatalogReader::~CatalogReader() = default;
HRESULT CatalogReader::read(const std::vector<std::string> &ids,
                            const std::string &market,
                            const std::string &language,
                            volatile LONG *cancelled, std::vector<Product> *out,
                            ULONGLONG deadline, bool keep_raw) {
  if (!out)
    return E_POINTER;
  out->clear();
  if (ids.empty() || ids.size() > 100 || !state->fetch)
    return E_INVALIDARG;
  if (cancelled && InterlockedCompareExchange(cancelled, 0, 0))
    return E_ABORT;
  if (!deadline)
    deadline = GetTickCount64() + 30000;
  if (GetTickCount64() >= deadline)
    return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
  try {
    std::vector<std::string> unique;
    std::set<std::string> seen;
    for (const auto &id : ids) {
      if (id.size() != 12 || !std::all_of(id.begin(), id.end(), [](char c) {
            return (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9');
          }))
        return E_INVALIDARG;
      if (seen.insert(id).second)
        unique.push_back(id);
    }
    std::vector<Product> products(unique.size());
    std::atomic<size_t> next{0}, bytes{0};
    std::atomic<unsigned> active{0};
    std::atomic<HRESULT> failure{S_OK};
    volatile LONG stop = 0;
    std::vector<std::thread> workers;
    auto fail = [&](HRESULT hr) {
      HRESULT previous = S_OK;
      failure.compare_exchange_strong(previous, hr);
      InterlockedExchange(&stop, 1);
    };
    auto work = [&]() {
      try {
        for (;;) {
          if (InterlockedCompareExchange(&stop, 0, 0))
            break;
          if (GetTickCount64() >= deadline) {
            fail(HRESULT_FROM_WIN32(ERROR_TIMEOUT));
            break;
          }
          size_t index = next.fetch_add(1);
          if (index >= unique.size())
            break;
          const auto &id = unique[index];
          const auto key = market + "/" + language + "/" + id;
          auto cached = state->get(key);
          Product value;
          HRESULT hr;
          if (!cached.empty())
            hr = parse(cached, id, true, &value);
          else
            hr = state->fetch(id, market, language, true, &stop, &value,
                              deadline);
          if (FAILED(hr)) {
            fail(hr);
            break;
          }
          if (value.id != id || value.raw_json.empty()) {
            fail(HRESULT_FROM_WIN32(ERROR_INVALID_DATA));
            break;
          }
          if (bytes.fetch_add(value.raw_json.size()) + value.raw_json.size() >
              32 * 1024 * 1024) {
            fail(HRESULT_FROM_WIN32(ERROR_FILE_TOO_LARGE));
            break;
          }
          if (cached.empty())
            state->put(key, value.raw_json);
          if (!keep_raw)
            value.raw_json.clear();
          products[index] = std::move(value);
        }
      } catch (const std::bad_alloc &) {
        fail(E_OUTOFMEMORY);
      } catch (...) {
        fail(E_FAIL);
      }
      --active;
    };
    try {
      for (size_t i = 0; i < std::min<size_t>(4, unique.size()); ++i) {
        ++active;
        try {
          workers.emplace_back(work);
        } catch (...) {
          --active;
          throw;
        }
      }
    } catch (...) {
      fail(E_OUTOFMEMORY);
      for (auto &worker : workers)
        worker.join();
      return failure.load();
    }
    while (active.load()) {
      if (cancelled && InterlockedCompareExchange(cancelled, 0, 0))
        fail(E_ABORT);
      if (GetTickCount64() >= deadline)
        fail(HRESULT_FROM_WIN32(ERROR_TIMEOUT));
      Sleep(2);
    }
    for (auto &worker : workers)
      worker.join();
    if (cancelled && InterlockedCompareExchange(cancelled, 0, 0))
      return E_ABORT;
    if (FAILED(failure.load()))
      return failure.load();
    if (GetTickCount64() >= deadline)
      return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
    *out = std::move(products);
    return S_OK;
  } catch (const std::bad_alloc &) {
    return E_OUTOFMEMORY;
  } catch (...) {
    return E_FAIL;
  }
}
} // namespace xodus_catalog

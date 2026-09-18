/* SPDX-License-Identifier: LGPL-2.1-or-later */
#pragma once
#include <windows.h>
#include <atomic>
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

// This backend is an explicitly separate local branch. It neither reads nor
// claims to represent Microsoft's cloud save inventory. No network code exists.
namespace local_save {
using Handle = void*;
constexpr HRESULT invalid_name = (HRESULT)0x80830001;
constexpr HRESULT storage_full = (HRESULT)0x80830003;
constexpr HRESULT update_too_big = (HRESULT)0x80830005;
constexpr HRESULT quota_exceeded = (HRESULT)0x80830006;
constexpr HRESULT buffer_too_small = (HRESULT)0x80830007;
constexpr HRESULT blob_not_found = (HRESULT)0x80830008;
constexpr HRESULT handle_expired = (HRESULT)0x8083000d;
constexpr uint64_t default_quota = 256ull * 1024 * 1024;
constexpr uint64_t max_update_bytes = 16ull * 1024 * 1024;

struct Options {
    bool enabled = false;
    // Must already be an absolute, caller-selected private directory. The core
    // creates only its hashed namespace child and files, never parent paths.
    std::wstring root;
    // SHA-256 of versioned title + SCID + actual user ID (or explicit NULL user).
    // The bridge, not the core, authenticates/derives that binding.
    std::string namespace_key;
    uint64_t quota = default_quota;
};
struct Cancellation { std::atomic<bool> cancelled{false}; };
struct Blob { std::string name; std::vector<uint8_t> data; };
struct ContainerInfo {
    std::string name, display_name;
    uint32_t blob_count = 0;
    uint64_t total_size = 0;
    int64_t last_modified = 0;
};
struct BlobInfo { std::string name; uint32_t size; };

HRESULT initialize(const Options&, bool sync_on_demand, Handle*);
void close_provider(Handle);
HRESULT remaining_quota(Handle, int64_t*);
struct PendingMutation;
// Async wrappers register queued mutations before exposing successful Begin.
// A quota barrier includes earlier submissions, never later queued operations.
HRESULT register_mutation(Handle,std::shared_ptr<PendingMutation>*);
HRESULT quota_barrier(Handle,uint64_t*);
HRESULT remaining_quota_at(Handle,uint64_t,int64_t*);
HRESULT delete_container(Handle, const char*, const Cancellation* = nullptr);
HRESULT container_info(Handle, const char* exact_or_prefix, bool exact,
                       std::vector<ContainerInfo>*);
HRESULT create_container(Handle, const char*, Handle*);
void close_container(Handle);
HRESULT blob_info(Handle, const char* prefix, std::vector<BlobInfo>*);
HRESULT read_blobs(Handle, const std::vector<std::string>* names,
                   std::vector<Blob>*, const Cancellation* = nullptr);
HRESULT create_update(Handle, const char* display_name, Handle*);
void close_update(Handle);
HRESULT write_blob(Handle, const char*, const uint8_t*, size_t);
HRESULT delete_blob(Handle, const char*);
HRESULT submit_update(Handle, const Cancellation* = nullptr);
// Terminal: invalidates all handles; in-flight I/O holds storage alive until it
// returns. Never waits on an async worker or executes a callback under our lock.
void shutdown();
bool is_open(Handle);

// Async wrappers retain this guard so closing a public handle cannot release
// its backing storage while a queued operation still owns the reference.
struct Retained;
std::shared_ptr<Retained> retain(Handle);
bool is_open(const std::shared_ptr<Retained>&);

#ifdef XODUS_GAMESAVE_TESTING
// Compiled only into synthetic probes. 1=fail before rename, 2=cancel there.
void test_commit_fault(int);
void test_commit_barrier(HANDLE entered,HANDLE resume);
size_t test_live_handles();
#endif
}

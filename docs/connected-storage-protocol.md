# ConnectedStorage protocol evidence

Flightdeck uses an independently written compatibility implementation. The
reference components described here are not redistributed or executed by
Flightdeck. This document records wire-format and state-transition observations;
it does not contain Microsoft source code or disassembly.

## Reference and reproduction

Microsoft's `XblGameSave.dll` version `10.0.26100.8115`, x64, was obtained from
[Microsoft's public symbol server](https://msdl.microsoft.com/download/symbols/XblGameSave.dll/781F83B7d0000/XblGameSave.dll).
Its SHA-256 is
`9d2eacbc8fd2e43fdd2b680804a7aca3da5f22fc98ef9436601d5aa8c159bfd3`.
The DLL's CodeView record identifies `XblGameSave.pdb`, GUID
`37F22A6C-FA70-6539-A319-4F81FFF785BB`, age 1. The corresponding
[public Microsoft PDB](https://msdl.microsoft.com/download/symbols/XblGameSave.pdb/37F22A6CFA706539A3194F81FFF785BB1/XblGameSave.pdb)
has SHA-256
`d3f12a495febd700af3d1db7e6d3a572bc53ab914cc491c7cae0e713af6545d1`.
The public PDB itself reports age 3; its GUID and the mapped public symbols
match the downloaded DLL. Do not mistake this stripped-PDB property for a
separate DLL version.

The observations can be reproduced with `llvm-pdbutil dump --publics` and
`llvm-objdump -d --x86-asm-syntax=intel`. Addresses below are RVAs relative to
image base `0x180000000`. The private research workspace records the download
URLs, hashes and extraction tools. No credentials or save contents are needed.

## Normal lock and owner extension

`NtmWebService::AcquireLockImpl` (`0x3f1ec`) uses the scope's `/lock` endpoint.
Its Boolean argument adds `?breakLock=true` only when true. The false branch
has no takeover query. Flightdeck never offers a force-lock operation.

`MakeHeadersWithContext` (`0x420d4`) appends the actual package-family header
and constructs `x-xbl-lock-ext` as:

```text
Windows.System.User.NonRoamableId + "_" + actual XUID
```

The provenance of that first field is visible in
`UserManager::GetUserInfoFromUserContextToken` (`0x27e30`) and
`UserManager::GetUserInfo` (`0x27c6c`), through `get_NonRoamableId`, followed by
`ContextDesc` construction (`0x1fd4c`). There is no Base64 or cryptographic
transformation in this header builder. The second field is independently
confirmed by `MakeUrl` (`0x42278`) using it in `users/xuid(...)`.

[Microsoft documents NonRoamableId](https://learn.microsoft.com/en-us/uwp/api/windows.system.user.nonroamableid?view=winrt-26100)
as a local identifier tied to device, app and user. A private persistent Linux
client identifier is a compatibility projection of this local role, not a
Windows identity or cloud credential. Its acceptance must be verified with a
normal acquire/reacquire/release round trip before enabling save writes; the
server's allowed identifier alphabet is not established by this static analysis.

## Acquisition and renewal fencing

The acquire response callback (`0x3e18c`) distinguishes:

| HTTP | Reference status | Meaning used by the caller |
| --- | --- | --- |
| 200 | 4 | Existing ownership retained |
| 201 | 5 | New ownership acquired |
| 409 | 6 | Lock conflict/loss |
| 403 | 17 | Authorization rejected |

`ParseAcquireLockResponse` (`0x42904`) reads **`ownerChangeId`**, with this exact
spelling, as a required JSON string. It also reads numeric `quotaBytes`, using
256 MiB when that optional value is absent. Some older public notes misspell
the owner field; Flightdeck must not use that spelling as a fallback.

`Context::ReacquireLock` (`0x5addc`) calls the same operation with the force flag
false. `AfterReacquireLockImpl` (`0x58a2c`) resumes pending uploads only for
status 4/HTTP 200. A newly acquired lock (201) or conflict (409) disables further
uploads; 403 also disables them. A fresh ownership generation is therefore not
silently treated as continuation of an earlier transaction.

Flightdeck additionally compares every successful renewal's owner-change string
with the acquisition response. The reference stores this value with its local
context/index and uses it when deciding whether cached content remains valid.
It is **not** observed as an `If-Match` header or request-body field.

No lease lifetime was inferred from request timeouts. The observed reacquire
triggers include network reconnection (`0x59ff8`) and a retry after more than
300,000 ms in an uncertain lock state (`Contexts::DoWork`, `0x2ee3c`). That
retry interval is not a documented server lease expiry.

## Commit, conflict and release

`UploadContainerImpl` (`0x45844`) uses the scope's container route, a
`clientFileTime` query value, optional escaped `displayName`, JSON content type,
and the same owner-extension headers. No explicit `If-Match` is constructed.
The reference's `UploadContainerComplete` (`0x453f0`) maps HTTP 409 to lock-loss
status 6 and reads a response ETag after success. Its 401 retry is bounded and
refreshes authentication; it does not force ownership.

That status propagates through container completion to `Context::UploadFailed`
(`0x5c250`), which disables uploads. Container atomicity is not whole-library
atomicity. A multi-container operation can stop after earlier containers have
already committed.

`ReleaseLock` (`0x431a0`) uses `/lock` with the same context headers and no forced
break. Its response callback (`0x3ea48`) treats 200, 204, 403 and 409 as a finished
release attempt. Flightdeck only reports a positively confirmed release for
200/204; rejection or a lost response remains visible as uncertain cleanup.

Flightdeck's coordinator requires a freshly read remote-content digest to match
the user's reviewed plan after acquiring the lock. It checks ownership before
commits, stops on conflict, and reads content back after a possibly committed
request instead of retrying it blindly. Only a complete exact readback can
create a common-baseline receipt. Cancellation or failure after a commit attempt
reports that recovery is required and retains the local source and backups.

Public read-route and atom-format cross-checks include
[Xodus's pinned notes](https://github.com/xodus-gaming/xodus/blob/0670e25aeb0e0e9f800f8f2f4968ae3b681842a7/docs/xbox/titlestorage.md)
and the original implementations linked in `flightdeck/cloud_storage.py`.
They do not replace the owner-fencing evidence above. The native write adapter
must separately validate its atom-allocation, byte-upload and atom-commit flow
before any production write is enabled.

## Live verification

On 18 September 2026, a real MSFS profile accepted the persistent Linux client
identifier through normal acquisition, repeated ownership checks and release.
A separate 32-byte synthetic container then exercised atom allocation, byte
upload, atom commit, container commit, exact readback and deletion. All steps
succeeded without forcing the lock. The test container was absent afterward;
the original 18 containers' metadata, atom identities and bytes, and the active
local saves, remained unchanged. Private account and save data are not included
in this repository. This verifies the native wire path; a cross-device gameplay
test remains outstanding.

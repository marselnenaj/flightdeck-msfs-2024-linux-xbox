# Marketplace integration

The compatibility layer joins public catalog products with account-bound
Collections results. A real simulator test returned 250 consumable product/SKU
entries. A user subsequently downloaded free content successfully through the
in-game Store. Direct and satisfying account entitlements can now be enumerated
for the running game. Paid checkout and device-shared DLC licenses are not implemented.
Catalog entries alone do not establish ownership.

In another free-item test, the in-game screen briefly showed an expired
license after downloading; choosing **Load** again cleared it. This is one
observation, not evidence that license refresh is reliable. The receipt broker
now retries a genuine signed response at most twice when an explicitly requested
product has only expired grants. It waits 350 ms and 1 second, preserves the exact
developer challenge, and returns an unmodified Microsoft-signed receipt. Missing
ownership does not trigger retries. The existing 35-second deadline and account
validation still apply. This addresses a renewal race; a real expired-download
case has not yet verified the change in the simulator.

In earlier local simulator traces, `XStoreQueryEntitledProductsAsync` for durable
products and `XStoreQueryGameAndDlcPackageUpdatesAsync` returned `E_NOTIMPL`.
The former now uses the authenticated account inventory described below. The
package-update operation now supports online checks of Flightdeck's single
registered base-game package, with the limits described below.
The paid purchase UI and package installation methods also return `E_NOTIMPL`
in the current bridge. These gaps can affect existing edition upgrades as
well as new purchases. A successful free-content download does not prove that
paid inventory or its licenses work.

The launcher's diagnostic report now summarizes Store method names and HRESULTs
from the latest game session, including final asynchronous product-query
outcomes. It omits product IDs, account data and raw log lines. The native
bridge separately records bounded product IDs for unsupported
purchase and durable-license attempts in the private game log, to distinguish
which operation failed during local testing. These observations do not change
the result returned to the game.

The native game-license request reads Flightdeck's configured Store market
instead of sending `AT` for every user. The implemented account inventory and
Durable-license paths are described below. Purchase dialogs, device-shared DLC
rights and package-install operations remain unsupported.

## Package checks and the MSFS 2020 disc prompt

`XStoreQueryGameAndDlcPackageUpdatesAsync` reads the installed version from the
game's title configuration, binds the public catalog's PC MSIXVC content ID to
the exact package family, and reads `GetBasePackage` using the current Store
account's Xbox update-service authentication. The broker validates the account
context before and after the request. Only a matching version with one complete,
available MSIXVC image returns success with zero updates. Missing packages,
ambiguous metadata, predownloads and transport errors never become empty success.
A differing version returns `HRESULT_FROM_WIN32(ERROR_REVISION_MISMATCH)`.

This path is enabled by the Flightdeck loader's explicit
`FlightdeckBaseGameOnlyV1` package scope. That loader mounts one Store MSIXVC;
Flightdeck does not yet install separate Store DLC packages. Simulator content
packs are distinct from those platform packages. Generic Wine clients without
this scope still receive `E_NOTIMPL`. Adding a Store DLC package installer must
replace this scope with a complete installed-package registry and update checks
for every registered package. Ownership alone cannot populate that registry.

The native count/result functions follow the zero-length XAsync contract,
including cleanup when a caller only reads the count. Cancellation, failed
checks and unknown scopes are covered by native tests. On 2026-09-23 the real
Windows API -> Wine -> broker -> Microsoft path successfully checked the
installed MSFS 2020 and MSFS 2024 versions in isolated prefixes. Both had zero
updates; removing the scope returned `E_NOTIMPL` as intended. A temporary
configuration reporting an older version returned `0x8007051a`, not empty success. The package
response does not establish mandatory-update policy, so this implementation
does not invent update entries or install newer packages. The launcher's
separate game-update workflow remains responsible for those downloads.

The user reported a dismissible disc prompt during MSFS 2020 startup. Its game
trace had successful game-license and Durable-inventory queries followed by the
formerly unsupported package-update query. A separate live probe confirmed an
active, non-trial digital game license (`isDiscLicense=false`). That identifies
a real missing startup operation; it does **not** yet prove that implementing
the operation removes the prompt. Microsoft describes the prompt as a general
[license-authentication error](https://flightsimulator.zendesk.com/hc/en-us/articles/360015985699--Please-insert-the-Microsoft-Flight-Simulator-Game-Disc-Error-message).
See also the documented [package-update flow](https://learn.microsoft.com/en-us/gaming/gdk/docs/store/commerce/fundamentals/xstore-checking-for-updates?view=gdk-2604).

## Durable license handles

`XStoreAcquireLicenseForDurablesAsync` now requests a genuine Microsoft-signed
grant for the exact product through the bound Store account. The broker verifies
the signature, challenge, dates, SKU, Durable product kind and current game's
`addOnParent` relationship. Public catalog or Collections ownership alone cannot
create a license handle. Trials, ambiguous SKUs and bundle SKUs remain unsupported.

The implementation supports online licenses for Durables with or without a
package. Observations expire no later than the signed grant, signed JWT or 60
seconds. Held handles renew before that deadline; a temporary failed read retains
only the still-current proof. Expired handles cannot revive. `XStoreIsLicenseValid`,
closing handles, and registering/unregistering `XStoreRegisterPackageLicenseLost`
callbacks implement the corresponding lifecycle, including cancellation, context
closure, queued callbacks and reentrant callback cleanup. Both wall-clock expiry
and a monotonic deadline apply. Offline/device-shared licensing and package
installation are not provided by this path.

On 2026-09-23 a native Windows API test in an isolated Wine prefix acquired a valid
license for an actually owned MSFS 2020 Durable with a package. Holding it for 65
seconds exercised multiple online renewals beyond the original 60-second proof;
it remained valid without a spurious loss callback. Closing the
Store context invalidated the handle and delivered the loss callback. Synthetic
tests separately exercise short signed lifetimes, failed renewal, cancellation,
malformed proofs, delayed result retrieval and callback lifetime. This does not
validate the MSFS 2024 Aviator Upgrade, which has a different product kind.
See Microsoft's [Durable licensing API](https://learn.microsoft.com/en-us/gaming/gdk/docs/reference/system/xstore/functions/xstoreacquirelicensefordurablesasync?view=gdk-2604).

Explicit `XStoreQueryProductsAsync` requests also accept a single-SKU Durable
without a separate package when the public catalog shows the current game's
add-on relationship and Collections supplies a positive, active entitlement.
An absent Durable remains unknown because the account-only Collections response
cannot cover device-shared rights; no unowned product is returned for it.
The separate enumeration path below handles positive direct/satisfying rights.

## Title inventory

`XStoreQueryEntitledProductsAsync` exhausts the authenticated Collections library
with `expandSatisfyingItems=true`, including every continuation page. It requires
an active parent-game control, validates all records, and then filters positive
entitlements by the exact catalog `addOnParent` relationship (or the current
game itself). Public purchase channels are never used to discover ownership.
Unrelated account products never cross the native inventory boundary.

Snapshots are bound to the account, title, application, market, kind mask and
page size. Opaque continuation tokens expire after at most 30 seconds; a new
query always reads current service data. Pages group a product's owned SKUs and
retain owned entries without a currently purchasable offer. Concurrent catalog
requests, total pages, record counts, response sizes and in-flight snapshots are
bounded. Authentication failures, timeouts, incomplete paging, conflicting
records or unavailable catalog metadata return errors, never empty success.

The current mapping supports non-trial, non-subscription products without bundled
SKUs. The service reports direct and satisfying account coverage explicitly;
device-shared rights are not covered. This is not full GDK Store parity.

The broker and native provider accept both GUID and legacy 16-character
hexadecimal Microsoft application IDs, using the exact value from each game's
configuration. MSFS 2020 uses the older format. See Microsoft's
[title identity configuration](https://learn.microsoft.com/en-us/gaming/gdk/docs/services/fundamentals/portal-config/live-setup-partner-center-partners).

Local validation on 2026-09-23 exercised the production broker and the native
Windows API in an isolated Wine prefix with the existing Store account. Durable
enumeration returned a genuine empty result instead of `E_NOTIMPL`; enumeration
across product kinds returned the licensed game with its two owned SKUs grouped
into one product. Foreign account contexts were rejected. Synthetic native tests
also cover positive Durables, withdrawn offers, paging ownership and expired
records. This account had no active Durable record, so the tests do not prove
that another user's paid Aviator upgrade is restored.
The same native test with MSFS 2020 returned one actual owned Durable after the
legacy application-ID fix. An all-kinds query for 2020 encountered an HTTP 404
for another library product's catalog metadata; it correctly returned an error
instead of silently dropping that entitlement. Such unavailable metadata remains
a limitation of the broader enumeration path.

The publicly listed Aviator Upgrade (`9MTWMVDJ01FF`) is classified as an
`UnmanagedConsumable`, not a Durable. The observed simulator trace queried
Durable inventory and a separate list of coin products; it did not establish
an explicit Aviator Upgrade query. The catalog classification and the
single-SKU Durable improvement alone therefore cannot demonstrate that an
existing Aviator purchase becomes available in-game.
The explicit catalog mapper now carries validated localized HTTPS images and
search terms as well as the product text. An anonymous, public Aviator catalog
entry parsed and produced the expected exact SKU (`0010`) in a local Wine
smoke test. This test did not query anyone's ownership or exercise checkout.

The consumer Collections endpoint is based on the experiment documented in
[Nexorious issue 752](https://github.com/drzero42/nexorious/issues/752#issuecomment-4652690721)
and its [pinned implementation notes](https://github.com/drzero42/nexorious/blob/b7658c0dabc17d305910757d657dc4151b6bca57/docs/sync.md#L433-L449),
with bounded read-only integration checks. It is not a Microsoft guarantee of
consumer API support. The public test suite uses synthetic fixtures and includes
no captured account responses.

## Supported queries

The first integration classifies exact catalog product/SKU pairs for the actual
broker Store account. Remote requests use a deduplicated product-ID filter;
the complete returned SKU set is then matched to each requested exact SKU.
Bounded probes found that mixing entries with and without the optional `skuId`
filter produced HTTP 400, while consistently product-wide filters with
`expandSatisfyingItems=true` succeeded. A product-wide query is not an
unfiltered whole-account query.

The integration requires complete pagination, direct and satisfying
entitlement coverage, and an active known-parent control in every HTTP batch.
Caps, repeated cursors, timeouts, malformed records or missing control evidence
produce an error. They must not become an empty successful result.

`XStoreQueryConsumableBalanceRemainingAsync` now handles a single-SKU,
Store-managed `Consumable` associated with the current game. It queries a fresh
exact product/SKU Collections snapshot through the bound Store account. A
positive record supplies the Store's remaining quantity; a complete, explicit
absence supplies zero. Unknown, paged, mismatched, developer-managed and
package-bearing products return an error instead of an invented balance. This
does not report the simulator's publisher-managed Simverse wallet.

Positive records must match the requested SKU and kind and have a valid active
time interval. Quantity and trial fields come from the service; conflicting
duplicate records are rejected. Responses expire within 30 seconds and no later
than the supporting authentication and entitlement evidence. The native caller
must retain an owned snapshot and reject expired or incomplete classifications.

For products genuinely cataloged as `Consumable` or `UnmanagedConsumable`,
Microsoft documents single-account purchases that are not shared through game
licensing. A complete query for the correct account can therefore support a
negative result for this narrow subset without a device-sharing backend.
[Product types and consumables](https://learn.microsoft.com/en-us/gaming/gdk/docs/store/commerce/getting-started/xstore-choosing-the-right-product-type?view=gdk-2604#consumables)

This describes the Store entitlement and its remaining quantity. A title can
already have fulfilled that entitlement and credited currency to its own
service. Store absence must not reset or imply a zero game-wallet balance.
On PC the Store account can also differ from the Xbox account playing the game;
an arbitrary game-user XSTS token is not a substitute for the selected Store
account. [PC account mismatch behavior](https://learn.microsoft.com/en-us/gaming/gdk/docs/store/commerce/pc-specific-considerations/xstore-handling-mismatched-store-accounts?view=gdk-2604)

Missing Game, Durable and Pass records remain unknown in this first contract.
Microsoft distinguishes direct/satisfying account entitlements from shared
device entitlements. A whole-title entitled-products result also needs complete
title association, including hidden, bundle-only and withdrawn products. The
public list of purchasable add-ons cannot establish that completeness.
[Entitlement and licensing rules](https://learn.microsoft.com/en-us/gaming/gdk/docs/store/commerce/fundamentals/xstore-granting-access-to-content?view=gdk-2604)

No part of this read-only work consumes currency, grants entitlements, imports
another device's licenses or performs a purchase. A successful catalog display
would still require separate validation of every subsequent commerce operation.

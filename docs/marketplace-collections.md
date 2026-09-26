# Marketplace integration

Flightdeck can list account-owned Marketplace content and supports genuine
signed licenses for eligible Durable products. Free-content downloads have been
reported working. Paid checkout, device-shared DLC rights and separate Store
package installation are not supported. Full DLC coverage and the MSFS 2024
Aviator Upgrade remain unverified. Catalog visibility alone does not establish
ownership or in-game availability.

License renewal retries an expired signed response only for an explicitly
requested product, preserves the original challenge and returns an unmodified
Microsoft-signed receipt. Missing ownership does not trigger retries. A renewed
receipt does not by itself confirm that every in-game download or activation
flow succeeds.

**Diagnostics** includes Store method names and result codes, excluding account
data, product IDs and raw logs. The integration uses the configured Store market
for game licensing. The sections below document the supported API scope for
contributors and advanced troubleshooting.

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
including cleanup when a caller only reads the count. Package responses do not
establish mandatory-update policy; Flightdeck's separate game-update workflow
handles downloads and installation.

MSFS 2020 can display a disc prompt during startup. Successful package and
license checks do not establish that the prompt is resolved. Microsoft describes
it as a [license-authentication error](https://flightsimulator.zendesk.com/hc/en-us/articles/360015985699--Please-insert-the-Microsoft-Flight-Simulator-Game-Disc-Error-message).
See also the [package-update flow](https://learn.microsoft.com/en-us/gaming/gdk/docs/store/commerce/fundamentals/xstore-checking-for-updates?view=gdk-2604).

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

Durable-license support does not establish compatibility with the MSFS 2024
Aviator Upgrade, which has a different product kind. See Microsoft's
[Durable licensing API](https://learn.microsoft.com/en-us/gaming/gdk/docs/reference/system/xstore/functions/xstoreacquirelicensefordurablesasync?view=gdk-2604).

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

Catalog metadata must be available for every returned entitlement. Unavailable
metadata returns an error rather than silently dropping owned content.

The publicly listed Aviator Upgrade (`9MTWMVDJ01FF`) is classified as an
`UnmanagedConsumable`, not a Durable. Durable inventory support therefore does
not establish availability of an existing Aviator purchase in the simulator.
The explicit catalog mapper carries
validated localized HTTPS images, search terms and product text; those fields
neither confirm ownership nor implement checkout.

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
Requests use product-wide filters with `expandSatisfyingItems=true`; they do
not mix product-wide and SKU-specific filters. A product-wide query does not
request the entire account library.

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

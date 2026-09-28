# Marketplace integration

Flightdeck can list account-owned Marketplace content and supports genuine
signed licenses for eligible Durable products. Free-content downloads have been
reported working. Paid checkout is not yet verified. Flightdeck 0.1.7
implements a Microsoft-hosted purchase dialog, described below.
Device-shared DLC rights and separate Store package installation remain
unsupported. Full DLC coverage and the MSFS 2024
Aviator Upgrade remain unverified. Catalog visibility alone does not establish
ownership or in-game availability.

Loading the Marketplace catalog, recognizing the game edition and completing
a purchase are separate operations. A working catalog does not establish
checkout support. Account or session errors during a purchase attempt do not
by themselves establish that the account owning the game is incorrect.

Flightdeck 0.1.8 fixes premature five-second timeouts in inventory and package
update queries. These calls now wait long enough for the broker's bounded server
requests to finish. A server timeout returned by the broker remains an error,
but does not itself close the shared connection used by later license queries.
An unresponsive or broken broker connection still requires a fresh game session.

Flightdeck 0.1.7 implements `XUserFindUserById` for the user
already added through Xbox authentication. It matches the exact validated ID
and returns an owned handle. Unknown IDs, absent users and shutdown return
errors; a lookup does not sign in or switch accounts. In an MSFS 2024 1.8.16.0
test, successful lookup allows the game to reach `XStoreShowPurchaseUIAsync`.
The Store-user check compares the opaque account context captured during Xbox
authentication with the current broker account; account changes and failed
account reads do not return a match. Successful lookup or matching accounts
alone do not establish checkout support.

**Play with local saves** skips Xbox cloud-save synchronization for that
session. It does not disable MSFS networking or the Marketplace. The next
normal launch checks cloud synchronization again. Local save storage is also
used during cloud-synchronized sessions, so `local_save_init.enabled=1` in a
diagnostic report is expected. See [cloud saves](cloud-saves.md).

License renewal retries an expired signed response only for an explicitly
requested product, preserves the original challenge and returns an unmodified
Microsoft-signed receipt. Missing ownership does not trigger retries. A renewed
receipt does not by itself confirm that every in-game download or activation
flow succeeds.

**Diagnostics** includes Store method names and result codes, excluding account
data, product IDs and raw logs. Diagnostics schema 4 adds `store_session`: a
bounded, timestamped sequence of native query, catalog and purchase-window
stages from the latest game session. Repeated events are retained in order.
`components_at_launch` contains the launcher version and hashes of the native
files observed at launch, recorded in that session's broker log. Older sessions
without this record report an unknown component set; the currently installed
files are never substituted for it. `partial` and `sources` identify clipped or
unavailable logs. An absent event is not proof that an operation never occurred.

The existing `store_catalog` summary distinguishes the authenticated ownership
query (`inventory`), metadata retrieval (`inventory-catalog`), product conversion
(`inventory-mapping`) and page construction (`inventory-page`). `80004001`
indicates an unsupported operation or product shape, not necessarily a network
outage. The cloud-sync section and `store_check` describe the current launcher
service, which may differ from the session recorded in the game log.

## Check Store without a purchase

In Flightdeck 0.1.7, select **Diagnostics → Check Store → Start
Store check** with the game closed. The check validates installed components,
the saved sign-in, the public title catalog, the signed game license and the
authenticated title library. It uses the same Store brokers as the simulator.
It does not open checkout, create an order, acquire a Durable license handle,
change accounts or refresh stored credentials. A missing or expired sign-in
requires the normal Flightdeck sign-in flow.

A separate local window asks you to confirm that its text and buttons are
visible. Only that confirmation passes the display step; loading HTML alone
does not prove that the window is visible. Closing it or waiting past its
90-second limit leaves the display step unconfirmed. The check can be cancelled
from the launcher, and it excludes simultaneous game starts and setup changes.
Results belong to the selected runtime and remain available in diagnostics for
the current launcher service. Passing these checks does not establish that
Microsoft's payment page loads or that a paid transaction succeeds.

The integration uses the configured Store market
for game licensing. The sections below document the supported API scope for
contributors and advanced troubleshooting.

## Purchase dialog

Flightdeck 0.1.7 replaces the purchase-dialog stub with an
asynchronous request to Microsoft's hosted confirmation page. The broker selects
one current desktop offer for the requested product/SKU and authenticates with
the configured Store account. Authentication data reaches the isolated window
through an inherited pipe and reaches Microsoft in the HTTPS request body;
it is not placed in command-line arguments, URLs or diagnostic reports.

The window first loads Microsoft's public prefetch document without credentials
or product data. It then opens the confirmation page from that genuine Microsoft
origin. Direct form submissions from the local opening view produce an opaque
origin, for which Microsoft returns an empty HTTP 403 response. The host waits
for the committed prefetch document and checks its exact URL before attaching
the dialog. It does not override origin headers or disable browser security.
Initialization has a separate 30-second limit and a visible connection error.
The confirmation request selects Microsoft's hosted Xbox layout. The window
keeps its own header compact after loading and follows validated height messages
from the Microsoft frame, within the available window space. Flightdeck does
not rewrite Microsoft's payment controls or legal text.

Microsoft handles prices, payment details and the user's final confirmation.
Flightdeck does not submit orders or fulfill consumables itself. Completion is
accepted only from the expected Microsoft frame, with a successful result and
an order identifier. Opening a page is not a completed purchase. Cancelling an active dialog returns cancellation. A failed dialog retains its
failure when closed; timeouts, account changes and service errors remain failures.
The window shows loading progress, a visible error if Microsoft does not signal
readiness within 45 seconds, a distinct expired-session message and a 15-minute
session limit. Errors stay visible until dismissed. The broker records only
fixed phase/outcome names, never URLs, tokens, payment details or order IDs. Concurrent requests cannot open a second
dialog, and requests are not automatically retried. Ownership caches are cleared
after completion or interruption so subsequent game queries read fresh data.
Closing a dialog does not reverse a payment already confirmed by Microsoft.

The initial scope is an unambiguous, non-trial desktop offer associated with
the running title. Subscription and bundle selection, gifting, redemption,
custom campaign metadata and separate Store package installation are outside
this purchase path. Simverse's publisher-managed wallet remains the simulator's
responsibility; Store inventory is not substituted for that wallet.

Automated tests cover request validation, asynchronous completion, cancellation,
account matching, message origins and the dialog host in an isolated browser.
An offline Linux window test also checks actual WebKitGTK rendering and
cancellation under X11 and Wayland. The webview uses the existing GTK container
to avoid an empty window caused by the previous foreign-window attachment.
The native navigation regression test reproduces the empty 403 response on a
loopback server and verifies the corrected origin, cancellation before navigation
and rejection of an unexpected document. It uses synthetic data exclusively.
The Store-account authentication exchange, anonymous page loading and Microsoft's
hosted stylesheet have also been checked. The authenticated confirmation page
and its layout have been checked in MSFS 2024 1.8.16.0. Completed paid
transactions, delivery of purchased content and layout across other systems
remain unverified.

Title inventory processing validates and exhausts the account's collection
pages, establishes each product's title relationship, and then interprets the
relevant rights. Unsupported metadata belonging to another title no longer
invalidates the current title's inventory. Unknown or malformed rights for the
current title still return an error. Pagination respects both the requested
product count and the transport limit while keeping a product's SKUs together.

## Current-game product and license preview

`XStoreQueryProductForCurrentGameAsync` returns the current title and its
account-owned SKUs through the authenticated inventory mapper. The returned
page owns its metadata and uses the normal product-query lifecycle. This
initial subset requires exactly one matching current-game product and a
complete page; it does not list unowned edition SKUs or infer ownership from
public catalog data. Unknown or incomplete inventory remains an error.
See Microsoft's [current-game product query](https://learn.microsoft.com/en-us/gaming/gdk/docs/reference/system/xstore/functions/xstorequeryproductforcurrentgameasync).

`XStoreCanAcquireLicenseForStoreIdAsync` previews a verified full-game license
for the current title or a signed eligible Durable grant associated with it.
It returns the exact licensable SKU without acquiring a handle or changing a
license concurrency slot. Consumable and unmanaged-consumable product kinds
return `LicenseActionNotApplicableToProduct`. Missing grants do not produce `NotLicensableToUser`, because the
account-only backend cannot rule out device-shared rights. Unsupported products,
authentication failures and timeouts also remain errors. Other games, trials, shared-device licenses and package-based preview
are outside this implementation. See Microsoft's [license-preview API](https://learn.microsoft.com/en-us/gaming/gdk/docs/reference/system/xstore/functions/xstorecanacquirelicenseforstoreidasync).

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

The current mapping supports non-trial, non-subscription products. In Flightdeck
0.1.7, an owned bundle SKU can be listed using its exact
authenticated entitlement. Catalog bundle membership does not grant ownership
of child products. Inventory also accepts a fallback translation returned for
the selected market, preserving its actual language. Explicit purchase-offer
queries retain their narrower restrictions.
The service reports direct and satisfying account coverage explicitly;
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

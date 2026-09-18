# Marketplace integration

The compatibility layer joins public catalog products with account-bound
Collections results. A real simulator test returned 250 consumable product/SKU
entries. A user subsequently downloaded free content successfully through the
in-game Store. Paid checkout and complete DLC inventory remain unverified.
Catalog entries alone do not establish ownership.

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

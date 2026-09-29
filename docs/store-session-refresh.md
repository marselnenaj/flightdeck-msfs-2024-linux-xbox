# Store session renewal and sign-in recovery

Included in Flightdeck 0.1.9. The reported blocking MSFS
Marketplace dialog still needs an end-to-end check on an affected installation.

## Automatic renewal

Store account selection, authenticated Store requests and Xbox ticket requests
now use one silent renewal path. It checks the stored Microsoft user and device
tickets and renews tickets expiring within two minutes through Microsoft's
existing signed token exchange. It reauthenticates an expired device with its
existing device identity. Missing device identities are not provisioned by a
Store request.

Concurrent connections share a renewal lock and re-read the resulting ticket.
A failed renewal backs off for 15 seconds; a newly stored login clears that
failure. The complete renewal has a 20-second deadline. The native Store and
Xbox IPC deadlines now allow renewal plus the subsequent server request instead
of terminating a renewing account query after five seconds.

The broker keeps the simulator's opaque Store context through an expiry gap
only for the same identified account. No expired credential grants access.
An observed logout, malformed or missing account, or account switch invalidates
the context. Requests still validate the account before and after Store access.
Renewal cannot overwrite a newer stored user ticket. Requested root tickets are
selected by audience and key, not their position in the response.

An explicit Microsoft interactive challenge or credential rejection is reported
as requiring sign-in, including SOAP authentication faults without a login URL.
The explicit `FailedAuthentication` and `InvalidSecurityToken` cases follow
[WS-Security fault definitions](https://docs.oasis-open.org/wss/v1.1/wss-v1.1-spec-errata-SOAPMessageSecurity.htm).
Unknown SOAP faults, network failures, malformed responses, timeouts and keyring
errors remain distinct failures. They do not delete credentials or launch an
interactive login loop. Purchase requests are never automatically replayed.

## Interactive recovery

With the simulator closed, select **Diagnostics → Check Store → Renew Microsoft
sign-in**. The genuine Microsoft sign-in window opens for the existing account.
Selecting another account fails before saving that account. Closing or
cancelling an incomplete login leaves the existing user credentials in place.
A successful exchange must include a valid root ticket before it is stored.

After login, Flightdeck rechecks the account, product catalog, signed game
license and authenticated library. The login command's exit code alone does
not establish recovery. No separate Store display-test window opens during
this flow.

For a cloud authentication failure, **Sign in again and sync** starts this same
flow. After all checks pass, Flightdeck retries the captured cloud operation.
Its existing account-bound journals, backups and conflict checks still apply.
Failure or cancellation does not start the simulator or upload saves. Stale
requests cannot resume a different cloud operation. Interactive login is
excluded while the simulator, setup or a cloud transfer owns the runtime.

## Evidence and limits

Synthetic Rust tests cover expiry, coalesced renewal, failure backoff, login
challenges, malformed replies, logout and account changes during renewal. The
broker regression for losing a same-account context after expiry is retained.
Launcher tests use executable local stand-ins to cover successful and failed
login, cancellation, rechecking and cloud-request binding. Browser checks cover
the German and English controls and mobile layout. Native IPC tests delay an
account response beyond five seconds and exercise the bounded timeout path.
These tests do not use real Microsoft credentials.

This fixes the identified broker and launcher recovery gaps. It does not prove
that every reported cloud error is caused by expired credentials, dismiss a
dialog owned by MSFS, or reconnect a broker process that has terminated. A
blocking dialog already shown by MSFS may require closing the simulator before
using launcher recovery. NVIDIA rendering is a separate issue; see
[known issues](known-issues.md).

The source changes are pinned in `compat/patches/xodus-broker.patch` and
`compat/patches/winegdk-runtime.patch`, with their reviewed source deltas and
upstream hashes. A clean stage must reproduce them before packaging.

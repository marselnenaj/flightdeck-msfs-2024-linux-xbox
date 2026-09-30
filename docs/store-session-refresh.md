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

## Interactive challenge correction (0.1.14)

The credential-fault classification added to the shared token exchange in 0.1.9
ran before checking the accompanying interactive challenge. A reply containing both a
credential fault and a Microsoft follow-up URL therefore terminated the login
instead of opening the next sign-in step. The CLI reduced this to its generic
window-or-flow error. The same message can have other causes and does not by
itself establish a WebKit or GPU crash.

Flightdeck 0.1.14 preserves the challenge for the interactive handler.
Both interactive login and silent renewal use the existing HTTPS validation for
`login.live.com` and `account.live.com`. Silent renewal still reports sign-in
required; credential faults without a valid challenge still fail. Account
matching, root-ticket validation and credential storage after successful
completion are unchanged.

Synthetic tests cover both supported SOAP credential-fault forms, both challenge
hosts, malformed and disallowed URLs, and rejection without an inline URL.
The positive regression fails against the 0.1.13 source and passes with the
correction. The immediately closing window still needs confirmation on an
affected installation. Update and restart Flightdeck before retrying sign-in.

A local X11/GTK/WebKit probe using the actual login webview module and a fresh
isolated browser profile displayed Microsoft's real sign-in form and kept the
window open. It did not initialize account storage, exchange credentials or
complete an account sign-in. This verifies local page rendering only.

## Browser challenge handoff (0.1.15)

An email-code verification failure after password submission remains reported
on 0.1.14. The window opens and accepts credentials before this failure. The
earlier rendering probe did not exercise that transition.

The browser runtime had two further handoff problems. Each follow-up view used
a new default WebKit context, losing the previous view's in-memory session
cookies. It also accepted queued token callbacks from inactive views: both
Microsoft's page and the `post.srf` fallback can deliver a token, and an old
callback could repeat its exchange after a verification view had opened.

Version 0.1.15 retains one `WebContext` for the entire interactive
attempt and dispatches tokens only for the active session. Cookies remain
scoped to that browser context; a new attempt creates a new context. Required
verification is not skipped, and account matching, challenge URL validation,
root-ticket validation and storage after successful completion still apply.

The runtime also preserves typed errors so the launcher can show fixed codes:
73 for a request failure, 74 for an unusable Microsoft response, 75 for credential
rejection without a supported challenge, 76 for a different account and 79 for
a fault without a supported challenge. Codes 70–72 retain their existing
window/preparation/storage meanings, including compatibility with older native
components. URLs, email addresses, tokens, codes entered by the user and raw
responses are not included in these messages.

`tests/compat/login-flow-test.py` runs the production webview with synthetic
loopback pages through password, email code and final completion. It reproduces
lost HttpOnly session cookies and duplicate callbacks on 0.1.14, checks both
handoffs together, and verifies that response errors retain their classification.
It does not contact Microsoft or initialize account storage. These regressions
establish defects in the handoff, but do not prove the cause of every affected
account's error or replace a real email-code sign-in test.

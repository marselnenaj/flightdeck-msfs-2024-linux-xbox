# Report a problem by email

[Deutsch](problem-reports.de.md)

Available in Flightdeck 0.1.9 and later.

Open **Diagnostics → Report a problem**, select a category, describe what
happened, and choose **Prepare report**. For graphics problems you can also
select what you saw. Flightdeck saves the report locally and shows the email
text for review.

**Open email draft** fills in `contact@flightdeck-app.com`, the subject, your
description and the collected diagnostics. No attachment or GitHub account is
needed. Review the draft in your email app and click **Send** there. Flightdeck
does not send mail itself and cannot confirm delivery.

For webmail, an unconfigured email app or a report too long for an email link,
use **Copy email text** and paste it into a message to the address shown. The
complete text remains visible if clipboard access is unavailable. **Save as
text** downloads an optional `.txt` copy; the report is never silently shortened.

## What is included

- Your description and optional observations, a report ID and capture time.
- Distribution, kernel and architecture; available GPU and driver information.
- The selected installation's last available game-session summary: launcher
  version, graphics components and settings, numeric authentication, Store and
  cloud errors, and the latest Store check if available.
- The current cloud-service status, explicitly separate from the last game log.

**In 0.2.6:** reports also include the latest recorded Fenix
installer, manager or repair attempt for the selected runtime: timestamps,
validated app version, runner category, exit codes and a fixed failure category.
They never include Fenix log text. Prepare a new report after the failed attempt;
older reports cannot recover this evidence retroactively.

Collection uses Flightdeck's existing diagnostic allowlist. Raw game logs,
account credentials, authentication tokens, usernames, local paths, hardware
UUIDs and save contents are not collected. Free text is yours: do not paste
passwords or private logs into the description. The report is passed to the
email handler you choose when opening the draft; for a webmail handler that can
be a web service.

The report describes the available data **when you prepare it**, not an
independently recorded snapshot at the instant of the failure. Polling, changing
the selected simulator and restarting Flightdeck do not replace the saved
report. Editing its description requires preparing it again. Preparing another
report replaces the previous draft.

The local draft is `problem-report.json` in Flightdeck's state directory
(normally `$XDG_STATE_HOME/flightdeck`, or `~/.local/state/flightdeck`) with
owner-only permissions. JSON is an internal storage format; the email and
optional download are plain text. **Delete local draft** removes that saved
copy, but does not remove downloaded files or email drafts.

## Maintainer setup

Official builds use `contact@flightdeck-app.com`. Reports arrive in that mailbox
as normal emails, identified by category and report ID in the subject. A reply
goes to the sender. There is no upload service, SMTP password or API key in the
launcher, and Flightdeck introduces no paid reporting service. Receiving mail
requires your existing mailbox to be configured and reachable.

A downstream build can set `FLIGHTDECK_SUPPORT_EMAIL` to a single valid email
address before launching. An empty or invalid value disables the email link;
local capture, preview, copying and download remain available.

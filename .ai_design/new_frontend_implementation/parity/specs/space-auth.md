> **Superseded.** This note was folded into [`space.md`](space.md) by NEWFRONT-751; that file is the authoritative build spec for the area. Kept for history (the oracle research notes it was written from).

# Area spec — spaces auth (SPC-058–064)

Source: NEWFRONT-452 oracle research. Behavior paraphrased from the old
space frontend's landing page, auth views and auth forms, session and
instance stores, OAuth hooks, and the spaces auth endpoints with their
providers and error catalogue. No code, selectors, copy or structure are
carried over; the new frontend is written from this prose.

## Email-first step (SPC-058)

The landing card starts on an email step for anonymous visitors: one
labeled address field with a clear control and a single continue action.
The address is format-checked in the client. The inline invalid-address
notice appears only after the field loses focus; while focused, a bad
address shows no notice. The continue action stays disabled while the
field is empty, while the address fails the format check, and while a
check is in flight. Submitting a valid address runs the server address
check and the card advances to whichever method step the routing below
selects. The check itself creates nothing server-side. A landing address
may carry an email parameter that pre-fills the field.

## Known vs new routing (SPC-059)

The server address check answers two facts: whether an account already
holds the address, and whether the next step is a one-time code or
credentials. Known addresses continue in sign-in mode, new ones in
sign-up mode. The method then follows instance configuration: the code
step requires a configured mail host plus the magic-login flag (for a
known address the account must also be a code-created one, since
password holders always route to credentials); otherwise the password
step requires the password flag. When neither method is available the
card does not advance: sign-in raises the dismissible banner with a
misleading invalid-address notice, while sign-up stays on the email step
in silence with no banner at all — both are current behavior pinned by
the oracle (see the SPC-059 row for the bug links). Clearing the locked
address on a later step returns the card to the email step but keeps the
current mode.

## Code step (SPC-060)

The code step shows the locked address (with a clear control back to the
email step), a code field, delivery guidance, and a resend control. Entry
generates a fresh code; resending re-generates and then disables itself
behind a short client-side cooldown of a few seconds before offering to
resend again. Submit is a native CSRF-protected form post to the
mode-specific magic endpoint; a post without the token is refused with a
dedicated verification-failed page (served with a success status and no
redirect) instead of running. The token is fetched when the step renders
(the password step instead awaits its token at submit time), so the code
submit must wait for it. A wrong code, a missing or consumed code,
and a past-cap resend each fail distinctly: the post redirects with the
matching error code, and opening that coded link at the landing root
lands back on the code step with the matching guidance banner (details
under SPC-063). Codes are single-use and short-lived server-side (minutes),
and only a few may be generated per address inside that window; past the
cap the generate call itself reports exhaustion, which the resend control
surfaces as a banner on the code step. A valid code signs the visitor in
and consumes the code — replaying it reports expiry instead of signing
in again.

## Password step (SPC-061)

The password step shows the locked address, one password field in
sign-in mode, and a second confirmation field in sign-up mode. Each field
has its own visibility toggle that flips only that field between hidden
and revealed text. In sign-up mode only, a non-empty password that fails
the strength rules shows a strength meter (a short verdict plus the
unmet criteria) and the submit stays disabled until the password passes
all rules and both fields match; a filled confirmation that differs from
the password shows an inline mismatch notice (once the confirmation
field is left or has grown at least as long as the password). Sign-in
needs only a non-empty password and never shows the meter. The primary
action is labeled for continuing when mail is configured and for going
to the workspace when it is not (sign-in mode); sign-up mode labels it
for account creation. Sign-in mode with mail configured additionally
offers a switch to the code step, which generates a code and moves the
card there; without mail there is no switch. Submit is a native
CSRF-protected form post to the mode-specific credential endpoint (a
token-less post is refused with the verification-failed page, as on the
code step): wrong
credentials redirect with the sign-in failure code and create no
session, while correct credentials (or a fresh sign-up) create the
session — the new account is a password account, and it routes as known
from then on.

## Provider buttons (SPC-062)

Third-party buttons render exactly when the instance enables providers:
any enabled provider among the four supported ones appears, and with
none enabled the card shows no buttons and no leftover slot (the divider
belongs to the button block, so it vanishes too). Off the password step
each button carries its full invitation text, which follows the current
mode; beside the password step the same buttons render compact as icons
only. Activating a button starts the provider flow at the app's provider
endpoint (not a spaces-prefixed one), carrying the return address.
Completing a provider flow is unobservable in the seeded stack (no
provider credentials), so the oracle proves presence, compact shape, and
flow start only.

## Failure banner and error links (SPC-063)

Card failures explain themselves in a dismissible banner above the
header; dismissing removes the banner and leaves the current step in
place. Failures that arrive as native-post redirects carry an error code
(plus a machine message and the address); opening such a coded link at
the landing root lands directly on the fixing step: credential failures
on the password step in the matching mode, wrong/expired/exhausted code
failures on the code step in the matching mode, and account-existence
failures on the email step (whose banners link onward to the other
mode). Codes the card does not know — including unknown numbers and the
rate-limit code — raise no banner and leave the email step untouched.
The banner only renders for banner-typed failures; inline step errors
(malformed address, mismatch, weak password) never use it.

## Headings and consent (SPC-064)

The header above the form carries a mode-specific heading and subline:
sign-in speaks to returning contributors, sign-up to new ones. Below the
form a consent line names the terms of service and the privacy policy as
links; its lead-in differs by mode (account creation vs signing in).
Both links are always present in both modes.

## Server shape behind the card

The card drives the spaces auth endpoints: the address check and the
code generate as JSON posts, and the credential/code submits as native
CSRF form posts that answer redirects. Success redirects to the spaces
base plus the validated return path; failures redirect to the spaces
base with the error code. (Under the parity serving the space app lives
at the proxy root rather than under that base, so native submits land
on the catch-all page there — the oracle asserts the redirect target and
the coded link, not a post-submit card.) The address check is anonymous-
rate-limited per minute; the code generate is capped per address as
described above. Live codes are held server-side per address; the seeded
stack prints mail to the console, so the oracle reads codes from the
server store instead. Code-created accounts are marked password-unset,
which is what routes them back to the code step; password accounts
always route to credentials. The session the spaces endpoints create is
the regular session cookie the space app reads, so a signed-in visitor
reopening the landing root meets the logged-in guidance view instead of
the card.

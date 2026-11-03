# Auth area spec (NEWFRONT-479)

Prose spec for inventory rows AUTH-001–044 (area epic NEWFRONT-69): sign-in,
sign-up, password recovery, route guards, landing, sign-out, account
switching, deactivation, session handling, CLI device approval, desktop
sign-in seams, authorization-failure screens, keyboard behavior, invitations,
onboarding, standalone workspace creation, and the first-run tour. Written
from reading the old sources listed below plus the five oracle spec files;
no old code, strings, styles, or assets are reused. New code is written from
this spec.

This file incorporates and supersedes the sub-area spec
`auth-invitations-onboarding-start.md` (AUTH-026–033): its findings are
folded into the sections below, so this is the single spec the auth slices
and the area gate build from. The sub-area file is left in place untouched.

Old sources consulted (reference only, never a source): routes
`app/(all)/{accounts/forgot-password, accounts/reset-password,
accounts/set-password, auth/device, sign-up, invitations,
workspace-invitations, onboarding, create-workspace}` plus the legacy
redirect routes; `core/components/account/` (auth forms, deactivation
dialog); `core/components/auth-screens/` (card shell, headers, denial
screens); `core/components/onboarding/` (flow root, header, steps);
`ce/components/onboarding/tour/`; `ce/components/desktop/sign-in-card.tsx`;
`core/services/auth.service.ts`, `auth-signout.ts`, `desktop-session.ts`;
the authentication guard helper and wrapper; the workspace/project auth
wrappers; the instance-config, user, and workspace services; the
desktop-overlay home page, desktop redirects, and desktop session wipe; the
Tauri deep-link handler and desktop README. Oracle behavior references:
`apps/web_new/e2e/parity/auth/sign-in-core.spec.ts` (NEWFRONT-107),
`sign-up-recovery.spec.ts` (NEWFRONT-108), `session-account.spec.ts`
(NEWFRONT-109), `invitations-onboarding-start.spec.ts` (NEWFRONT-110),
`workspace-onboarding.spec.ts` (NEWFRONT-111).

Conventions used below:

- Screens, states, rules, and API calls are described in prose. The only
  code-like tokens are API paths, query and field names, error codes, and
  short enum values — never control flow, markup, styles, or copy text.
- User-visible wording is described by gist (for example "a banner
  conveying authentication failure"). New copy must be written fresh; the
  one exception is text an oracle scenario pins by substring or accessible
  name, which is marked below as oracle-pinned so slices can see the
  constraint. Where a pinned string conflicts with the copying ban, the
  slice raises a `process:` comment on NEWFRONT-1 instead of working
  around it.
- "Instance config" means the `config` object of `GET /api/instances/`.
  The flags this area reads are `enable_signup`,
  `is_workspace_creation_disabled`, `is_google_enabled`,
  `is_github_enabled`, `is_gitlab_enabled`, `is_gitea_enabled`,
  `is_magic_login_enabled`, `is_email_password_enabled`,
  `is_smtp_configured`, and `is_self_managed`.
- "Onboarded" means the profile reports `is_onboarded`, or all four
  `onboarding_step` flags are set: `profile_complete`, `workspace_create`,
  `workspace_invite`, `workspace_join`.
- Two oracle scenarios pin live old-app bugs with `bug:` markers; the
  intended behavior is recorded in the row's section: AUTH-013
  (NEWFRONT-130, malformed reset uid) and AUTH-024 (NEWFRONT-157,
  invited outsiders). The area gate adjudicates fix-versus-pin.
- Three rows describe target behavior that differs from the live old app
  until main PR 494 merges (see "Target behavior: desktop onboarding
  without the CLI step"): AUTH-030, AUTH-031, AUTH-043. Slices build the
  target, not the live behavior.

## Entry URLs

- `/` is the sign-in entry: signed-out visitors get the sign-in card in
  place; everybody else is funneled elsewhere by the guards (AUTH-015).
- `/sign-up` is the canonical sign-up card. `/register` and
  `/accounts/sign-up` forward to `/sign-up/` (AUTH-011).
- `/sign-in`, `/signin`, and `/login` forward to `/` with no prefill or
  return-path handling (AUTH-044). Exception: in the desktop build,
  `/sign-in` and `/login` render the desktop sign-in screen instead, so a
  `?error=` query from a failed hand-off is displayed rather than dropped
  (AUTH-022).
- `/accounts/forgot-password`, `/accounts/reset-password`,
  `/accounts/set-password` (AUTH-012, AUTH-013, AUTH-014).
- `/auth/device` (AUTH-021). `/invitations`, `/workspace-invitations`
  (AUTH-026, AUTH-027). `/onboarding`, `/create-workspace` (AUTH-029,
  AUTH-040).

## Sign-in card (AUTH-001–008)

Shared chrome. The card page centers a narrow card beside a hero panel under
a top bar. The top bar carries the brand mark and, only when the instance
config enables sign-up, a cross-link to the other card (sign-in offers
sign-up and vice versa); when sign-up is disabled no cross-link shows. The
card header is generic per mode and step: a welcome-back line in sign-in
mode, a create-account line in sign-up mode (oracle-pinned: the sign-in
subheader contains "Welcome back to Pi Dash"). An invitation-aware variant
replaces the header (AUTH-005). A dismissible banner slot sits above the
header (AUTH-006). Provider buttons, when any provider is enabled, sit
above the email form with a divider between them (AUTH-003). A
terms-and-conditions line closes the card.

### AUTH-001 — sign in with email plus password

The card is email-first. The email step takes an address (autofocused, with
an in-field clear control that empties the field and keeps focus) behind a
continue action that stays disabled while the field is empty, fails a
client-side email-validity check, or a check is in flight; the invalid
hint appears only after the field loses focus. Submitting posts
`POST /auth/email-check/` with `{email}` and routes on the
`{existing, status}` answer: a known address with `CREDENTIAL` status
advances to the password step.

The password step shows the address locked with a clear control that resets
the whole card (see AUTH-004), a password field (autofocused, with a
show/hide toggle), and submits a native form POST to `/auth/sign-in/`
carrying `email`, `password`, a fresh `csrfmiddlewaretoken` fetched from
`GET /auth/get-csrf-token/` when the step mounts and injected at submit
time, plus `next_path` when one was supplied. Every submit ends in a full
page load: success lands per AUTH-016; wrong credentials return to the
form on the password step with an explanatory banner, and the server
creates no session. Dismissing the banner keeps the password step, and the
correct password still signs in afterwards.

### AUTH-002 — sign in with an emailed one-time code

The code step appears when the email check answers `MAGIC_CODE` for the
address, or when the signed-in-mode password step's secondary "unique
code" action is used (offered only when outgoing mail is configured).
Entering the step generates a code via `POST /auth/magic-generate/` with
`{email}`; code generation is skipped entirely when mail is unconfigured.
The step shows the locked address with the card-resetting clear control
and a code field (autofocused), and submits a native form POST to
`/auth/magic-sign-in/` carrying `email`, `code`, a fresh CSRF token, and
`next_path` when supplied.

Resend is throttled by a short client cooldown of five seconds: the resend
control counts down, then shows a requesting state while the generate call
is in flight, then offers resend again; it is disabled while cooling down
or requesting. Submit stays disabled while the code field is empty, a
resend is in flight, or a submit is in flight. A wrong code returns a
banner conveying an invalid code on the code step and creates no session
(the server answers the POST with a redirect carrying `error_code=5090`
and no session cookie).

### AUTH-003 — sign in through a third-party provider

One button per provider the instance config enables: Google, GitHub,
GitLab, Gitea (`is_google_enabled`, `is_github_enabled`,
`is_gitlab_enabled`, `is_gitea_enabled`). Disabled providers are not
offered; with none enabled no provider section renders. Each button starts
a full-page navigation to `/auth/{google,github,gitlab,gitea}/` on the API
origin, forwarding `next_path` when one was supplied, and the provider
flow returns the user authenticated. Buttons read compact on the password
step and full elsewhere; the button wording names the current mode and
provider ("sign in / sign up with …" gist). Provider-side failures return
through the AUTH-006 banner path.

### AUTH-004 — email-first routing between sign-in and sign-up paths

The email check answer selects the path. A known address stays in the
card's sign-in mode (password or code step per status, no confirmation
field). An unknown address on the sign-in card flips the card into sign-up
mode (password step with a confirmation field, sign-up header); a known
address on the sign-up card flips it into sign-in mode. Clearing the
address resets the card to its home mode and the email step, restores the
generic header, and navigates back to the card's home URL (`/` for
sign-in, `/sign-up` for sign-up).

### AUTH-005 — invitation-aware sign-in header

When the card URL carries `invitation_id` and `slug`, the card fetches the
single invitation with `GET
/api/workspaces/{slug}/invitations/{id}/join/` (no auth headers; the fetch
resolves after the form paints, showing a loading marker meanwhile). When
the fetched invitation's email equals the typed address and the invitation
carries a workspace, the header names that workspace (brand logo plus
name, "join" gist) with a mode-specific subheader (create-an-account vs
log-in to work with the team gist). Any mismatch — another address, a
missing workspace, no invitation — keeps the generic header.

### AUTH-006 — server error banner with step recovery

Every failure kind surfaces as one dismissible banner (announced as an
alert) above the header; dismissing clears the banner and keeps the
recovered step. Link-carried `error_code` values land the form on the step
that can fix them and set the matching mode:

- Authentication-failure codes land on the password step in their mode
  (sign-in code `5065`, sign-up code `5035`).
- The magic-code family lands on the code step in its mode: invalid code
  (`5090` sign-in, `5092` sign-up), invalid magic email (`5080` sign-in,
  `5050` sign-up), expired code (`5095` sign-in, `5097` sign-up), and
  exhausted attempts (`5100` sign-in, `5102` sign-up).
- All other recognized codes land on the email step with their banner:
  unknown account (`5060`, banner carries a create-account link that
  preserves the typed address), already registered (`5030`, banner carries
  a sign-in link preserving the address), disabled sign-up (`5015`),
  disabled magic login (`5016`), disabled password login (`5018`),
  deactivated account (`5019`), instance not configured (`5000`), invalid
  or missing email (`5005`, `5010`), weak or invalid password (`5020`,
  `5021`), mail unconfigured (`5025`), the sign-up/sign-in
  required-field and invalid-email codes (`5040`, `5045`, `5055`, `5070`,
  `5075`, `5085`), the provider configuration and provider-error codes
  (`5104`, `5105`, `5110`, `5111`, `5115`, `5120`, `5121`), the reset
  token codes (`5125`, `5130`), the change-password codes (`5135`,
  `5138`, `5140`), password-already-set (`5145`), the admin codes
  (`5150`–`5190`), and rate limiting (`5900`). Banner wording follows the
  failure gist (for example unknown-account, already-registered,
  provider-error, rate-limit); new copy must be written fresh.
- An unrecognized code raises no banner and leaves the email step.

Oracle-pinned cases: `5065` recovers on the password step with an
authentication-failure banner; `5090` and `5100` recover on the code step
with invalid-code and expired-code banners; `5060`, `5900`, and `5015`
recover on the email step with no-account, rate-limit, and sign-up-disabled
banners.

### AUTH-007 — no-authentication-methods notice

When the instance enables no provider and neither email login method
(`is_email_password_enabled` nor `is_magic_login_enabled`), the card shows
a notice instead of any form: it explains login is unavailable and points
at the administrator. Restoring either login method brings the form back.

### AUTH-008 — behavior when outgoing mail is unconfigured

When `is_smtp_configured` is false, the password step changes three ways:
the reset entry becomes an explanation (a popover conveying that the
server cannot send mail) instead of a link to the forgot-password page;
code login is not offered (no secondary unique-code action, and the code
step is unreachable); and the primary button carries the mail-less label
(oracle-pinned: "Go to workspace") instead of the standard continue
label. The server agrees mail is off and refuses code generation (error
`5025`).

## Sign-up card and entry URLs (AUTH-009–011, AUTH-044)

### AUTH-009 — account creation (password and code paths)

The sign-up card at `/sign-up` (signed-out-only) runs the same email-first
machine as the sign-in card with sign-up as its home mode. A new address
with `CREDENTIAL` status reaches the password step with a confirmation
field; submitting matching strong secrets posts the native account-creation
form to `/auth/sign-up/` (`email`, `password`, `confirm_password`, fresh
CSRF token, `next_path` when supplied) and the newcomer lands on the
post-sign-up path (`/onboarding` via the guards), never silently membered
in any workspace. A new address with `MAGIC_CODE` status (mail configured)
reaches the code step instead; the mailed code posts to
`/auth/magic-sign-up/` and likewise lands on onboarding, leaving a
passwordless account (the server marks the password auto-set). The server
stores the account and, on the password path, the secret; the new address
checks as existing afterwards and the new secret signs in.

### AUTH-010 — new-password strength and confirmation enforcement

Secrets are gated client-side before anything is posted. On the sign-up
card a weak secret raises an inline banner conveying that a strong
password is required and the form is not submitted; a non-empty
confirmation that differs from the secret shows an inline mismatch message
and disables submit. The mismatch message appears once the confirmation
field loses focus or grows at least as long as the secret. The
reset-password and set-password forms enforce the same two rules by
disabling submit until the secret is strong and both fields match (the
reset password field additionally requires a minimum length of eight).
A strength indicator accompanies the secret field while it holds a weak
value.

### AUTH-011 — sign-up entry behavior

Each legacy sign-up address forwards to the canonical card: `/register`
and `/accounts/sign-up` redirect client-side to `/sign-up/` after
hydration. An `email` query value prefills the card's email field, and a
`next_path` query value is carried through the steps as a hidden field so
it survives to the post-login redirect.

### AUTH-044 — retired sign-in addresses land on the home route

The legacy addresses `/sign-in`, `/signin`, and `/login` forward to `/`
as bare redirects: no email prefill and no return-path handling, unlike
the sign-up legacy URLs in AUTH-011. (Desktop builds override `/sign-in`
and `/login` to render the desktop sign-in screen so hand-off errors
survive; see AUTH-022.)

## Password recovery and session handling (AUTH-012–014, AUTH-020)

### AUTH-012 — forgotten-password request with resend throttling

The forgot-password page (`/accounts/forgot-password`, signed-out-only)
prefills its email field from the `email` query value and validates it
(required, valid address). Submitting posts
`POST /auth/forgot-password/` with `{email}`. Success shows a success
toast conveying an inbox check and starts a thirty-second resend countdown
that disables the email field while the submit button counts down; the
page offers a way back to sign-in. Failures surface an error toast
carrying the server detail when one exists, else a generic message —
without leaking account-existence details (no unknown-account / not-found
wording even for addresses with no account).

### AUTH-013 — password reset from the emailed link

The reset page (`/accounts/reset-password`, signed-out-only) reads
`uidb64`, `token`, and `email` from the query. It shows the address
locked (not editable, not submitted) with new-secret and confirmation
fields behind the AUTH-010 gates (strength plus match, submit disabled
until both hold, mismatch message, per-field show/hide). Submitting posts
the native form to `/auth/reset-password/{uidb64}/{token}/` with
`password`, `confirm_password`, and a fresh CSRF token. Success signs the
new secret in effect (the new secret authenticates, the old one stops)
and returns to the sign-in entry. Invalid or expired links render an
explanatory banner (invalid/expired-token gist) instead of the form, and
reset links are single-use: reusing a consumed link surfaces the same
banner rather than resetting again.

Recorded bug (oracle-pinned, NEWFRONT-130): a malformed `uidb64` crashes
the native reset POST with a raw HTTP 500 at the `/auth/reset-password/`
URL instead of returning to the banner flow. Intended per this row: an
explanatory invalid-token banner. The scenario locks the live behavior
with a `bug:` marker so the fix has a regression test; the area gate
adjudicates.

### AUTH-014 — first password for passwordless accounts

The set-password page (`/accounts/set-password`) is guarded for
passwordless accounts: users whose server record marks the password
auto-set (for example provider-created or code-created accounts) see the
form, while users who already hold a secret and are onboarded are sent
away to the landing route instead of seeing it. The live page renders the
shared reset form; a standalone set-password form also exists in the old
tree but is not mounted anywhere — it would post
`POST /auth/set-password/` with `{password}` under an `X-CSRFTOKEN`
header, navigate to `/` on success, and toast failures. The oracle proves
reachability (passwordless users reach the form, set users are bounced),
not submission; slices implement the guard rule plus the shared form and
must not invent a second submission path beyond what the oracle can prove.

### AUTH-020 — session expiry and request-forgery handling

An expired session sends the user to sign-in preserving the return path:
any 401 moves the session to an expired state that keeps the current
location, and the app lands on the sign-in entry with that location as
the return path. Every login and password native form carries a fresh
forgery token with each submission: the card fetches
`GET /auth/get-csrf-token/` when a credential step mounts and injects the
token at submit time. A credential POST without the token issues no
session, while the UI flow that supplies it authenticates normally.

## Guards, landing, sign-out, switching, deactivation (AUTH-015–019)

### AUTH-015 — route guards

Five page types gate the area (evaluated after the session, profile, and
workspace list load; while loading with no user yet, a brand spinner
shows):

- Public pages render for everyone. Only `/workspace-invitations` is
  public in this area.
- Signed-out-only pages (sign-in entry, `/sign-up`, forgot/reset
  password) render for visitors. Signed-in users who are onboarded leave
  for the AUTH-016 landing; signed-in users who are not onboarded leave
  for `/onboarding`.
- The onboarding funnel (`/onboarding`) bounces signed-out visitors to
  `/?next_path={page}` and finished users away to the landing route
  (replacing history); unfinished signed-in users render it.
- The set-password page bounces signed-out visitors to
  `/?next_path={page}`; signed-in users who already hold a secret and are
  onboarded leave for the landing, and everyone else renders it.
- Signed-in-only pages (the default: invitations, device approval,
  standalone creation, every workspace page) render for onboarded members;
  signed-in users who are not onboarded leave for `/onboarding`, and
  signed-out visitors leave for `/?next_path={page}`.

### AUTH-016 — post-authentication landing

Landing resolves in order: a safe return path first, else the last or
fallback workspace still available to the user, else workspace creation.
The in-app guard honors a `next_path` only when it is not an absolute
URL (no `http`, `https`, or `ftp` scheme); the native login POST
additionally sanitizes an absolute return path down to its local path so
landing never leaves the origin (an absolute URL to another origin lands
on that URL's path within the app). Otherwise the landing uses the
settings' `last_workspace_slug`, falling back to `fallback_workspace_slug`,
but only when that slug is still in the user's workspace list; when
neither applies — for example an onboarded user who belongs nowhere —
landing is `/create-workspace`. The old app normalizes workspace landings
with a trailing slash.

### AUTH-017 — sign-out from every entry point

Sign-out is offered from the sidebar user menu, the sidebar workspace
menu, the command-palette account command, the workspace-not-found
screen, the change-email dialog, the deactivation dialog, and the
account-switching dialog. Confirming ends the server session with a
sign-out POST to `/auth/sign-out/` carrying a fresh CSRF token (the old
app submits it as a hidden form), after disposing the agent runtime and
before clearing client state: every client store resets and the cached
server data drops. The user returns to the signed-out entry (`/`; `/login`
on desktop). A sign-out that fails surfaces an error toast and leaves the
user where they are.

On desktop the server's cookie deletions never reach the webview (page
and API are different origins), so the app additionally wipes its local
webview store — cookies plus local and session storage plus caches —
after the sign-out request and before landing. A wipe that fails is
swallowed: landing on sign-in is still the right outcome. This desktop
clause overlaps DESK-010 (full local teardown on sign-out, already oracle
green, owned by the desktop epic NEWFRONT-77): the desktop area owns the
wipe command itself, while the auth area owns calling the teardown seam
in the right order around its sign-out POST. The oracle proves the
browser legs here; the wipe leg is proven under DESK-010.

### AUTH-018 — account switching

The switch-account control shows which account is active and offers
switching: in onboarding it hangs off the header account menu (avatar
plus display name, with a single "wrong email address" gist item); the
dialog it opens names the active account's address and explains switching
is for users who signed in with the wrong identity. Confirming routes
through sign-out into a fresh login: the session ends per AUTH-017, the
theme resets to the system default, and the app lands on `/` for a new
login. A failed sign-out surfaces an error toast and the dialog stays.
The control surfaces live on shell and profile surfaces owned by later
epics (see the seams section); the auth slice owns the dialog and the
sign-out-into-login call path.

### AUTH-019 — account deactivation

The deactivate control lives on the profile-settings general page (owned
by NEWFRONT-90). It opens a confirmation dialog explaining the
consequences (loss of assignment and billing standing, reactivation only
through a fresh workspace invitation to the same address) with cancel and
confirm actions; confirming shows a deactivating state. Confirmation
issues `DELETE /api/users/me/` (no body), then on success toasts the
deactivation, signs out per AUTH-017, and returns to the signed-out
entry. A failed deactivation surfaces an error toast carrying the server
detail and leaves the account signed in. The dialog-versus-API split with
NEWFRONT-90 is recorded in the seams section: the settings epic owns
placement and trigger, the auth epic owns the dialog and the call path,
and the gate proves the flow through the API path plus the dialog
contract without depending on the settings page.

## Device approval, desktop sign-in, denials, keyboard (AUTH-021–025)

### AUTH-021 — CLI device approval in the browser

The device-approval page (`/auth/device`, signed-in) takes the short code
the CLI printed on the user's terminal. The code field is autofocused and
auto-formats as typed: non-alphanumeric characters are stripped, letters
uppercased, and a dash inserted after the first four characters
(`XXXX-YYYY`, eight code characters plus the dash). Submit stays disabled
until eight code characters are present and no approval is in flight. A
CLI deep link carrying `?code=` prefills the field and fires one
automatic approval attempt on mount; the user can still edit and resubmit
afterwards.

Approval posts `POST /api/v1/auth/device/approve/` with `{user_code}`
(the CLI starts its flow at `POST /api/v1/auth/device/start/`, which
answers `{user_code}`). Success replaces the form with a confirmation
naming the approving account's email and workspace slug and noting the
CLI picks the token up within seconds. Failure keeps the form editable
and shows the server's error detail, or a generic retry message when the
server returns none. A footer note warns to approve only codes the user
started themselves on a device they control (terminal login-command
gist). Approving the same code twice as the same user is idempotent,
while a different user is rejected — together proving the code is bound
to the approving account.

### AUTH-022 — desktop sign-in through the system browser plus deep-link return

Desktop login runs outside the webview: the app opens the system browser
for login (always from an explicit click — the card must never start
sign-in on its own) and completes the session when the identity provider
redirects to the `pidash://auth/callback` deep link carrying `code` and
`state`. The desktop shell navigates the webview to
`/api/auth/desktop-exchange/` on the server origin with that `code` and
`state`, which sets the session cookies on the same origin and redirects
into an app route. Provider-side or aborted flows land back on sign-in
with an explanatory error: when the callback carries `error` (and
optionally `error_description`), or when `code`/`state` is missing, the
shell navigates the bundle to `/sign-in?error=…` where an alert above the
card conveys that sign-in did not complete, with the provider description
or a retry line naming the error.

This hand-off exists only where the server implements the exchange
endpoint; the community server has none, so the plain desktop build shows
the AUTH-023 card instead. The auth slice builds the web-side contract
(the error landing that reads `?error=` / `?error_description=`, the
never-autostart rule); the desktop epic NEWFRONT-77 owns the shell halves
(browser opening, deep-link handling, exchange navigation) behind the
platform seams (`openExternal`, deep-link subscription). AUTH-022 and
AUTH-023 have no oracle scenario in this checkout (desktop-only halves
with no web-observable seam; NEWFRONT-486 adds their scenarios and
NEWFRONT-491 runs them old-first, then new).

### AUTH-023 — desktop unavailable-sign-in card

Where direct desktop login is unsupported, the desktop root (and the
`/login` and `/sign-in` overrides, all rendering the same bare screen
under the signed-out-only guard) shows an explanatory card instead of a
dead form: it conveys that the server does not support desktop sign-in
yet and offers opening the web app in the system browser, resolved
against the baked web base URL with the API origin as fallback (and a
plain navigation fallback when no native bridge answers). The card's
heading deliberately avoids the wording an edition hand-off card is
recognized by, so a misbuilt bundle fails its build check instead of
passing silently. Signed-in users on these routes are forwarded to their
workspace, honoring `?next_path=`.

### AUTH-024 — authorization-failure screens

Project denials key off the project-details fetch status and the
permission store. A user lacking project membership (409), or a workspace
admin facing a private project (403 plus admin standing), gets the join
screen: it conveys the missing access and offers joining; confirming
posts `POST /api/users/me/workspaces/{slug}/projects/invitations/` with
`{project_ids: [id]}`, refreshes the permission store (the joined project
takes the user's workspace role) and the project's entities, with the
join action disabled while the join is in flight. A non-admin facing a
private project (403) gets the no-access screen conveying the missing
access and pointing at the workspace admin, with no join action. Unknown
or otherwise failed projects (404 and anything else) report project
not-found. While the details load, nothing renders.

Workspace denials split two ways. When the slug is absent from the user's
workspace list and no membership info exists (Case A), the app shows the
workspace-not-found screen: a header with the brand mark, the signed-in
address, and a sign-out action; an illustration; a "Workspace not found"
heading (oracle-pinned) with a line conveying the URL may not exist or
may be out of reach; and actions that depend on membership — a way home
plus a profile visit when the user belongs somewhere, or workspace
creation when they belong nowhere (oracle-pinned: the home action reads
"Go Home"). When the slug is in the list but membership info is missing
(Case B), the app shows the not-a-member screen conveying the missing
membership and pointing at pending invitations (link to `/invitations`)
and workspace creation (link to `/create-workspace`).

The permission store feeds are the own-membership read
(`GET /api/workspaces/{slug}/workspace-members/me/`) and the per-project
roles read (`GET /api/users/me/workspaces/{slug}/project-roles/`), both
fetched only once the slug is known in the workspace list, plus the
project member-info read behind the project wrapper. The project join
call and the store feeds are shared with the issues and projects areas
(see the seams section): auth owns the denial screens and the join call
path, later areas reuse the feeds.

Recorded bug (oracle-pinned, NEWFRONT-157): a user invited to a workspace
but never joined opens that workspace and sees Case A ("Workspace not
found") instead of Case B, because the workspace list filters to active
memberships so the membership read never fires. Intended per this row:
the not-a-member screen. The scenario locks the live behavior with a
`bug:` marker; the area gate adjudicates fix-versus-pin.

### AUTH-025 — keyboard and focus behavior (no custom shortcuts)

Every step autofocuses its primary input (email, password, code, device
code, workspace name, admin email). Native forms submit with Enter; the
email-step clear control is reachable by pointer but excluded from tab
order, while every other toggle, clear, dismiss, resend, and navigation
control is a keyboard-reachable button. The invite-members form
deliberately suppresses Enter submission so multi-row editing never
sends early. No screen in this area requires custom key bindings (the
command-palette shortcut is shell-owned).

## Invitations (AUTH-026–028)

### AUTH-026 — invitation inbox with batch accept

The invitations page (`/invitations`, signed-in-only) lists
`GET /api/users/me/workspaces/invitations/` as one selectable card per
pending invite: workspace logo and name (long names truncated) plus the
invited role label. Clicking a card toggles its selected state (selected
cards are visually highlighted with a check mark). One primary action
accepts and joins all selected invites at once via
`POST /api/users/me/workspaces/invitations/` with
`{invitations: [ids]}`; it stays disabled while nothing is selected or a
join is in flight (and a guarded submit with nothing selected toasts a
select-something error). Success stamps the first joined workspace as the
user's last workspace (`last_workspace_id`), refreshes the workspace
list, and lands inside that first workspace. A secondary "go home" action
leaves for `/` without joining anything. With zero pending invites the
page shows an illustrated empty state (oracle-pinned: "No pending
invites" plus an explanatory line) with a single action back home.

### AUTH-027 — single-invitation emailed link (accept or decline)

The workspace-invitation page (`/workspace-invitations`) is public and
driven by three query params: invitation id (`invitation_id`), workspace
slug (`slug`), and `token`. It fetches the invitation with
`GET /api/workspaces/{slug}/invitations/{id}/join/`.

- Pending invites render a card naming the workspace with two actions:
  accept posts `{accepted: true, token}`, ignore posts
  `{accepted: false, token}` (ignore requires the token present).
- Stale states replace the actions entirely. Answered-plus-accepted
  renders an already-member card (oracle-pinned: "already a member") with
  a continue-home action — reachable only when the invite was answered
  before any account existed for its address, because accepting with a
  matching account deletes the invitation row, so revisiting that link
  renders the inactive card instead. Answered-plus-declined, or any fetch
  error, renders the no-longer-active card (oracle-pinned: "not active
  anymore") with home-or-sign-in plus community actions. An unresolvable
  invitation id renders the inactive card too: the "not found" branch
  needs fetched detail plus a fetch error at once, which a fresh broken
  link never produces, so it is effectively dead.
- The invitation model has no expiry field, so there is no distinct
  "expired" rendering; declined and broken links share the inactive card.

### AUTH-028 — landing after answering an invitation

Accepting a single invite lands in the joined workspace when the
signed-in address matches the invitee address, and on the home route
otherwise. Ignoring always lands on the home route (the bare origin —
anything deeper means the wrong landing). Batch-accepting from the inbox
lands in the first joined workspace.

## Onboarding (AUTH-029–039, AUTH-043)

### AUTH-029 — onboarding gate and resume

Onboarding is a single-URL flow at `/onboarding` behind the onboarding
funnel guard: signed-out visitors bounce to sign-in with a return path,
finished users bounce away to the landing route, and unfinished
signed-in users render it. The page pre-fetches the workspace list, the
pending invitations, and the user's workspace join requests, showing a
brand spinner until the user and invitations load.

A refresh resumes mid-flow from the stored profile progress flags: a
user with the profile flag stored but no workspace created or joined
resumes at the workspace step; a user who created but has not invited
resumes at the invite step. A pending workspace join request parks the
user on the holding view (the workspace step defaults to it; see
AUTH-034). Step order on browser builds is CLI install, profile, role,
use case, workspace create-or-join, invite members; self-managed
instances skip the role and use-case steps; desktop builds omit the CLI
step (see the target-behavior section). The header shows a progress
fraction (current step over visible total, with the invite step counted
only while the user has no invitations or is already on it) and the
account menu; step content scrolls to top on every step change.

Forward transitions stamp progress as they go: leaving the CLI step
stores nothing, and leaving role setup stores nothing beyond the role
value. Leaving use-case setup — or leaving profile setup on self-managed
instances — stamps `profile_complete` and then either finishes
immediately when the user already belongs to a workspace or moves to the
workspace step. Advancing from workspace creation to the invite step
stamps `workspace_create`. Completing the invite step stamps
`workspace_invite` as part of finishing (subsumed by the finish
routine's all-flags stamp in AUTH-039).

The old tree also holds older full-page onboarding variants beside the
steps flow (previously, invited users finished right after profile
setup). They are unmounted dead code encoding the same capabilities, so
they carry no rows; the oracle confirms nothing extra survives.

### AUTH-030 — CLI-install onboarding step

The first browser step shows per-OS install guidance in switchable tabs
("macOS / Linux" with one shell command, "Windows" with two, one per
shell), preselecting the tab matching the browser OS. Each command block
names its shell and offers a copy action with a copied confirmation; when
copying is unavailable (for example non-secure origins without clipboard
access, after a manual-copy fallback attempt), an error toast explains
copying is unavailable and to copy manually. A warning callout conveys
that the CLI plus coding agents are required end to end. Both "skip for
now" and the primary continue advance identically to the profile step
and store nothing — the profile flags stay at their sign-up default (all
false), which the server distinguishes from null.

The step's heading and explanation carry main PR 357 (`a41baf3a4`,
PDASHOSS01-127, NEWFRONT-1029). The heading says "Connect your AI agents
with Pi Dash" and the explanation covers two facts: the CLI is what
connects the user's AI agents to Pi Dash, so tasks can be handed to
those agents, and it has to live on whichever machine hosts the coding
agent, since that is where the work happens. The rebuild writes its own
sentence for the explanation (goal 1: our copy, not the old app's,
including the install-location sentence), so the parity
scenario pins intent — the step the user lands on and both required
facts on the rebuild — while the heading sentence itself is the
product copy main#357 settled. The oracle built from this checkout
predates main#357, so its heading still reads "Install the Pi Dash
CLI" and the scenario accepts either heading there (same intent-pin
convention as NEWFRONT-934).

On desktop builds this step is omitted entirely and onboarding starts at
profile setup (target behavior per main PR 494; see the target-behavior
section). The live old app still shows the step on desktop.

### AUTH-031 — profile-setup onboarding step

The profile step collects the display name (required, validated as a
person name — letters, spaces, hyphens, and apostrophes, at most fifty
characters, no injection-risk characters — with an inline error) plus an
optional avatar through an upload dialog with change and remove actions;
submit stays disabled until the name validates. Accounts that never set
a password (the server marks the password auto-set) additionally get a
password-plus-confirmation pair behind the AUTH-010 gates (strength and
match; submit disabled until both hold); other accounts do not see it.
On non-self-managed instances a marketing-consent checkbox (default on)
shows below the submit.

Submitting writes the user row (`PATCH /api/users/me/` with
`first_name`, `last_name`, `avatar_url`), sets the password where offered
(`POST /auth/set-password/` with `{password}` under a fresh `X-CSRFTOKEN`
header), and stores consent on the profile (`PATCH
/api/users/me/profile/` with `has_marketing_email_consent`). A user-row
failure toasts but still advances. The step then advances to role setup
— or, on self-managed instances, stamps `profile_complete` and either
finishes immediately when the user already belongs to a workspace or
moves to the workspace step. On desktop this is the initial step with no
back affordance (target per main PR 494).

### AUTH-032 — role-selection onboarding step

Seven fixed roles (product and engineering management, designer,
developer, founder/executive, operations, others), exactly one required:
submit stays disabled until a choice is made, and submitting empty shows
a required-field message. Saving stores the role id on the profile
(`PATCH /api/users/me/profile/` with `role`) with a success toast; skip
advances without storing. A failed save toasts but still advances.
Skipped entirely on self-managed instances — the flow jumps from profile
straight to the workspace step and stores no role. The oracle proves the
skip on the self-managed stack; the choose-and-require UI is
non-self-managed-only and its driver methods cover it until such a stack
exists.

### AUTH-033 — use-case onboarding step

Five shared use cases, multi-select with checkboxes, at least one
required with the same disabled-until-chosen plus inline-message behavior
as the role step. Saving stores the selection on the profile as one
combined value (`PATCH /api/users/me/profile/` with `use_case` set to
the choices joined with a separator); skip advances without storing,
with the same toast-and-advance failure rule. Skipped entirely on
self-managed instances with the same oracle caveat as the role step.

### AUTH-034 — workspace create-or-join onboarding step

Four sub-views under one step. The default view resolves by precedence:
a pending join request forces the holding view; else outstanding invites
default to the invites view; else the create view.

- Create view: name, URL slug, and team-size pills (see AUTH-035 for the
  fields and checks). Creating posts the workspace, stamps it as the
  last workspace, refreshes the workspace list and settings, and advances
  to the invite step — or finishes immediately for the solo size or the
  already-member shortcut. Two ghost actions leave the view: joining an
  existing workspace (shown only when invitations exist) and joining by
  admin email. When workspace creation is disabled the view is replaced
  by the AUTH-041 inline notice.
- Invites view: checkbox cards (logo, name, role) with a continue action
  disabled until at least one invite is selected or a join is in flight;
  continuing batch-accepts the selected invites
  (`POST /api/users/me/workspaces/invitations/` with
  `{invitations: [ids]}`), refreshes workspaces and settings, and
  finishes onboarding. A ghost action returns to the create view.
- Join-by-email view: see AUTH-036.
- Holding view: see AUTH-036.

The already-member shortcut: when the join-request answer carries a
workspace slug (the typed address resolved only to a workspace the user
already belongs to), the flow refreshes and finishes directly instead of
parking on the holding view.

### AUTH-035 — workspace URL availability and reserved-name enforcement

The slug derives from the name as typed (lowercased, trimmed,
spaces-to-dashes) but stays editable. The name is required, at most
eighty characters, holding letters, numbers, spaces, hyphens, and
underscores with no injection-risk characters; the slug is required, at
most forty-eight characters, holding letters, numbers, hyphens, and
underscores. A malformed slug shows an inline message (allowed-characters
gist) and blocks submit before anything is created. Submitting checks
availability with `GET /api/workspace-slug-check/?slug={slug}`: the
answer must report available AND the slug must avoid the restricted list
— `404`, `accounts`, `api`, `create-workspace`, `god-mode`,
`installations`, `invitations`, `onboarding`, `profile`, `spaces`,
`workspace-invitations`, `password`, `flags`, `monitor`, `monitoring`,
`ingest`, `pi-dash-pro`, `pi-dash-ultimate`, `enterprise`,
`pi-dash-enterprise`, `disco`, `silo`, `chat`, `calendar`, `drive`,
`channels`, `upgrade`, `billing`, `sign-in`, `sign-up`, `signin`,
`signup`, `config`, `live`, `admin`, `m`, `import`, `importers`,
`integrations`, `integration`, `configuration`, `initiatives`,
`initiative`, `workflow`, `workflows`, `epics`, `epic`, `story`, `mobile`,
`dashboard`, `desktop`, `onload`, `real-time`, `one`, `pages`,
`business`, `pro`, `settings`, `license`, `licenses`, `instances`,
`instance`. Taken, reserved, or otherwise unavailable values show an
inline taken message and keep the user on the view creating nothing.
Team size is required, picked from "Just myself", "2-10", "11-50",
"51-200", "201-500", "500+".

### AUTH-036 — workspace access request plus pending view

The join-by-email view takes a workspace admin address (autofocused,
validity-gated submit, Enter submits): posting
`POST /api/users/me/workspaces/join-requests/` with `{admin_email}`.
Every accepted address parks the user on the holding view naming that
address — the outcome is identical whether or not the address belongs to
a real admin, so the request reveals nothing about the address. A
rejected submit surfaces an error toast. The holding view conveys the
request is pending approval and names the admin address; its only action
returns to the create view ("create instead" gist). The already-member
shortcut (AUTH-034) bypasses the hold entirely.

### AUTH-037 — invite-members onboarding step

The step opens with three blank address-plus-role rows (roles default to
member), an add-another action that grows the list on demand, and
hover-remove controls once more than one row exists. Malformed addresses
show an inline message per row; sending stays disabled until at least one
valid address exists (plus form validity and no send in flight); Enter
never submits the form. Sending drops empty rows and posts
`POST /api/workspaces/{slug}/invitations/` with
`{emails: [{email, role}]}` against the first workspace (roles: 20 admin,
15 member, 5 guest), toasts success, and finishes onboarding; a failed
send toasts the server detail and stays. Deferring through the
finish-without-inviting action completes onboarding without sending
anything.

### AUTH-038 — solo-workspace shortcut

Choosing the "Just myself" team size finishes onboarding immediately
after creation: no invite step is shown, the invite step is hidden from
the header progress, and landing proceeds per AUTH-016. The server marks
the user onboarded with no pending invites recorded.

### AUTH-039 — onboarding completion and landing

Finishing stamps the profile (`PATCH /api/users/me/profile/` with all
four `onboarding_step` flags true plus `last_workspace_id` set to the
first workspace), marks the user onboarded (`PATCH
/api/users/me/onboard/` with `{is_onboarded: true}`), refreshes the
cached profile and settings (with cache-busting), and only then marks
the local profile onboarded. Landing follows AUTH-016 (into the new or
first workspace in the create/join flows). A finish that fails surfaces
an error toast.

### AUTH-043 — in-flow back navigation

The header chevron moves step to step along the visible sequence:
profile returns to the CLI step (browser only), role to profile,
use-case to role, workspace to profile on self-managed instances and to
use-case otherwise. The control hides on the first step (CLI install on
browser; profile setup on desktop per main PR 494) and on the last step
(invite members). The browser location never changes with the step —
back is in-flow only on the single onboarding URL.

## Standalone creation and first-run tour (AUTH-040–042)

### AUTH-040 — standalone workspace creation

The standalone route (`/create-workspace`, signed-in-only) offers the
same fields and checks as the onboarding create view (AUTH-035: name,
slug, availability plus restricted-name enforcement, required team size)
with full-page chrome: brand mark linking home, the signed-in address,
and a team-size dropdown instead of pills. Success stamps the new
workspace as the last workspace and enters it directly — no invite step
follows. The shell workspace-switcher seam that reaches this route is
recorded in the seams section.

### AUTH-041 — workspace-creation-disabled states

When the instance config disables workspace creation, the standalone
route shows an admin-only screen (illustration, "only the instance admin
can create workspaces" gist, go-back plus ask-admin actions — the latter
opening a pre-addressed mail with a prefilled subject and a body template
carrying the requester's name and email) instead of the form. The
in-onboarding create view instead shows an inline notice conveying the
restriction and steering to joining (a single action into the
join-by-email view).

### AUTH-042 — first-run product tour

The tour is a modal overlay on the workspace home, gated per user on the
profile's `is_tour_completed` flag: it shows until completed or
explicitly skipped, once per user. It opens on a welcome stop (product
concept video, greeting personalized with the user's first name, a
take-the-tour action and a skip-and-explore dismissal) and continues
through five stops — work items, runners, chat, workpads, pages — each
with an illustration, Back/Next stepping, a sidebar stop navigator, and
an always-available close; the last stop ends in a project-creation
action that completes the tour and opens the create-project dialog. Any
completion or dismissal writes `PATCH /api/users/me/tour-completed/`
with `{is_tour_completed: true}`. This is a cloud-edition row; the gate
itself is edition-independent and proven web-observably, while any
cloud-overlay-only tour content present in other checkouts is outside
what the oracle exercises here. The home-surface seam (owned by
NEWFRONT-70) is recorded in the seams section: the auth slice owns the
overlay plus the flag wiring plus the registration seam, not home UI.

## Target behavior: desktop onboarding without the CLI step (AUTH-030/031/043)

Inventory acceptance text (amended by NEWFRONT-156, target per main PR
494): browser builds show the CLI-install step with per-OS guidance and
both continue actions advancing identically with nothing stored; desktop
builds omit the step entirely and onboarding starts at profile setup;
on desktop the profile step is first so it offers no back; elsewhere
back moves step to step per the visible sequence except on the first and
last steps, and the single URL stays step-unaware.

The cited PR's diff mechanics, restated precisely: it introduces a
per-edition flag (true on web, false on desktop, justified because the
desktop app bundles the CLI) that drives three flow decisions. The
initial step is CLI install when the flag is true and profile setup when
false. The previous-step lookup returns CLI install from profile setup
when the flag is true and none when false; all other previous-step edges
(role to profile, use-case to role, workspace to profile on self-managed
and to use-case otherwise, none from the invite step) are unchanged. The
progress order includes CLI install only when the flag is true, so
desktop progress counts shift down by one. Back availability is exactly
"a previous step exists". The PR covers both flags with unit tests
(browser keeps CLI install first with profile stepping back to it;
desktop starts at profile with no previous step and the CLI step absent
from the order). The live old app still shows the pre-PR behavior (CLI
step present on desktop): slices build the target, and the area gate
aligns the desktop legs of these scenarios to it.

## Cross-epic seams

Each seam below names what the auth slice builds versus what a later
epic registers or reuses, so every row stays provable at gate time
without depending on Backlog epics. In each case the gate drives the
auth-owned surface or contract directly; the later epic only adds
trigger placement or host UI.

- AUTH-017 sign-out entries (shell surfaces owned by NEWFRONT-70). The
  auth slice owns the sign-out call path end to end — fresh-CSRF sign-out
  POST, agent-runtime disposal, store and cache clearing, desktop
  teardown-seam invocation in order, failure toasts, landing — plus a
  registration seam (a sign-out command/action later surfaces invoke).
  NEWFRONT-70 registers that seam into the full sidebar user menu,
  workspace menu, and command palette. Gate proof needs no shell UI: the
  palette and menu legs are proven through the auth-owned seam contract
  and the call path, and the F-06 shell already exposes both entries.
- AUTH-018 switch-account control (shell and profile surfaces). The auth
  slice owns the dialog (active-account naming, confirm-into-sign-out
  flow, theme reset, failure toast) and its call path. Later epics place
  the trigger on their surfaces. Gate proof drives the dialog plus the
  API path directly.
- AUTH-019 deactivation dialog (profile settings owned by NEWFRONT-90).
  Dialog-versus-API split: NEWFRONT-90 owns the settings-page placement
  and trigger; the auth slice owns the dialog component, the `DELETE
  /api/users/me/` call, and the post-flow (toast, sign-out, landing).
  Gate proof exercises the dialog contract and the API path without the
  settings page.
- AUTH-024 project join plus permission-store feeds (issues and projects
  areas). The auth slice owns the denial screens, the join call
  (`POST /api/users/me/workspaces/{slug}/projects/invitations/`), and
  the feed readers (own membership, per-project roles, project member
  info) behind the permission store. Later areas reuse the feeds and the
  join path for their own surfaces without forking them.
- AUTH-042 tour overlay on workspace home (home surface owned by
  NEWFRONT-70; cloud edition). The auth slice owns the overlay, the
  `is_tour_completed` flag wiring, and the registration seam the shell
  mounts on the home route; it does not build home UI. Gate proof mounts
  the overlay through the seam on a home harness and asserts the flag
  write plus the stop flow.
- AUTH-022/023 desktop seams (desktop epic NEWFRONT-77). The auth slice
  owns the web-side contract: the `?error=` / `?error_description=`
  alert landing, the never-autostart rule, and the unavailable card
  behind `platform.openExternal` (browser opening) — all desktop-gated.
  NEWFRONT-77 owns the shell halves: deep-link subscription and
  exchange navigation plus the Tauri `openExternal` implementation and
  the webview-wipe command (whose sign-out leg is already oracle green
  under DESK-010). Gate proof for the auth halves runs against the
  desktop build through the platform seams.
- AUTH-040 standalone create-workspace entry points (shell switcher
  seam). The auth slice owns the route, the shared create unit (built
  once and mounted both in onboarding and standalone — not copied), and
  the entry contract; the shell switcher later links to the route.
  Gate proof opens the route directly.
- AUTH-017 desktop webview-wipe clause overlap with DESK-010. The wipe
  command and its desktop proof live under DESK-010 (desktop epic,
  already oracle green); the auth row's clause is the call-site
  obligation (invoke teardown after the sign-out POST, before landing,
  swallowing wipe failures). No double implementation: auth calls the
  seam, desktop owns it.

## F-06 reuse map (new code survey)

NEWFRONT-17 built the first vertical slice in `apps/web_new`. Per
auth slice, what exists versus what must be added or reworked (all
paths under `apps/web_new/src` unless noted):

- Shared foundations (used by every slice). Exist: `core/session`
  queries (`me`, workspaces, workspace selection, role selection, pure
  route-gate deciders), store (sign-in lifecycle plus return URL,
  sign-out reset registry), hooks (`useMe`, `useWorkspaces`,
  `useWorkspace`, `usePermissions`, `useSessionStatus`), middleware
  (401 expires the session keeping the return URL; wired into the app
  client), and permissions (admin 20 / member 15 / guest 5). Exist in
  `@pidash/api-client`: CSRF fetch, credential sign-in, email check,
  code generate, code sign-in, sign-out, `getMe`, `getMeSettings`
  (last/fallback workspace slugs), `getProfile` / `updateProfile`,
  workspace listing. Exist in kit: button, input, dialog, menu, popover,
  toast, tooltip, spinner, and related primitives. To add: the
  contracts issues' endpoints (NEWFRONT-480/481), the onboarding funnel
  and set-password guards, last/fallback-validated landing, member and
  project-role feed readers, and the platform deep-link subscription
  (the Tauri platform implements browser opening but no deep-link
  listener yet).
- S1 sign-in card (AUTH-001–008, NEWFRONT-482). Exists: `SignInCard`
  with email/password/code steps, banner-with-step-recovery,
  resend-with-cooldown, return-path plumbing, and `error_code`
  preloading; `authErrors` mapping codes to banners and steps;
  `routes/sign-in.tsx` with safe internal-only return paths and a
  signed-in redirect. To add or rework: the sign-up mode flip for
  unknown addresses (F-06 banners instead), provider buttons resolved
  through the edition (the OSS edition carries no providers; the slot
  exists), the invitation-aware header fetch, the no-methods notice,
  the mail-less mode (reset explanation, hidden code login, relabeled
  primary), the locked-email-with-clear presentation, the
  forgot-password entry, the full numeric error-code matrix (F-06 keys
  symbolic names while the server sends numeric `error_code`s — the
  mapping must key on what the server sends), the five-second resend
  cooldown (F-06 uses thirty), and last/fallback-validated plus
  onboarding-aware landing (F-06 lands on the first workspace and
  ignores onboarding state).
- S2 sign-up card plus entry URLs (AUTH-009–011, 044, NEWFRONT-483).
  Exists: the sign-in card's step, banner, and return-path conventions
  to reuse (not fork). To add: the sign-up card with confirmation,
  strength gating, and code path; the legacy-URL forwards (sign-up
  addresses with prefill and return-path survival; sign-in addresses as
  bare redirects); plus the missing `@AUTH-044` oracle scenario.
- S3 recovery plus set-password plus session expiry (AUTH-012–014, 020,
  NEWFRONT-484). Exists: the CSRF plumbing, the 401-expired pipeline
  with return-URL keeping, and the banner conventions. To add: the
  forgot/reset/set-password pages with their countdowns, gates, and
  single-use/invalid-link states; per-submission CSRF freshness on the
  new forms. (The expired-to-sign-in navigation with return path already
  exists as the AppShell effect; S3 keeps it, extending coverage to any
  new authenticated surface that needs it.)
- S4 guards, landing, sign-out, switching, deactivation (AUTH-015–019,
  NEWFRONT-485). Exists: the `$ws` membership gate with its not-found
  notice, the `_gated` sign-in gate with return-path bounce, the shell
  with palette and sidebar sign-out entries wired to `useSignOut`
  (server best-effort plus full client clearing), and the expired-state
  redirect wiring. To add or rework: the five page-type guards
  (public, signed-out-only, onboarding funnel, set-password,
  signed-in-only) as extensions of the `_gated` framework rather than a
  second framework; AUTH-016 landing (safe return path, validated
  last/fallback workspace, creation fallback); the desktop teardown
  call around sign-out; the switch dialog and its call path; the
  deactivation dialog and its call path; the sign-out registration seam
  for later shell surfaces.
- S5 device approval, desktop sign-in, denials, focus (AUTH-021–025,
  NEWFRONT-486). Exists: `platform.openExternal` on both targets, the
  `deepLinks` platform contract (no Tauri listener yet), the `$ws`
  not-found notice, workspace roles in the session module, and the kit
  form primitives. To add: the device-approval page with its format,
  prefill, auto-attempt, and confirmation rules; the desktop error
  landing and unavailable card behind the platform seams; the
  Case-A/Case-B workspace screens and the project join/no-access/
  not-found screens with their feed readers and join path inside the
  S4 guard framework; the area-wide focus/keyboard conventions in the
  shared auth feature or kit; plus the missing `@AUTH-022`/`@AUTH-023`
  oracle scenarios.
- S6 invitation inbox plus single-invite link plus landing (AUTH-026–028,
  NEWFRONT-487). Exists: last-workspace stamping via `updateProfile`,
  the signed-in-only and public guard types from S4, and S4 landing.
  To add: the inbox page (selectable cards, batch accept, empty state),
  the public single-invite page with its pending/stale states, and the
  AUTH-028 landing rules — with reusable data hooks the onboarding
  workspace step shares.
- S7 onboarding shell plus profile, role, use-case steps (AUTH-029–033,
  043, NEWFRONT-488). Exists: the onboarding funnel guard type from S4
  and the profile/user-update contracts. To add: the single-URL shell
  (step indicator, header with chevron and account menu, scroll
  handling), the CLI-install, profile, role, and use-case steps with
  their validation, storage, skip, and self-managed rules, the resume
  logic, and the main-PR-494 desktop target (no CLI step, profile
  first with no back) behind the platform kind.
- S8 onboarding workspace step plus invite-members plus finish
  (AUTH-034–039, NEWFRONT-489). Exists: the S7 shell, the S6 invitation
  contracts, and the finish primitives (profile stamp, onboard marker,
  refresh). To add: the four workspace sub-views with their precedence
  and transitions, the reusable create unit (fields, checks, sizes)
  that S9 mounts with full-page chrome, the invite-members step with
  its row, gating, and defer rules, the solo shortcut, and the finish
  routine with AUTH-016 landing.
- S9 standalone create-workspace plus disabled states plus tour
  (AUTH-040–042, NEWFRONT-490). Exists: the S8 create unit to mount
  (not copy), the instance-config contract from NEWFRONT-480, and the
  tour-completed flag on the profile contract. To add: the standalone
  route with full-page chrome and dropdown size picker, the disabled
  states (admin-only screen; in-onboarding notice), and the tour
  overlay with its welcome, stops, completion, and registration seam.

## Server contract index

Every endpoint this area uses, with the rows that need it (contracts
issues NEWFRONT-480/481 own the zod shapes; the shapes below are the
observed call sites):

- `GET /auth/get-csrf-token/` → `{csrf_token}`: fresh token per
  credential submission (AUTH-001, AUTH-002, AUTH-009, AUTH-013,
  AUTH-017, AUTH-020, AUTH-031).
- `POST /auth/email-check/` `{email}` → `{existing, status}` with
  status `CREDENTIAL` or `MAGIC_CODE`: email-first routing (AUTH-001,
  AUTH-002, AUTH-004, AUTH-009).
- `POST /auth/sign-in/` native form (`email`, `password`,
  `csrfmiddlewaretoken`, `next_path?`): credential sign-in (AUTH-001).
- `POST /auth/magic-generate/` `{email}`: code generation (AUTH-002,
  AUTH-009).
- `POST /auth/magic-sign-in/` native form (`email`, `code`,
  `csrfmiddlewaretoken`, `next_path?`): code sign-in (AUTH-002).
- `POST /auth/sign-up/` native form (`email`, `password`,
  `confirm_password`, `csrfmiddlewaretoken`, `next_path?`): password
  account creation (AUTH-009).
- `POST /auth/magic-sign-up/` native form (same code fields): code
  account creation (AUTH-009).
- Full-page navigation to `/auth/{google,github,gitlab,gitea}/`
  (`next_path?`): provider flows (AUTH-003).
- `POST /auth/forgot-password/` `{email}`: reset-link request
  (AUTH-012).
- `POST /auth/reset-password/{uidb64}/{token}/` native form
  (`password`, `confirm_password`, `csrfmiddlewaretoken`): password
  reset (AUTH-013).
- `POST /auth/set-password/` `{password}` under `X-CSRFTOKEN`: first
  password (AUTH-014, AUTH-031).
- `POST /auth/sign-out/` with fresh CSRF: session end (AUTH-017).
- `POST /api/v1/auth/device/start/` → `{user_code}`;
  `POST /api/v1/auth/device/approve/` `{user_code}` →
  `{user_email, workspace_slug?}`: CLI device flow (AUTH-021).
- Webview navigation to `/api/auth/desktop-exchange/` with
  `code`/`state`: desktop session completion (AUTH-022).
- `GET /api/users/me/workspaces/invitations/`; `POST` same path with
  `{invitations: [ids]}`: invitation list and batch accept (AUTH-026,
  AUTH-034).
- `GET /api/workspaces/{slug}/invitations/{id}/join/`;
  `POST` same path with `{accepted, token}`: single-invitation fetch
  and answer (AUTH-005, AUTH-027).
- `GET /api/users/me/workspaces/join-requests/`;
  `POST` same path with `{admin_email}`: join-request list and create
  (AUTH-034, AUTH-036).
- `GET /api/workspace-slug-check/?slug={slug}` → availability status:
  slug check (AUTH-034, AUTH-035, AUTH-040).
- `POST /api/workspaces/` (name, slug, `organization_size`):
  workspace creation (AUTH-034, AUTH-038, AUTH-040).
- `POST /api/workspaces/{slug}/invitations/`
  `{emails: [{email, role}]}`: bulk invites (AUTH-037).
- `PATCH /api/users/me/` (name, avatar fields): user update (AUTH-031).
- `PATCH /api/users/me/profile/` (role, `use_case`, consent,
  progress flags, `last_workspace_id`): profile update (AUTH-016,
  AUTH-028, AUTH-031, AUTH-032, AUTH-033, AUTH-034, AUTH-039, AUTH-040).
- `PATCH /api/users/me/onboard/` `{is_onboarded: true}`: onboard
  marker (AUTH-039).
- `PATCH /api/users/me/tour-completed/`
  `{is_tour_completed: true}`: tour flag (AUTH-042).
- `GET /api/users/me/settings/` (workspace `last_workspace_slug`,
  `fallback_workspace_slug`): landing inputs (AUTH-016).
- `DELETE /api/users/me/`: account deactivation (AUTH-019).
- `GET /api/workspaces/{slug}/workspace-members/me/`: own membership
  (AUTH-024).
- `GET /api/users/me/workspaces/{slug}/project-roles/`: per-project
  roles (AUTH-024).
- `POST /api/users/me/workspaces/{slug}/projects/invitations/`
  `{project_ids}`: project join (AUTH-024).
- `GET /api/instances/` → `{instance, config}`: instance flags
  (AUTH-003, AUTH-007, AUTH-008, AUTH-011 cross-link, AUTH-032,
  AUTH-033, AUTH-041).
- `GET /api/workspaces/{slug}/members/`: membership rows for
  verification reads (AUTH-027 server checks).

Observed-but-dead endpoints carry no rows: `GET
/api/users/me/instance-admin/` has no caller in the surveyed web, admin,
or space trees. The standalone `SetPasswordForm` component and the older
full-page onboarding variants are unmounted; the single-invitation
"not found" branch is effectively unreachable (see AUTH-027).





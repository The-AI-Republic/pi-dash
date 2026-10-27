# Admin area spec (NEWFRONT-501)

Prose spec for inventory rows ADM-001–080 (area epic NEWFRONT-98):
instance-admin console (god-mode) — first-run setup, admin sign-in and
session, and the dashboard settings pages (general, email, authentication
providers, AI, loop, workspaces, runners) plus the cloud entry gate. New
home is the route subtree `/god-mode/*` in `apps/web_new`, web build
only. Written from reading the old sources listed below plus the nine
oracle spec files; no old code, strings, styles, or assets are reused.
New code is written from this spec.

Old sources consulted (reference only, never a source): the `(home)`
route tree (root gate, sign-in form, banners, layouts), the
`(dashboard)` route tree (shell plus general, email, authentication
landing and the four provider pages, AI, image, loop list/modal/detail,
workspace list and create form), `components/` (instance, workspace,
authentication, common), `store/` (instance, user, theme, workspace,
root), `hooks/` (store bindings, OAuth registry, sidebar menu),
`helpers/authentication.ts`, `providers/` (instance, user, store, toast),
`app/routes.ts`, the shared service and type packages for API shapes
only, the backend admin/auth views for redirect and error-code behavior,
and the single cloud override file
`private-pi-dash/ee-overlay/apps/admin/app/(all)/(home)/page.tsx`. All
of these are Plane code: nothing is ported (see `§0.7`). Oracle
behavior references: `apps/web_new/e2e/parity/admin/entry-session.spec.ts`
(NEWFRONT-441), `general-email.spec.ts` (NEWFRONT-442),
`authentication.spec.ts` (NEWFRONT-443), `workspaces.spec.ts` and
`ai-image.spec.ts` (NEWFRONT-444), `loop.spec.ts` and `runners.spec.ts`
(NEWFRONT-445), `shell.spec.ts` (NEWFRONT-445), `cloud-entry.spec.ts`
(NEWFRONT-446).

Conventions used below:

- Screens, states, rules, and API calls are described in prose. The only
  code-like tokens are API paths, query and field names, error codes,
  config keys, and short enum values — never control flow, markup,
  styles, or copy text.
- User-visible wording is described by gist (for example "a banner
  conveying authentication failure"). New copy must be written fresh.
  Values an oracle scenario pins exactly (option values, error codes,
  config keys, URL shapes) are marked below as oracle-pinned so slices
  can see the constraint. Where a pinned value conflicts with the
  copying ban, the slice raises a `process:` comment on NEWFRONT-1
  instead of working around it.
- "Instance info" means the `instance` object of `GET /api/instances/`
  (setup flag, names, telemetry flag, workspace-existence flag, version
  fields). "Instance config" means the `config` object of the same
  response. "Configurations" means the key/value list from
  `GET /api/instances/configurations/`, where every value is a string
  and boolean flags are encoded as `"1"`/`"0"`.
- Eleven oracle scenarios pin live old-app bugs or pre-target behavior
  with `bug:` markers; the intended behavior each slice must build is
  recorded in the row's section: ADM-006 (NEWFRONT-457, setup redirect
  handling), ADM-008 (NEWFRONT-458, sign-in banner link target), ADM-013
  (NEWFRONT-475, empty name feedback), ADM-018 (NEWFRONT-476, failed
  disable reporting), ADM-020 (NEWFRONT-478, security-picker dirtiness),
  ADM-021 (NEWFRONT-477, test-email scope), ADM-030 (NEWFRONT-492, Gitea
  help link), ADM-049/050/062 (NEWFRONT-493, image page removal),
  ADM-062/080 (NEWFRONT-494, runners page addition), ADM-076/077/079
  (NEWFRONT-496, cloud gate classification). The area gate adjudicates
  fix-versus-pin.
- Five rows describe target behavior that differs from the live old app
  per NEWFRONT-156: ADM-049, ADM-050, ADM-062 (image removal, main#523)
  and ADM-062, ADM-080 (runners addition, main#534). The live old app
  still shows the pre-PR behavior; the new app builds the target.
- Three behaviors are accepted OSS quirks the new app preserves:
  log-only save failures (ADM-017, ADM-019, ADM-035, ADM-046), loop
  delete with no confirmation (ADM-060, ADM-073), and no workspace
  delete (ADM-042, ADM-073).

## 0. Cross-area seams

### 0.1 The `/god-mode/*` subtree, its layout, and its session

The console lives under the `/god-mode/` path prefix in `apps/web_new`
(Architecture page; D13). It has its own layout that is distinct from
the main workspace shell: its own sidebar, header, and entry gate. No
god-mode route renders inside the main app shell, and no main-app route
renders inside the god-mode layout. The subtree is mounted in the web
build only (see `§0.2`); the desktop build never mounts it (ADM-072).

The console authenticates through its own instance-admin session, driven
by `GET /api/instances/admins/me/`. This session is separate from the
member app session (see `§0.3`): the two sessions use different cookies,
so a signed-in workspace member who is not an instance admin reads as
anonymous inside god-mode, and an instance-admin session does not sign
the visitor into the main app. Sign-out of one session leaves the other
untouched.

Route map inside the subtree (old console paths preserved under the new
prefix; trailing-slash form is canonical):

- `/god-mode/` — entry gate: OSS setup/sign-in branch selector (ADM-001)
  or, in the cloud edition, the authorization-over-OIDC resolver
  (ADM-074). No email/password form and no setup form exist in cloud.
- `/god-mode/general/` — general settings (ADM-013–017). This is the
  landing page after sign-in, setup, and every guard redirect, and the
  target of the not-found shortcut.
- `/god-mode/email/` — email settings (ADM-018–022).
- `/god-mode/authentication/` — auth landing (ADM-023–026, ADM-036).
- `/god-mode/authentication/<provider>/` — one provider page each for
  `github`, `gitlab`, `gitea`, `google` (ADM-027–035). No other provider
  segment exists.
- `/god-mode/workspace/` — workspace list (ADM-037–042).
- `/god-mode/workspace/create/` — create-workspace form (ADM-043–045).
- `/god-mode/ai/` — AI settings (ADM-046–048).
- `/god-mode/loop/` — loop job list (ADM-051–055).
- `/god-mode/loop/<jobId>/` — loop job detail (ADM-056–061, ADM-073).
- `/god-mode/runners/` — runners page (ADM-080, target per main#534).
- There is no `/god-mode/image/` route (ADM-049/050, target per
  main#523): the path falls through to the not-found screen like any
  other unmatched path.
- Any other unmatched path under the mount renders the catch-all
  not-found screen (ADM-011), which sits outside both guarded layouts
  and therefore renders for signed-in and signed-out visitors alike.

### 0.2 Web-only mount (D13, ADM-072)

The god-mode subtree ships only in the web build. The desktop build
contains no admin tree and references no admin mount: there is no way to
reach these screens from the desktop app, and no desktop-specific
affordance (title-bar, native menu, deep link, updater hook) exists on
any god-mode screen. Slices gate the subtree on the platform the same
way existing web-only/desktop-only routes do (see the reuse map,
`§12.4`); they do not invent a second gating mechanism. The area gate
proves absence on desktop per the ADM-072 scenario.

### 0.3 Relation to the F-06 shell/session (NEWFRONT-17)

F-06 built the member shell and the member session (`core/session`:
member identity, workspace list, profile, permissions, page guards, and
the sign-in/out flows). God-mode relates to it as follows:

- Separate auth state. God-mode auth state comes only from the admin
  check `GET /api/instances/admins/me/`; it never reads the member
  session (`GET /api/auth/me/`, member stores, F-06 guards) except for
  the cloud probe, which is a one-shot classification read, not a
  session (ADM-076–079). A member app session cookie is meaningless to
  the admin check: anonymous reads answer 401, and a member session
  reads as anonymous. A 403 from the admin check arises only when a
  previously granted admin grant is revoked mid-session (per the
  ADM-009 oracle note).
- Separate guard framework. The god-mode guards (home tree bounces a
  signed-in admin to general settings; dashboard tree bounces a
  signed-out visitor to the root; both replace history and show a
  loader while the tri-state is unresolved, ADM-009) are god-mode's
  own, driven by the admin check. They do not reuse the F-06 page-gate
  deciders, whose inputs (onboarding flags, workspace membership,
  member landing) do not exist in this area. Shared pure helpers
  (return-path parsing, safe-path checks) may be reused where they fit
  without dragging member semantics along.
- Separate sign-out. Admin sign-out posts to
  `/api/instances/admins/sign-out/` and resets admin-side client state
  (theme back to system, fresh stores, cleared admin identity, ADM-010).
  It does not call the member sign-out path, and member session state
  is out of scope for the reset.
- Separate query subtree. Admin server data lives under its own query
  key root (an admin root, not the `["ws", ws]` member root and not the
  `["session", …]` member-session root), so member sign-out and
  workspace switches neither clear nor leak admin data, and admin
  sign-out clears exactly the admin subtree plus the registered admin
  UI stores. The contracts slices (NEWFRONT-502/503) own the key
  factory; page slices only consume it.
- No shared shell chrome. The console shares nothing with the main
  shell sidebar or command palette: its sidebar, header, breadcrumbs,
  and menus are god-mode's own components (ADM-062–068). Slices do not
  import the main shell.

### 0.4 Cross-app links via the F-09 coexistence helper (NEWFRONT-20)

Three behaviors link out of the console into the main web app, and all
three go through the F-09 coexistence helper (`CrossAppLink` /
`resolveLinkKind` over `migrated-routes.json`), never through hardcoded
anchors to one app or the other:

- Workspace rows deep-link to the main app at the workspace slug in a
  new tab (ADM-039).
- The sidebar help control opens the main web app root (ADM-067).
- The create-workspace form prefixes the slug field with the web base
  URL (ADM-043), which is the same origin the helper assumes.

Rationale: during migration the main-app screens these links target may
live in either app; the helper resolves router navigation versus a
plain full-page anchor from the migrated-route list, so the links flip
automatically when their targets move. The admin entries target the
old-app/main-app paths (workspace slug home, site root), which the
helper maps correctly in both directions. External documentation, forum,
bug-report, provider-console, and policy links are ordinary external
links, not coexistence links.

### 0.5 The native full-page auth POST mechanism

Admin sign-up, sign-in, and sign-out are native full-page form posts to
the API host carrying a hidden `csrfmiddlewaretoken` field whose value
comes from `GET /auth/get-csrf-token/`. The server answers with a
redirect: success lands on the dashboard; failures redirect back to the
entry with `error_code`, `error_message`, and echoed-field query params
that the forms read (ADM-004–008, ADM-010). Slices preserve the
full-page round trip (including the in-flight busy render and the fact
that no client code runs between submit and the committed response)
rather than converting these three calls to background requests: the
redirect-with-query-params contract and the session-cookie establishment
are observable behavior the oracle pins. The CSRF fetch is the only
background request in these flows.

### 0.6 Configuration value encoding and the shared fetch

Every configuration value is a string; boolean flags are encoded as
`"1"`/`"0"` (for example `ENABLE_SMTP`, `ENABLE_SIGNUP`,
`IS_GITHUB_ENABLED`, `EMAIL_USE_TLS`). Toggles parse with the same
rule the old app uses (numeric parse, nonzero is on, missing is off),
and saves write back `"1"`/`"0"` strings. The sync toggles on provider
forms are string flags too (`ENABLE_<PROVIDER>_SYNC`, defaulting to
`"0"` when unset).

The configurations list is fetched once and shared by every settings
page (ADM-069): pages that need it read from the same cached query
rather than issuing their own fetches, and the cache refreshes only via
explicit revalidation after a mutation. There is no polling, no
websocket, and no focus-triggered refetch for admin data (ADM-069).
Slices map the old SWR keys (`INSTANCE_CONFIGURATIONS`,
`INSTANCE_DETAILS`, `CURRENT_USER`, `INSTANCE_ADMINS`,
`INSTANCE_WORKSPACES`, `INSTANCE_LOOP_JOBS`, …) onto the new query key
factory; the key names themselves are not behavior and are not
preserved.

### 0.7 Portability: all Plane code, nothing ported

Every old path this area reads is Plane code: the whole `apps/admin`
tree, the cloud `ee-overlay/apps/admin` override, and the shared
service/type/constant/helper modules the admin code uses (instance,
workspace, loop, auth, validation, constants). The "your own code"
exception on the Reference page does not cover any of them: the only
superficially adjacent portable path is
`packages/services/src/auto-pm/**`, which is the end-user auto-PM
client (`/api/users/me/auto-pm/…`), is not imported by any admin
screen, and is unrelated to the instance loop endpoints. The admin loop
UI and `packages/services/src/instance/loop.service.ts` are Plane code
and stay reference-only. Slices write every god-mode module from this
spec; the similarity check and import-ban lint apply in full.

### 0.8 Edition seam for the cloud entry

The OSS root (setup/sign-in branch selector, ADM-001) and the cloud
root (authorization-over-OIDC resolver, ADM-074–079) are different
screens behind the same `/god-mode/` path. The dashboard settings pages
are identical in both editions; only the entry gate differs. The new app
selects the entry screen through `core/edition` (the typed edition
interface that replaces file swapping), following the existing slot
precedent (`desktopSignInCard`): the cloud slice (NEWFRONT-519)
proposes the exact seam (a named slot or route extension) in its PR and
records it on the Decisions page, since adding an edition slot is a
design decision. The OSS build must contain no OIDC bounce, no probe,
and no denied screen; the cloud build must contain no password form and
no setup form (ADM-074 pins zero password/setup fields in every cloud
visit).

## 1. Entry and session (ADM-001–012)

Slice owner: NEWFRONT-504 (ADM-001–008), NEWFRONT-505 (ADM-009–012),
NEWFRONT-519 (cloud entry, ADM-074–079 — see `§10`).

### 1.1 Bootstrap gate (ADM-001)

On OSS, the root URL resolves to exactly one of four screens from a
single instance-info fetch (`GET /api/instances/`, issued once, with no
automatic retry and no refetch on window focus):

- While instance state is unresolved, a full-screen loader (ADM-002).
- When the fetch errors, the failure screen with manual retry (ADM-003).
- When the instance reports setup not done, the first-run setup form
  (ADM-004–006).
- When the instance reports setup done, the admin sign-in form
  (ADM-007–008).

The branch key is the `is_setup_done` flag on the instance info. The
cloud edition replaces this whole screen (ADM-074); the
setup-vs-sign-in branching is OSS-only.

### 1.2 Brand loader (ADM-002)

A centered pulsing brand spinner covers the full screen only while
state is genuinely unknown: while instance info is unresolved on the
home tree, and while the login tri-state is unresolved on the dashboard
tree (ADM-009). It never renders alongside a form or the shell, so no
wrong screen flashes first. In cloud the same spinner covers the
authorized-admin redirect window and the probing window (ADM-075,
ADM-076).

### 1.3 Fetch-failure screen (ADM-003)

When the instance-info fetch errors on OSS, a theme-aware illustration
screen explains that the instance details could not be loaded and
suggests a connectivity cause. Its retry control performs a full page
reload, which re-issues the single info fetch. There is no silent
automatic retry and no retry counter. The cloud root never renders this
screen; transient cloud failures surface the ADM-078 retry screen
instead.

### 1.4 First-run setup form (ADM-004)

On an un-set-up OSS instance, the root shows the instance-admin
creation form: the first user becomes the instance admin. Fields are
first name, last name, email, company, password, confirm-password, and
a telemetry opt-in checkbox (checked by default) with an external
further-reading link. Submit is a native full-page POST to
`/api/instances/admins/sign-up/` (see `§0.5`) carrying the fields under
the names `first_name`, `last_name`, `email`, `company_name`,
`password`, `confirm_password`, plus `is_telemetry_enabled` as the
strings `"True"`/`"False"` and the hidden `csrfmiddlewaretoken`. The
server creates the admin, marks the instance set up, stores the company
on the admin profile, adopts the company as the initial instance name,
applies the telemetry flag, establishes the admin session, and
redirects to general settings.

### 1.5 Setup validation (ADM-005)

Client-side, before submit:

- Submit is disabled until first name, email, and password are all
  non-empty, the password reaches the valid-strength level, and
  confirm-password matches. Last name and company do not gate submit.
- Valid-strength means at least 8 characters with an uppercase letter, a
  lowercase letter, a digit, and a special character.
- Name fields reject invalid keystrokes as typed (letters, spaces,
  hyphens, and apostrophes only) and cap input length at 50 characters;
  the company field rejects invalid keystrokes (letters, numbers,
  spaces, hyphens, underscores) and caps at 80. Rejected keystrokes are
  swallowed, leaving the field unchanged.
- A strength meter renders while the password field is focused; a
  mismatch hint renders under confirm-password once it is non-empty and
  differs, shown when the field is not focused or once its length
  reaches the password's length.
- First name is auto-focused. Both password fields have independent
  show/hide toggles that are kept out of tab order (oracle-pinned:
  tabindex -1).

No server request fires for validation alone.

### 1.6 Setup prefill and server-error surfacing (ADM-006)

After a server redirect back to the root, the setup form prefills first
name, last name, company, and email from the query params `first_name`,
`last_name`, `company`, and `email`. Telemetry stays on unless the
`is_telemetry_enabled` param is explicitly the string `"False"`.

Server error codes round-tripped as `error_code` (with the human
message in `error_message`) render inline under the email field for the
invalid-email case, inline under the password field for the
invalid-password case, and as a top error banner for the remaining
named cases (instance-not-configured, admin-exists, missing-fields,
user-exists). Unknown codes render nothing.

Intended behavior behind NEWFRONT-457 (the slices build this, not the
observed old behavior): the server actually sends numeric codes in
`error_code` (the 5150–5190 admin series, plus the weak-password code
when the server-side strength check rejects) and echoes the company
under `company_name`, while the old form matches only named codes and
reads only `company` — so real rejected posts resurface as a bare form
with no banner, no inline error, and a dropped company. The new form
matches the numeric codes the server sends (mapping each to the same
inline-or-banner placement a crafted named code gets) and prefills the
company from both echo spellings, so every rejected post shows its
failure where crafted-code failures show and no echoed field is lost.
A rejection writes nothing server-side: setup stays undone and the
admin check still answers 401.

### 1.7 Admin sign-in form (ADM-007)

On a set-up OSS instance, the root shows the admin sign-in form: email
and password fields with a show/hide toggle on the password. Submit is
disabled until both fields are non-empty and shows a busy indicator
while the round trip is in flight. Email is auto-focused and prefills
from the `email` query param. Submit is a native full-page POST to
`/api/instances/admins/sign-in/` (see `§0.5`) carrying `email`,
`password`, and the hidden `csrfmiddlewaretoken`. Success establishes
the admin session cookie and lands on general settings. The in-flight
busy render is unobservable to the oracle (a pending native post stalls
the frame until the response commits), so the scenario pins the
round-trip outcome, not the spinner frames.

### 1.8 Sign-in error surfacing (ADM-008)

After a failed sign-in redirect, two banners can render from the
`error_code`/`error_message` query params:

- A generic top error banner renders when the code is one of the five
  recognized sign-in codes (instance-not-configured,
  required-email-password, invalid-email, user-does-not-exist,
  authentication-failed) and a message is present; it shows the server
  message verbatim.
- A mapped dismissible info banner renders for the nine numeric admin
  auth codes (oracle-pinned values 5150, 5155, 5160, 5165, 5170, 5175,
  5180, 5185, 5190), each mapped client-side to a title and human-
  readable message. Two mappings (admin-user-already-exists,
  admin-user-does-not-exist) carry a link back to the sign-in entry;
  the deactivated-account mapping references the support contact.
  Dismissing clears the banner for the visit.

The generic banner takes precedence when both match. Unknown codes
render no banner. Note the asymmetry the oracle pins: real server
round trips surface only through the mapped banner, because the server
sends numeric codes the generic path never matches.

Intended behavior behind NEWFRONT-458: the old mapped-banner link
targets a path that resolves to a nonexistent route under the console
mount, landing on the not-found screen. The new banner links back to
the sign-in entry (`/god-mode/`).

Backend edge cases the sign-in slice must preserve: a valid password
for a user who is not an instance admin fails exactly like a wrong
password (same code); the deactivated-account redirect echoes no
email, so the form shows no prefill on that path; missing email or
password, malformed email, and unknown account each echo the submitted
email for prefill.

### 1.9 Route guards (ADM-009)

Both layouts guard on the admin login tri-state driven by
`GET /api/instances/admins/me/` (no retry; a 403 maps to
"authentication not done"):

- An authenticated admin visiting the home/sign-in tree is
  replace-navigated to general settings.
- An unauthenticated visitor on any dashboard route is
  replace-navigated to the root.
- While auth state is unresolved, the loader shows (ADM-002); the
  guarded content never flashes first.
- Redirects add no history entry (oracle-pinned: back-button neutral).

Session semantics (oracle-pinned): anonymous reads answer 401; a member
app session reads as anonymous because admin paths use a separate admin
cookie; a 403 arises only when the grant is revoked mid-session. The
current-admin fetch also triggers the admins-list fetch that populates
the general page email (ADM-014) and the shell identity (ADM-064).

### 1.10 Sign-out (ADM-010)

The sidebar account menu hosts sign-out as a native form POST to
`/api/instances/admins/sign-out/` with the hidden CSRF field (see
`§0.5`). The server clears the session and redirects to the bare mount
path (the edge redirect normalizes it to the slashed entry). The
client also resets its own state on submit: theme back to system, fresh
stores, cleared admin identity — so the dashboard guard bounces the
user to the root afterwards. The account menu shows the admin email,
and the shell mounts only after the session resolves, so the sign-out
control always renders in the loaded state.

### 1.11 Catch-all not-found screen (ADM-011)

Any unmatched path — under the mount or outside it — renders a
not-found illustration screen with an explanatory message and a single
action navigating to general settings. The catch-all sits outside both
guarded layouts, so it renders for signed-in and signed-out visitors
alike, and it performs no guard redirect of its own.

### 1.12 First-workspace popup (ADM-012)

On the first instance-info load that reports no existing workspaces, a
bottom-corner card invites the admin to create the first workspace,
with a shortcut to the create page and a dismiss action. The flag is
set by the info fetch (the `workspaces_exist` signal), not by any
workspace-list read. Following the shortcut navigates without clearing
the flag; dismissing clears it, and a later truthful reload (instance
holding workspaces) never re-raises it. The card renders at dashboard-
layout level, above page content.

## 2. General settings (ADM-013–017)

Slice owner: NEWFRONT-507. Page route `/god-mode/general/`.

### 2.1 Instance name (ADM-013)

A required text field pre-filled with the current `instance_name`
renames the instance. On save the name persists via
`PATCH /api/instances/` and a success notice shows; a reload shows the
new name. Intended behavior behind NEWFRONT-475: the old form blocks an
empty-name save (no request fires) but renders no message even though
the field carries a required rule. The new form fails validation with
a visible inline required message, so the admin knows why nothing was
saved.

### 2.2 Read-only identity fields (ADM-014, ADM-015)

Two disabled display-only fields sit beside the name: the admin email
(the first entry of `GET /api/instances/admins/`, via its nested user
email) and the instance ID (from the instance info). Neither value is
ever submitted: the save payload carries only the editable fields.

### 2.3 Telemetry toggle (ADM-016)

A switch with explanatory copy (no personal data collected) and an
external policy link controls `is_telemetry_enabled`. Flipping the
switch writes nothing; the value persists only when the shared save
action is pressed. The switch is disabled while a save is in flight. A
missing value is treated as off (the unset branch is unreachable in
practice because the flag is a non-nullable boolean).

### 2.4 Shared save (ADM-017)

One primary save commits the name and the telemetry flag together in a
single `PATCH /api/instances/` carrying `instance_name` and
`is_telemetry_enabled`. It shows a busy label while in flight and a
success notice on completion. On failure only a console log records
the error — no visible notice appears and the server is untouched.
This log-only failure is an accepted OSS quirk the new app preserves.

## 3. Email / SMTP settings (ADM-018–022)

Slice owner: NEWFRONT-508. Page route `/god-mode/email/`. Config keys
involved: `ENABLE_SMTP`, `EMAIL_HOST`, `EMAIL_PORT`, `EMAIL_FROM`,
`EMAIL_HOST_USER`, `EMAIL_HOST_PASSWORD`, `EMAIL_USE_TLS`,
`EMAIL_USE_SSL`.

### 3.1 Master enable toggle (ADM-018)

The page header carries the master email toggle, whose initial state
derives from the stored `ENABLE_SMTP` flag (`"1"` is on). Turning off
calls `DELETE /api/instances/configurations/disable-email-feature/`,
then shows a success or error notice and hides the config form. The
client blanks its local mail values optimistically
(`EMAIL_HOST`, `EMAIL_PORT`, `EMAIL_HOST_USER`,
`EMAIL_HOST_PASSWORD`, `EMAIL_FROM`, `ENABLE_SMTP`) while keeping the
transport-security pair untouched, and restores them if the call fails.
Turning on reveals the config form with no server call; persistence
happens only through the form save (ADM-019), which always forces
`ENABLE_SMTP` back to `"1"`.

Intended behavior behind NEWFRONT-476: the old page reports success
even when the disable call fails (the store swallows the rejection, so
the error notice never renders while the server keeps SMTP enabled).
The new page shows the error notice on a failed disable and leaves the
toggle on with the form visible, so the display matches the server.

### 3.2 SMTP configuration form (ADM-019)

Revealed when email is enabled. Required fields are host, port, and
sender address; an optional authentication section holds username and
password (with show/hide). Save is disabled unless the form is both
valid and dirty, and persists via `PATCH /api/instances/configurations/`
with the mail keys plus `ENABLE_SMTP: "1"`. On success the keys persist
and a success notice shows. On failure only a console log records the
error — an accepted OSS quirk the new app preserves.

### 3.3 Transport-security selector (ADM-020)

A three-way choice (TLS, SSL, none) maps onto the two boolean config
keys `EMAIL_USE_TLS`/`EMAIL_USE_SSL` as the pairs 1/0, 0/1, and 0/0, so
exactly one or neither flag is set. The displayed choice derives from
whichever key is on, defaulting to none, with TLS winning a both-on
tie. Intended behavior behind NEWFRONT-478: the old picker writes the
two flags without marking the form dirty, so a security-only change can
never enable save. The new picker dirties the form, so flipping the
security choice alone enables save and persists on its own.

### 3.4 Send-test-email modal (ADM-021)

A secondary action, enabled whenever the form is valid (no prior save
required), opens a three-step modal: enter a receiver address, then a
success step (advising to check spam and re-verify settings) or a
failure step showing the server's error message. The modal fully resets
its state on close. The check POSTs to
`/api/instances/email-credentials-check/` with the receiver address
under `receiver_email` and sends a real message.

Intent behind NEWFRONT-477: the old check grades the stored SMTP
configuration, not the values currently entered in the form, so an
admin who edits and tests without saving actually tests the previous
saved settings. The new flow grades the entered settings: the check
carries the receiver plus the form's current values (host, port,
sender, username, password and the TLS/SSL/none choice) as candidate
settings — `EMAIL_HOST`, `EMAIL_PORT`, `EMAIL_FROM`,
`EMAIL_HOST_USER`, `EMAIL_HOST_PASSWORD`, `EMAIL_USE_TLS`,
`EMAIL_USE_SSL` alongside `receiver_email`. The endpoint grades the
candidate values it is given and falls back to the stored value for
anything omitted; a blank password also means "keep the stored one",
because stored secrets never read back. So a cleared password field
grades the saved secret, while any other cleared field is sent as
given. The test action therefore opens on any valid form, saved or
not. Requests without candidate fields keep the receiver-only
behavior. Backend contract: NEWFRONT-524.

### 3.5 Loading states (ADM-022)

While configurations load, a pill-shaped skeleton stands in for the
master toggle and a multi-row skeleton stands in for the form; the form
skeleton shows only when the toggle reads on with no configuration yet.
Skeletons never render alongside the real controls. Data comes from the
shared configurations fetch (`GET /api/instances/configurations/`).

## 4. Authentication providers (ADM-023–036)

Slice owners: NEWFRONT-509 (ADM-023–026, ADM-036), NEWFRONT-510
(ADM-027, ADM-032–035), NEWFRONT-511 (ADM-028–031). Landing route
`/god-mode/authentication/`; provider routes
`/god-mode/authentication/<github|gitlab|gitea|google>/`. Enabling a
method here changes the main web app's sign-in options; that
cross-screen effect is the point of the area.

Method enable keys: `ENABLE_MAGIC_LINK_LOGIN` (email magic-link
codes), `ENABLE_EMAIL_PASSWORD` (passwords), `IS_GOOGLE_ENABLED`,
`IS_GITHUB_ENABLED`, `IS_GITLAB_ENABLED`, `IS_GITEA_ENABLED`. The
open-sign-up key `ENABLE_SIGNUP` is separate and is not a method key.

### 4.1 Open-sign-up toggle (ADM-023)

A switch controls the `ENABLE_SIGNUP` key: on, users may self-register
without an invite; off, sign-up in the main web app is invite-only. The
toggle persists immediately via `PATCH /api/instances/configurations/`
with a progress notice that resolves to success or failure. The whole
switch group dims and disables while any config save is in flight. A
failed save reports failure and keeps the stored value.

### 4.2 Method registry (ADM-024)

Six method cards render in a fixed order (oracle-pinned): email
magic-link codes, passwords, Google, GitHub, GitLab, Gitea. Each card
shows an icon, a name, and a description; the magic-link card notes
that SMTP must be configured first. The GitHub icon swaps with the
light/dark theme.

### 4.3 Per-method enable with last-method guard (ADM-025)

Each method has its own enable flag, toggled directly from its card and
persisted via `PATCH /api/instances/configurations/` with the
per-method enable key. Turning a method off is blocked when it is the
only enabled method: an error notice shows and no API call fires. The
guard counts only the six method keys — open-sign-up is exempt and
never counts toward (or against) the minimum of one. After a save
notice, toggles reflect stored state.

### 4.4 Card controls by configured state (ADM-026)

Magic-link and password cards always show only a toggle. An OAuth card
that is not yet configured shows a configure action linking to its
sub-page; once configured it shows an edit link plus an enable toggle.
"Configured" means the required credentials are present:
client id and secret for Google, GitHub, and GitLab; host plus client
id and secret for Gitea (oracle-pinned keys: `GOOGLE_CLIENT_ID`,
`GOOGLE_CLIENT_SECRET`, `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET`,
`GITLAB_CLIENT_ID`, `GITLAB_CLIENT_SECRET`, `GITEA_HOST`,
`GITEA_CLIENT_ID`, `GITEA_CLIENT_SECRET`).

### 4.5 Provider sub-page shell (ADM-027)

Each provider page shows a borderless method card with the provider
identity and a master enable toggle in its header. Toggling persists
the provider enable key with a success notice. The toggle is disabled
while saving or before configuration loads; the body shows a skeleton
until configuration loads. Each page carries its own provider-specific
tab title. Note: unlike the landing toggles, the shell master toggle
has no last-method guard — disabling the last method from a provider
page is allowed and persists.

### 4.6 GitHub credential form (ADM-028)

Required client id (`GITHUB_CLIENT_ID`) and secret
(`GITHUB_CLIENT_SECRET`, password field with show/hide) plus an
optional organization id (`GITHUB_ORGANIZATION_ID`); a sync toggle
(`ENABLE_GITHUB_SYNC`, see `§4.10`); read-only copy fields for the
origin URL and the derived callback URL (see `§4.11`). Field help
links to the provider's OAuth application console. Save persists all
fields.

### 4.7 GitLab credential form (ADM-029)

Required host (`GITLAB_HOST`), application id (`GITLAB_CLIENT_ID`),
and secret (`GITLAB_CLIENT_SECRET`, password field with show/hide); a
sync toggle (`ENABLE_GITLAB_SYNC`); a read-only copy field for the
derived callback URL only (no origin copy field on this form). The
host placeholder suggests the public GitLab service; its help explains
either the public service or a self-hosted domain is acceptable. Save
persists all fields.

### 4.8 Gitea credential form (ADM-030)

Required host (`GITEA_HOST`, defaulting to the public Gitea service
when unset), client id (`GITEA_CLIENT_ID`), and secret
(`GITEA_CLIENT_SECRET`, password field with show/hide); a sync toggle
(`ENABLE_GITEA_SYNC`); a read-only copy field for the derived callback
URL only. The help link in the field description is built from the
currently typed host, pointing at that host's application-settings
page. Intended behavior behind NEWFRONT-492: the old link is built at
parent-render time from form internals, so it lags the typed value
(and a cleared host keeps showing the old host's link until the next
render). The new link updates live with the typed host (for example by
subscribing to the field), falling back to the default public host
when the field is empty.

### 4.9 Google credential form (ADM-031)

Required client id (`GOOGLE_CLIENT_ID`) and secret
(`GOOGLE_CLIENT_SECRET`, password field with show/hide); a sync toggle
(`ENABLE_GOOGLE_SYNC`); read-only copy fields for the origin URL and
the derived callback URL. Save persists all fields.

### 4.10 Sync toggle (ADM-032)

Each provider form carries a switch, separate from the master enable,
controlling whether user attributes are refreshed from the provider on
each sign-in. It is stored as a string flag (`"1"`/`"0"`,
defaulting to `"0"` when unset) and persists only with the form save:
flipping it stages the value without firing a request, and the master
enable toggle sits out of the form entirely.

### 4.11 Copy fields (ADM-033)

Clicking a copy field writes the shown URL to the clipboard and shows
an informational notice. The base origin is the configured API base
URL, or the current window origin when unconfigured; callback URLs are
derived as `<origin>/auth/<provider>/callback/` (oracle-pinned shape,
with `github`, `gitlab`, `gitea`, `google` spellings). GitHub and
Google forms show both origin and callback fields; GitLab and Gitea
show the callback field only. The pasted values are consumed in the
external provider consoles; this app never calls them.

### 4.12 Discard guard (ADM-034)

The back action links to the authentication landing page. If the form
is dirty, navigation is intercepted and a confirm-discard modal offers
keep-editing (closes the modal, values preserved) or leave-and-discard
(navigates, values lost). A clean form navigates directly with no
modal.

### 4.13 Save gating and reset (ADM-035)

Save is disabled unless the form is dirty and shows a busy label while
in flight. Required-field violations block silently (no notice, no
request). On success a success notice fires and the form's default
values are re-read from the server response, so the form is clean
again. On failure only a console log records the error — an accepted
OSS quirk the new app preserves — and the server is untouched.

### 4.14 Landing skeleton (ADM-036)

Before configuration loads, the landing page shows a multi-bar
skeleton in place of the sign-up toggle and the method cards. Data
comes from the shared configurations fetch.

## 5. Workspaces (ADM-037–045)

Slice owners: NEWFRONT-512 (ADM-037–042), NEWFRONT-513 (ADM-043–045).
List route `/god-mode/workspace/`; create route
`/god-mode/workspace/create/`.

### 5.1 Creation-restriction toggle (ADM-037)

A switch controls the `DISABLE_WORKSPACE_CREATION` key: on, only the
admin may create workspaces (everyone else must be invited); off,
creation is open per the main app's rules. It persists immediately via
`PATCH /api/instances/configurations/` with a progress notice resolving
to success or failure. The row is a skeleton while configuration
loads, and the group disables while a save is in flight. A failed save
reports failure and keeps the stored value.

### 5.2 Workspace list (ADM-038)

A titled section with a count badge lists every workspace on the
instance from `GET /api/instances/workspaces/` (cursor-paginated; see
`§5.4`). Each row shows a logo (image, or a lettered fallback from the
workspace name when no logo is set), the name and slug (the slug with
an explanatory tooltip), the owner email when present, and project and
member counts when present (each count hidden when null rather than
shown as zero).

### 5.3 Row deep-links (ADM-039)

Each row is a link opening the main web app at the workspace slug in a
new tab (with the noreferrer relation). There is no in-console
workspace detail: the row offers only the open-in-new-tab link. The
link goes through the F-09 coexistence helper (see `§0.4`), targeting
the main-app workspace home path.

### 5.4 Load-more pagination (ADM-040)

When more results exist (the page reports further results with a next
cursor), a load-more control appends the next page via
`GET /api/instances/workspaces/?cursor=<next>`; page size is ten. The
control shows a busy state and disables while paginating. Appended rows
join the existing list; nothing already shown is refetched.

### 5.5 Loading and mutation states (ADM-041)

The whole section is a skeleton on first load. A small spinner appears
next to the count during pagination or refresh. An empty instance shows
a zero count with no rows and no special empty illustration — just the
section chrome and the create action.

### 5.6 Management limits (ADM-042)

A helper line states that workspaces cannot be deleted here and can
only be opened when the admin is a member of the workspace. Rows expose
no delete and no edit action. The section header carries the link to
the create page. The no-delete rule is load-bearing (see also
ADM-073): the new app must not add workspace deletion.

### 5.7 Create-workspace form (ADM-043)

Fields are name, slug, and organization size (a required select over
the fixed size bands, from solo to the largest band). The slug input is
prefixed with the web base URL (see `§0.4`) and auto-derives from the
name (lowercased, trimmed, spaces to hyphens) while remaining
independently editable; typed slugs are forced lowercase. The slug
accepts only alphanumerics, hyphen, and underscore, with an inline
message for illegal characters; the name is required and capped in
length with inline messages; organization size is required with an
inline message when unpicked. Submit is disabled until the whole form
is valid and shows a busy label while creating. Typed values persist
across unmount (navigating away and back keeps what was typed).

### 5.8 Slug-availability check (ADM-044)

On submit, the slug is checked first via
`GET /api/instances/workspace-slug-check/?slug=<slug>`. Creation
proceeds only when the check reports available and the slug is not on
the restricted-URL list; otherwise an inline "URL taken" message shows
under the slug and nothing is created. A failed check request shows an
error notice and the user stays on the form.

### 5.9 Create and return (ADM-045)

Creation POSTs to `/api/instances/workspaces/` carrying the name,
slug, and organization size — and no owner field (oracle-pinned: the
request body carries no owner; ownership is assigned server-side to the
creating admin and later shown in the list row). On success a success
notice shows and the app navigates to the workspace list. On create
failure an error notice shows and the user stays on the form with
values intact.

## 6. AI settings and image removal (ADM-046–050)

Slice owner: NEWFRONT-514. AI route `/god-mode/ai/`. There is no image
route (see `§6.4`).

### 6.1 OpenAI model and key (ADM-046)

Under an OpenAI heading with helper links to the vendor docs, a model
text field (`LLM_MODEL`) and a password API-key field (`LLM_API_KEY`,
with show/hide) configure the instance LLM. Both fields are optional:
an empty save is allowed and the save action is always enabled. Save
persists via `PATCH /api/instances/configurations/` with a success
notice; configuring enables AI features across all workspaces. Save
failure logs to console only — an accepted OSS quirk the new app
preserves.

### 6.2 Other-vendor callout (ADM-047)

A static callout invites admins who prefer a different AI vendor to
make contact through the company contact page. It is messaging only:
no alternative provider is configurable on this page.

### 6.3 AI skeleton (ADM-048)

A header/grid/button skeleton shows until configurations load, then
yields to the form. Data comes from the shared configurations fetch.

### 6.4 No image page (ADM-049, ADM-050) — target per main#523

No Image page, route, sidebar entry, Unsplash key field, or loading
skeleton exists anywhere: no third-party image search is offered. The
removal deletes the old page sources, the route-table entry, the
sidebar entry, and the `UNSPLASH_ACCESS_KEY` config key.

The old console still serves the pre-removal behavior (a sidebar
Images entry, an `/image/` route rendering the key form with save, and
a skeleton while configurations load). That is the bug NEWFRONT-493
records, and the oracle leg of `ai-image.spec.ts` keeps pinning it
with a `bug:` title until the upstream removal, PDASHOSS01-237, lands
on `main`.

The new console builds the absence, and its own leg asserts it: the
sidebar exposes no image entry, `/god-mode/image/` falls through to
the catch-all not-found screen like any unmatched path (so no page
mounts, no key field or save renders, and no skeleton shows even while
a configurations read is stalled), and the breadcrumb label map
carries no image entry — `image` uppercases like any unknown segment.
Two unit suites back that up outside the browser: the route test pins
the fall-through and the sidebar absence, the shell test pins the
label map, and the source-level absence scan rejects the Unsplash key
and the image-page test ids in every admin module.

## 7. Loop jobs (ADM-051–061, ADM-073)

Slice owners: NEWFRONT-515 (ADM-051–054), NEWFRONT-516 (ADM-055–057),
NEWFRONT-517 (ADM-058–061, ADM-073). List route `/god-mode/loop/`;
detail route `/god-mode/loop/<jobId>/`.

Job fields: `id`, `slug`, `name` (admin name), `public_name`
(user-facing name), `public_description`, `prompt`, `min_role`
(oracle-pinned numeric values 20/15/5 mapping to Admin/Member/Guest),
`enabled`, `is_builtin`, `rrule`, `tzid`, timestamps, and optional
`stats` (`target_count`, 24h `completed`/`failed`/`skipped`).

### 7.1 Job list (ADM-051)

A table lists the jobs from `GET /api/instances/loop/jobs/` with
columns for name (linking to the job detail), slug, recurrence rule,
minimum role (numeric role rendered through the Admin/Member/Guest
labels, raw value as fallback), builtin yes/no, and an inline enabled
toggle.

### 7.2 Inline enable toggle (ADM-052)

The toggle flips optimistically (the UI updates before the write
lands), then persists via `PATCH /api/instances/loop/jobs/<id>/` with
the `enabled` flag and revalidates. No notice shows on success. On
failure an error notice shows and the state rolls back via
revalidation.

### 7.3 Empty state (ADM-053)

With no jobs, the list shows a short inline paragraph prompting
creation of the first job — an inline message, not the shared
empty-state component, and no table.

### 7.4 List skeleton (ADM-054)

Row-height skeleton bars show while the list loads, then resolve to
the table or the empty message.

### 7.5 Create modal (ADM-055)

A header action opens a create modal with fields for slug, admin name,
user-facing name and description, prompt, minimum role (defaulting to
member), timezone (defaulting to UTC), and recurrence rule (defaulting
to a daily rule). On save the modal closes and the list revalidates,
showing the server-stored job.

### 7.6 Edit modal (ADM-056)

The same modal opens pre-filled from the detail page. The slug field is
disabled for builtin jobs; all other fields edit normally. On save the
modal closes and the job and list revalidate. Cancel leaves the server
untouched.

### 7.7 Modal validation and errors (ADM-057)

Save is disabled unless slug, admin name, user-facing name, and prompt
are all non-blank. Description, minimum role, timezone, and recurrence
have defaults and never gate save. A save failure shows an error
notice preferring the server-provided detail, then the error field,
then a generic fallback. The modal body scrolls within a capped
height.

### 7.8 Job detail (ADM-058)

The detail page loads the job (`GET /api/instances/loop/jobs/<id>/`)
and its targets (`GET /api/instances/loop/jobs/<id>/targets/`). It
shows the job name as the heading with the user-facing description,
edit and delete actions (see `§7.10`), four stat tiles (targets, and
24h completed/failed/skipped) when stats are present, and a targets
table with columns for workspace slug, user email, next run, last-run
status, last-run tokens, and last skip reason. Missing timestamps,
statuses, and reasons render as an em dash; empty targets show a
placeholder row. Unloaded jobs show a skeleton (see `§7.11`).

### 7.9 Skip-reason filter (ADM-059)

A dropdown of skip reasons — blank for all (oracle-pinned label
`all`), plus the fixed set `user_disabled`, `master_paused`,
`min_role`, `llm_config_missing`, `membership_gone`, `turn_active`,
`dispatch_error` — refetches the targets with the `skip_reason` query
param when a reason is picked, and without it when reset to all.

### 7.10 Delete with no confirmation (ADM-060, ADM-073)

A danger-styled delete action deletes immediately via
`DELETE /api/instances/loop/jobs/<id>/` with no confirmation dialog.
On success a notice shows and the app returns to the list; on failure
an error notice shows and the user stays. The confirmation-free delete
is an accepted quirk the new app preserves (proven again by the
ADM-073 cross-page scenario: zero native and zero modal dialogs, row
gone server-side).

### 7.11 Detail skeleton (ADM-061)

Skeleton bars show under a placeholder header until the job loads,
then yield to the detail.

## 8. Dashboard shell (ADM-062–068, ADM-072)

Slice owner: NEWFRONT-506. The shell is the dashboard layout all
settings pages mount in: sidebar, header, and content outlet. It is
god-mode's own chrome and shares nothing with the main shell
(see `§0.3`).

### 8.1 Sidebar navigation (ADM-062)

The sidebar lists the settings areas in a fixed order (oracle-pinned):
general, email, authentication, workspaces, AI, loop, runners. Each
entry shows an icon, a name, and a one-line description and routes to
the matching page. There is no image entry (target per main#523) and
there is a runners entry between loop and the removed image position
(target per main#534); observed old behavior behind NEWFRONT-493
(image present) and NEWFRONT-494 (runners missing) is pre-target. The
active entry is highlighted by path prefix (so a provider sub-page
keeps its parent highlighted). No entry has per-item permission or
edition gating, and navigating the sidebar fires no mutation.

### 8.2 Collapse behavior (ADM-063)

A footer control collapses the sidebar to icon-only entries with hover
labels; expanding restores names and descriptions (which show no hover
labels when expanded). The collapsed state persists in local storage
and restores on reload and at session bootstrap. The sidebar
auto-collapses at narrow viewports (one-way: narrowing collapses,
widening does not re-expand), collapses on outside tap on small
screens, and renders off-canvas on mobile.

### 8.3 Account dropdown (ADM-064)

The sidebar header hosts an account control showing the admin label
with avatar and email, opening a menu with the theme switch and
sign-out (which embeds the CSRF token for its native form post, see
ADM-010). The menu renders the same entries off the icon trigger when
collapsed. The shell mounts only after the session resolves (the
unresolved window is the ADM-009 loader), so the trigger always
renders in the loaded state; there is no skeleton variant.

### 8.4 Theme toggle (ADM-065)

A menu item flips between light and dark mode; its label names the
target mode. The choice persists client-side and resets to system on
sign-out. The resolved theme drives the theme-swapping surfaces
(failure illustration, sign-in/out logos, GitHub icon).

### 8.5 Breadcrumbs (ADM-066)

The header shows a breadcrumb trail: a leading settings crumb linking
to general settings, then one crumb per path segment except the last
empty segment, each linking to its cumulative path. Segment labels
come from a fixed label map (covering the settings areas, the four
providers, and the create segment); unmapped segments render
uppercased with the full label on hover (oracle-pinned: the loop
segment and raw job ids render uppercased). Long labels truncate with
a tooltip.

### 8.6 Help section (ADM-067)

The sidebar footer carries a control opening the main web app root
(through the F-09 coexistence helper, see `§0.4`) and a help popover
with external documentation, forum, and bug-report links plus a footer
showing the running instance version (from the instance info
`current_version`). Collapsed, the controls restack vertically with
hover labels; expanded, the redirect control shows no hover label.

### 8.7 Mobile hamburger (ADM-068)

On small screens a header hamburger control shows the off-canvas
sidebar; it is hidden at desktop widths. While the rail is open it
covers the hamburger, so closing goes through the sidebar's own footer
control.

### 8.8 Web-only (ADM-072)

See `§0.2`: the console ships only in the web build. The new desktop
build mounts no admin tree and references no admin mount, and no
desktop affordance exists on any god-mode screen.

## 9. Cross-page invariants (ADM-069–071)

Slice owner: NEWFRONT-520. These rows pin absences: the new app must
not add what the old app omits.

### 9.1 No real-time updates (ADM-069)

No websocket, server-sent stream, or polling exists. The settings
pages that read configurations share one cached fetch (see `§0.6`):
client-side hops between them issue no refetch. Lists refresh only
via explicit revalidation after a mutation (a modal save re-reads the
affected list exactly once). The new app must not silently add live
updates that would change parity.

### 9.2 No drag-and-drop, import, or export (ADM-070)

The console has no drag-reorder, no file import, and no export on any
page. This absence is intentional and must be preserved: no draggable
source, no file input, and no import/export control may appear on any
dashboard page.

### 9.3 No custom keyboard shortcuts (ADM-071)

Navigation and forms rely only on native focus order, enter-to-submit,
and show/hide toggles removed from tab order. There are no bespoke
shortcuts to reproduce, and representative key presses navigate
nowhere and open nothing.

## 10. Cloud entry over OIDC (ADM-074–079)

Slice owner: NEWFRONT-519. Cloud edition only; the OSS root is `§1`.

### 10.1 Authorization decision, not a sign-in form (ADM-074)

In cloud, god-mode access is an authorization decision layered on AI
Republic OIDC: the only auth method is OIDC, and there is never a
password field, never a create-first-admin form, and no branching on
instance setup state. The root resolves to exactly one of four
outcomes from the admin check plus a one-shot auth probe: the
authorized-admin spinner (ADM-075), the OIDC bounce (ADM-076), the
terminal not-authorized screen (ADM-077), or the retryable error
screen (ADM-078). The oracle pins zero password/setup fields across
anonymous, signed-in, and admin cloud visits, and pins the password
sign-in/sign-up endpoints as shadowed 403s (OIDC only).

### 10.2 Authorized admin spinner (ADM-075)

When the admin check (`GET /api/instances/admins/me/`) confirms an
allowlisted instance admin, the page shows only the brand spinner
while the shared home layout replace-navigates to general settings
(the redirect itself is ADM-009). No form ever flashes first.

### 10.3 OIDC bounce with deep-link return (ADM-076)

On a genuine 403 from the admin check, a probe decides no-session
versus signed-in (see `§10.6`). A no-session result (probe answers
401/403) performs a full-page redirect to the OIDC sign-in entry
`/api/auth/sign-in/` carrying a relative `return_to` set to the
current path plus query, so a deep link into a specific god-mode URL
survives the round trip and lands back on the same URL. The redirect
replaces history (no back-button entry). The sign-in URL is built
against the configured API base, or the window origin when
unconfigured.

### 10.4 Terminal not-authorized screen (ADM-077)

A visitor who is authenticated but not on the instance-admin allowlist
(probe answers 200 after a genuine admin-check 403) sees a terminal
screen explaining the account is not authorized to manage the
instance, offering only a plain full-navigation link back to the site
root. It deliberately performs no redirect, no OIDC bounce, and no
switch-account action, so it cannot become a home-to-OIDC-to-denied
loop.

### 10.5 Retryable error screen (ADM-078)

A transient failure of the admin check or of the auth probe renders a
retryable error screen with a retry control. Retrying re-runs the
admin check: success lands the admin, a genuine 403 re-probes, and
another error stays on the retry screen. This state is never a deny
and never an OIDC bounce, so a backend blip cannot strand a legitimate
admin on the not-authorized screen or loop a visitor.

### 10.6 Probe classification and loop avoidance (ADM-079)

The probe is a credentialed fetch of `GET /api/auth/me/` that fires
only on a genuine admin-check 403 — never on success, never on a
transient admin-check failure. Its result classifies as authenticated
(response ok), anonymous (401/403), or inconclusive (any other status,
5xx, or network failure). An inconclusive probe resolves to the retry
screen, never to a guessed anonymous: guessing anonymous on a flaky
probe would trigger an OIDC bounce that returns to another failing
check, creating the exact home-to-OIDC-to-denied loop this gate
exists to prevent. The loop-avoiding classification is load-bearing.

Behavior behind NEWFRONT-496 (built in web_new; the oracle keeps the
old bug): the old gate classifies the admin check from a status field
that is always empty (it reads the status off the thrown response
body, which carries none), so every failure — anonymous 401, non-admin
403, transient 5xx — lands in the error bucket, the probe never fires,
and every non-admin visitor loops on the retry screen with zero OIDC
handoffs. The new gate reads the HTTP status off the response itself,
restoring the bounce for anonymous visitors, the denied screen for
signed-in non-admins, and the retry screen for genuinely transient
failures. The cloud-entry scenarios assert that classification on the
web_new leg and keep the oracle leg on the old behavior, and the gate
asserts a probe 5xx resolves to retry (never a bounce) before denying
on a retry once the session reads back.

## 11. Runners page (ADM-080) — target per main#534

Slice owner: NEWFRONT-518. Route `/god-mode/runners/`, sidebar entry
after loop (see `§8.1`).

### 11.1 Page behavior

A Runners page lists every runner on the instance newest-first from
`GET /api/instances/runners/` with columns for name, workspace slug,
owner email, provisioning, status, runner version, codex version, and
last heartbeat. Absent versions, slugs, owners, and heartbeats render
as an em dash; the heartbeat renders in the admin's locale format. A
provisioning filter dropdown (oracle-pinned values: blank/`all`,
`manual`, `desktop_bundled`) refetches from page 1 with a total count
shown beside it. Previous/next pagination shows the current page
number; the pager renders only when more than one page exists, with
each side disabled at its bound. A loader shows while first loading;
an empty result shows a no-runners message row. Fifty rows per page.

### 11.2 Endpoint contract

`GET /api/instances/runners/` is read-only, cross-workspace, and
restricted to instance admins. Query params: `provisioning` (must be a
known provisioning value when present — a present-but-empty or unknown
value is rejected with 400, never silently ignored), `status` (same
400 rule for unknown values), `workspace` (filters by workspace slug),
and `page` (1-based, defaulting to 1; unparseable values fall back to
1). Ordering is newest-first by creation time. The response carries
the page number, the total count, and the rows; each row carries the
runner id, name, workspace slug, owner email, provisioning, status,
runner version, codex version (from the runner's device metadata), and
timestamps (last heartbeat, creation, revocation). Observed old
behavior behind NEWFRONT-494 (no sidebar entry, `/runners/` falling
through to not-found, endpoint answering 404) is pre-target: the new
app builds the page above.

### 11.3 Backend availability check (binding for NEWFRONT-518)

The endpoint does not exist on the base branch: `web-new-dev` has no
runner admin view, no runner admin URL module, and no `runners/`
mount under the instance license routes, so the endpoint answers 404
there. The implementation exists only on the unmerged branch
`origin/pi-dash/pdashoss01-209` (feature commit plus a review fix that
rejects empty filter values with 400), which also carries the old-app
page, route, sidebar entry, and contract tests this section restates.
The runners slice therefore waits on a backend issue: NEWFRONT-503
already owns the conditional (if the seeded backend still 404s it
files a PDASHOSS01 backend issue blocking NEWFRONT-518 instead of
stubbing). No stub endpoint, no mock page: until the backend lands,
the slice stays blocked and `/god-mode/runners/` falls through to
not-found. Probed 2026-10-15 on `web-new-dev` tip: the endpoint still
404s, so NEWFRONT-503 filed PDASHOSS01-303 blocking NEWFRONT-518 and
landed workspaces+loop only.

## 12. F-06/F-09 reuse map

Survey of the existing `apps/web_new` code (new code, verified on
`web-new-dev`), per slice: what exists to reuse versus what must be
added. New code is freely reusable across the new trees (via public
`index.ts` boundaries); nothing below reuses old code.

### 12.1 What exists

- F-09 coexistence (`src/shared/coexistence/`): `CrossAppLink`,
  `resolveLinkKind`, `isMigratedPath`, and `migrated-routes.json`
  (currently the member-auth prefixes; the area gate appends
  `/god-mode`). Use directly for ADM-039, ADM-043, ADM-067 (see
  `§0.4`).
- Member session (`src/core/session/`): query-key factories and
  `queryOptions` for member identity, workspaces, profile, settings,
  and instance config; page-gate deciders; session-expiry middleware;
  the Zustand reset registry (`registerStoreReset`); permission
  constants. God-mode reuses the patterns (key factory,
  `queryOptions`, middleware, reset registry) but not the member
  state: admin auth state is its own (see `§0.3`).
- Password strength (`src/features/auth/passwordStrength.ts`, exported
  from the feature index): the shared 8-plus-four-classes rule with
  criteria breakdown, plus the hint component. The setup form reuses
  the rule; its meter-on-focus and mismatch-hint timing stay
  god-mode's own (ADM-005).
- Safe-path helpers (`asString`, `safeNextPath`, return-path parsing
  in `core/session` guards and `features/auth/entrySearch.ts`): reuse
  for the setup/sign-in query-param reads and the cloud `return_to`
  handling where the semantics fit.
- API client (`@pidash/api-client`): fetch client with base URL,
  credentials, CSRF header plumbing off `/auth/get-csrf-token/`,
  normalized errors, and middleware; one existing instance read
  (`getInstanceConfig`, the `config` half of `GET /api/instances/`).
  The native auth forms reuse the CSRF fetch for their hidden field
  (extending the client if it does not already expose the raw token
  for form embedding).
- Kit (`@pidash/kit`): button, text input, textarea, dialog, menu,
  popover, tooltip, toast host, skeleton, spinner, breadcrumbs,
  avatar, badge, field scaffold, icon button. Covers most admin
  needs.
- Platform (`src/core/platform/`): `platform.kind` with the web-only
  versus desktop-only route precedent in the existing entry routes.
  The god-mode mount follows that precedent (see `§0.2`).
- Edition (`src/core/edition/`): the typed `Edition` interface, the
  OSS module, and the named-slot precedent. The cloud entry seam
  follows the slot precedent (see `§0.8`).

### 12.2 What must be added

- Kit switch and kit select: no toggle or dropdown control exists in
  kit yet, and this area needs both pervasively (master toggles,
  method toggles, sync toggles, telemetry switch, restriction switch,
  enable toggles, security selector, organization-size select,
  min-role select, skip-reason and provisioning filters). The first
  slice that needs each control adds it to kit with unit tests; later
  slices reuse it. Admin-local one-off controls are not acceptable
  for these two primitives.
- Admin query subtree and contracts: every admin endpoint contract
  (NEWFRONT-502/503), the admin key factory rooted outside the member
  roots, the admin-check query, and admin guard deciders. The
  `getInstanceConfig` contract covers only the `config` half of the
  info response; the contracts slices add the `instance` half
  (setup flag, names, telemetry, workspace-existence, versions)
  without forking the existing contract.
- God-mode routes, layout, shell, pages, and forms: the whole
  `/god-mode/*` subtree is new. The main shell (`shared/shell`:
  AppShell, Sidebar, CommandPalette) is explicitly not reused.
- Cloud entry seam: the edition slot (or route extension) selecting
  the cloud resolver at the root, proposed by NEWFRONT-519 with a
  Decisions-page entry.
- Admin UI stores (sidebar collapsed state, popup flag, theme
  follow-system rule): small new stores registered for reset on
  admin sign-out. The member theme/session stores are not reused.

### 12.3 Per-slice map

- NEWFRONT-502 (contracts: instance, admins session, configurations,
  email): exists — client, CSRF plumbing, middleware, contract/test
  pattern, `getInstanceConfig`. Adds — instance-info (instance half),
  admins me/list, configurations get/patch, disable-email, and
  email-credentials-check contracts plus contract tests and the admin
  key factory.
- NEWFRONT-503 (contracts: workspaces, loop, runners): exists — same
  client and key-factory patterns from 502. Adds — workspace
  list/create/slug-check, loop jobs CRUD plus targets, and runners
  list contracts plus tests; owns the runners-backend conditional
  (files the PDASHOSS01 issue if the endpoint still 404s).
- NEWFRONT-504 (entry, setup, sign-in): exists — strength rule and
  hint, safe-path helpers, CSRF fetch, kit inputs/dialog/toast.
  Adds — god-mode home layout and root gate, setup and sign-in forms
  with the native-POST mechanism, banner mapping with the intended
  link target and numeric-code matching.
- NEWFRONT-505 (guards, sign-out, 404, popup): exists — guard-decider
  and middleware patterns, reset registry, coexistence helper.
  Adds — admin tri-state guards, admin sign-out with client reset,
  catch-all screen, first-workspace popup.
- NEWFRONT-506 (shell + web-only mount): exists — platform gating
  precedent, kit menu/popover/tooltip/skeleton/avatar/breadcrumbs,
  coexistence helper, instance version read. Adds — dashboard
  layout, sidebar with entries/order/highlight, collapse with
  persistence, account menu, theme switch, breadcrumbs, help
  section, hamburger, and the web-only mount.
- NEWFRONT-507 (general): exists — admin contracts and key factory,
  kit inputs. Adds — general page and shared save with the intended
  empty-name message and log-only failure.
- NEWFRONT-508 (email): exists — contracts, kit inputs, kit switch
  (added here or already added). Adds — master toggle with the
  intended failure reporting, SMTP form with dirtying security
  picker, test modal with the intended entered-settings scope (or
  save-first with a backend issue), skeletons.
- NEWFRONT-509 (auth landing): exists — contracts, kit switch.
  Adds — sign-up toggle, method registry, last-method guard, card
  controls, landing skeleton.
- NEWFRONT-510 (provider shells + shared form behavior): exists —
  contracts, kit inputs/switch/dialog, copy-to-clipboard web API.
  Adds — four shells with master toggles, sync toggle, copy fields,
  discard guard, save gating/reset with log-only failure.
- NEWFRONT-511 (provider credential forms): exists — everything from 510. Adds — the four credential field sets with help links,
  including the live Gitea host link.
- NEWFRONT-512 (workspaces list): exists — contracts, coexistence
  helper. Adds — restriction toggle, list rows with deep-links,
  load-more, loading states, limits line.
- NEWFRONT-513 (create workspace): exists — contracts, kit inputs,
  kit select (added here or already added), coexistence helper.
  Adds — create form with derived slug, availability check,
  create-and-return.
- NEWFRONT-514 (AI + image removal): exists — contracts, kit inputs.
  Adds — AI form with always-enabled save and log-only failure,
  vendor callout, skeleton; proves the image absence (no route, no
  entry, no key).
- NEWFRONT-515 (loop list): exists — contracts, kit switch. Adds —
  jobs table with role labels, optimistic toggle, empty state,
  skeleton.
- NEWFRONT-516 (loop modal): exists — contracts, kit dialog/inputs,
  kit select. Adds — create/edit modal with defaults, builtin slug
  lock, gating, and server-error surfacing.
- NEWFRONT-517 (loop detail): exists — contracts, modal from 516, kit
  select. Adds — detail with stat tiles and targets table,
  skip-reason filter, confirmation-free delete.
- NEWFRONT-518 (runners): exists — contracts (conditional on the
  backend landing), kit select. Adds — runners page with filter,
  pagination, loader, and empty state. Blocked until the endpoint
  exists (see `§11.3`).
- NEWFRONT-519 (cloud entry): exists — edition interface and slot
  precedent, admin-check contract, member probe endpoint (as a
  one-shot read, not a session). Adds — the cloud resolver with the
  four outcomes, the probe classification with loop avoidance, and
  the edition seam with a Decisions-page entry.
- NEWFRONT-520 (invariants): exists — the whole area. Adds — the
  shared-query proof (single cached configurations fetch), and pins
  the three absences (no realtime, no drag/import/export, no custom
  shortcuts).
- NEWFRONT-521 (area gate): runs the full admin parity suite against
  `apps/web_new`, turns passing rows to new-green, appends the
  `/god-mode` prefix to the migrated-route list with a regenerated
  proxy config, and files fix sub-issues for failures.

### 12.4 Precedents not to fork

Sign-out reset, guard deciders, query-key factories, promise
notices, and platform gating each have exactly one established shape
in the new trees; god-mode follows those shapes with admin inputs
rather than inventing parallel ones. The one deliberate split is
auth state itself (admin check versus member session), which the
backend enforces with separate cookies and which this spec keeps
separate end to end.

## 13. Endpoint and config-key reference

All endpoints the area uses, with the rows covering each. Bodies and
params are prose; see the row sections for exact field names.

| Endpoint                                                      | Rows                                                                                                                         | Notes                                                                                                                                                                     |
| ------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET /api/instances/`                                         | ADM-001, ADM-003, ADM-012, ADM-015                                                                                           | Single bootstrap fetch; no auto-retry, no focus refetch. Carries the instance object (setup flag, names, telemetry, workspace-existence, versions) and the config object. |
| `PATCH /api/instances/`                                       | ADM-013, ADM-016, ADM-017                                                                                                    | Name plus telemetry together.                                                                                                                                             |
| `GET /api/instances/admins/`                                  | ADM-014                                                                                                                      | Admin list; first entry's nested user email renders read-only.                                                                                                            |
| `GET /api/instances/admins/me/`                               | ADM-009, ADM-074, ADM-075, ADM-078                                                                                           | Admin check; drives the login tri-state and the cloud resolver. 401 anonymous, 403 only on revoked grant.                                                                 |
| `GET /api/auth/me/`                                           | ADM-076, ADM-077, ADM-078, ADM-079                                                                                           | Cloud one-shot probe only (200 signed-in, 401/403 anonymous, else inconclusive). Never a session.                                                                         |
| `/api/auth/sign-in/`                                          | ADM-076                                                                                                                      | Cloud OIDC entry; redirected to with relative `return_to`.                                                                                                                |
| `POST /api/instances/admins/sign-up/`                         | ADM-004                                                                                                                      | Native form post; redirects with echo/error params.                                                                                                                       |
| `POST /api/instances/admins/sign-in/`                         | ADM-007                                                                                                                      | Native form post; redirects with echo/error params.                                                                                                                       |
| `POST /api/instances/admins/sign-out/`                        | ADM-010                                                                                                                      | Native form post; redirects to the mount.                                                                                                                                 |
| `GET /auth/get-csrf-token/`                                   | ADM-004, ADM-007, ADM-010, ADM-064                                                                                           | CSRF token for the three native forms.                                                                                                                                    |
| `GET /api/instances/configurations/`                          | ADM-019, ADM-022, ADM-024, ADM-027, ADM-036, ADM-046, ADM-048, ADM-069                                                       | Shared single fetch; string key/value list.                                                                                                                               |
| `PATCH /api/instances/configurations/`                        | ADM-018, ADM-019, ADM-020, ADM-023, ADM-025, ADM-027, ADM-028, ADM-029, ADM-030, ADM-031, ADM-032, ADM-035, ADM-037, ADM-046 | Partial string map; response re-seeds saved forms.                                                                                                                        |
| `DELETE /api/instances/configurations/disable-email-feature/` | ADM-018                                                                                                                      | Master email off; optimistic blank with restore on failure.                                                                                                               |
| `POST /api/instances/email-credentials-check/`                | ADM-021                                                                                                                      | Test email; `receiver_email` plus optional candidate SMTP settings (`EMAIL_HOST`, `EMAIL_PORT`, `EMAIL_FROM`, `EMAIL_HOST_USER`, `EMAIL_HOST_PASSWORD`, `EMAIL_USE_TLS`, `EMAIL_USE_SSL`); omitted fields fall back to stored values, blank password = stored.                                             |
| `GET /api/instances/workspaces/`                              | ADM-038, ADM-040, ADM-041                                                                                                    | Cursor-paginated list, ten per page.                                                                                                                                      |
| `GET /api/instances/workspace-slug-check/?slug=`              | ADM-044                                                                                                                      | Availability probe before create.                                                                                                                                         |
| `POST /api/instances/workspaces/`                             | ADM-044, ADM-045                                                                                                             | Create; no owner in the body.                                                                                                                                             |
| `GET /api/instances/loop/jobs/`                               | ADM-051, ADM-054                                                                                                             | Job list.                                                                                                                                                                 |
| `POST /api/instances/loop/jobs/`                              | ADM-055                                                                                                                      | Create job.                                                                                                                                                               |
| `GET /api/instances/loop/jobs/<id>/`                          | ADM-058, ADM-061                                                                                                             | Job detail with optional stats.                                                                                                                                           |
| `PATCH /api/instances/loop/jobs/<id>/`                        | ADM-052, ADM-056                                                                                                             | Full update or `{enabled}`-only toggle.                                                                                                                                   |
| `DELETE /api/instances/loop/jobs/<id>/`                       | ADM-060, ADM-073                                                                                                             | Immediate delete, no confirm.                                                                                                                                             |
| `GET /api/instances/loop/jobs/<id>/targets/?skip_reason=`     | ADM-058, ADM-059                                                                                                             | Targets; optional reason filter.                                                                                                                                          |
| `GET /api/instances/runners/`                                 | ADM-080                                                                                                                      | Target per main#534; unmerged on the base branch (see `§11.3`). Params `provisioning`, `status`, `workspace`, `page`; 50 per page.                                        |

Configuration keys by page: general uses none (instance fields);
email uses `ENABLE_SMTP`, `EMAIL_HOST`, `EMAIL_PORT`, `EMAIL_FROM`,
`EMAIL_HOST_USER`, `EMAIL_HOST_PASSWORD`, `EMAIL_USE_TLS`,
`EMAIL_USE_SSL`; authentication uses `ENABLE_SIGNUP`,
`ENABLE_MAGIC_LINK_LOGIN`, `ENABLE_EMAIL_PASSWORD`,
`IS_GOOGLE_ENABLED`, `IS_GITHUB_ENABLED`, `IS_GITLAB_ENABLED`,
`IS_GITEA_ENABLED`, plus per-provider client, secret, host, org, and
sync keys (`GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`,
`ENABLE_GOOGLE_SYNC`, and the GitHub/GitLab/Gitea analogues with
`GITHUB_ORGANIZATION_ID`, `GITLAB_HOST`, `GITEA_HOST`);
workspaces use `DISABLE_WORKSPACE_CREATION`; AI uses `LLM_MODEL`,
`LLM_API_KEY`. `UNSPLASH_ACCESS_KEY` is removed with the image page.

## A. Non-rows and dead code

Reviewed and deliberately not rowed (matching the inventory coverage
checklist, confirmed while reading the sources):

- Framework bootstrap, compat shims, route type files, and the store
  context wiring are plumbing with no user-observable behavior.
- The shared empty-state component is not referenced by any admin
  screen (loop uses its own inline message, ADM-053).
- Two instance components are defined but never rendered (an
  alternate not-ready screen implying a setup deep link the live flow
  does not use, and an unused loading view); the live flow uses the
  inline setup form and the brand spinner instead.
- The extended header-label map and the extended providers module are
  empty passthrough seams with no behavior.
- The instance changelog endpoint, the generic auth sign-out helper,
  and the end-user magic-link/password-recovery helpers exist on the
  shared services but have no caller in the admin app; the console's
  sign-out uses the admin sign-out form (ADM-010).
- The admin session endpoint (`admins/session/`) exists on the
  backend but the console never calls it; the admin check is
  `admins/me/` (ADM-009).
- Static assets (illustrations, logos, favicon) are presentational
  only; the new app draws its own.
- Page document titles are set per route in the old app; the new app
  sets equivalent titles through its own router metadata. Exact title
  text is not pinned beyond the provider-specific and page-specific
  distinctions the oracle asserts.

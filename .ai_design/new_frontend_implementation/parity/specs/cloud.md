# Cloud area spec — home, docs, downloads, pricing, login, apps, profile tabs (CLOUD-001–040)

Parent epic NEWFRONT-91. Inventory
`.ai_design/new_frontend_implementation/parity/inventory/cloud.md` (rows
CLOUD-001–040, all oracle green). Oracle scenarios
`apps/web_new/e2e/parity/cloud/` (green on the cloud-edition old app).

Method: the old frontend was read as a behavior reference only — the cloud
overlay route group and its marketing, login, apps and profile-tab routes,
the catalog and credential components, the assistant settings page, the
provider sign-in hook, the session/refresh/sign-out/billing client logic,
and the OSS files each of those replaces. Nothing is imported, copied or
translated from those sources; this spec restates what was learned in prose.
The only code-like tokens below are route paths, API paths, query parameter
names and payload field names, which the contracts issues (NEWFRONT-638,
NEWFRONT-641) turn into zod contracts. New code is written from this spec,
the wiki pages (Reference, not source; Parity; Architecture; Platform and
editions; Data layer) and the kit — never with old files open.

Status convention: this spec owns no inventory Status changes. The forty
rows already carry `oracle green`; slices append `built` to the rows they
implement.

Intended-behavior rule: where the running old app contradicts the evident
intent, the oracle pinned the observed behavior as a `bug:` scenario and
filed an issue. One row still does that here — CLOUD-030 (`bug:`
NEWFRONT-473). CLOUD-038 used to (`bug:` NEWFRONT-474, intended: a plan
badge), but the owner removed the badge (NEWFRONT-1045), so both apps now
pin its absence and §8 states that target. Each section below states the
intended target; slices implement the target and the gate proves the
target. The remaining deviation is collected in §10.

## 1. Public redirects, marketing home and shared frame (CLOUD-001–005, slice NEWFRONT-645)

CLOUD-001 — The signed-out root address forwards to the marketing surface.
Visiting `/` with no session lands on the marketing home; a `next_path`
query value on that visit is carried forward as the login page's own
`next_path` parameter, so the visitor returns to their original target after
signing in. Without a carried target the root lands on the marketing home
directly. Signed-in handling at the root belongs to the auth area (its
wrappers decide where a session goes); this row covers the signed-out
forward only. No API. Oracle: `public-redirects.spec.ts` (root landing and
deep-target preservation).

CLOUD-002 — The cloud login address renders the sign-in page in place, while
the two legacy sign-in spellings keep the old redirect-to-root behavior and
bounce through the root forward instead of rendering anything themselves.
So `/login` shows the cloud card directly; `/sign-in` and `/signin` resolve
via `/`, which for a signed-out visitor means the marketing home (with any
`next_path` preserved per CLOUD-001). No API. Oracle:
`public-redirects.spec.ts` (in-place login vs alias bounce).

CLOUD-003 — The marketing home at `/home` presents the product and routes
visitors onward. Since NEWFRONT-1041 the page deliberately tracks the old
cloud landing page's shape, because the owner reviewed both side by side and
asked for them to stay nearly the same rather than for a redesign:

- Hero, two columns on wide screens and stacked on narrow ones. The copy
  column opens with a small status pill naming the product and what it is,
  then a very large two-line headline, a short paragraph on what Pi Dash
  does, three actions in this order — start free (sign-in, filled), download
  the app, read the docs — and a muted line naming the free start and the
  agent choices. The second column is an illustration of one agent run: an
  issue header with a running badge, a work-plan list of four labelled steps
  with done/active/queued marks, a runner status card, a live-activity
  message, a checks list with passed/running values, and a review card. The
  illustration is presentation only: no links, no buttons, no data.
- Value strip: a banded row of four short highlights (durable workpads,
  enrolled runners, structured escalation, review-ready changes), two per
  row on narrow screens and four on wide ones.
- Delivery narrative: an eyebrow, a headline and an intro line, then four
  numbered cards "01"–"04" (state the outcome, hand it to a runner, build
  and verify, review to done), four across on wide screens.
- Capability grid: an eyebrow and headline beside four cards with icons
  (issues-to-pull-requests, agent choice, your machines, human control).
- Open-source panel: eyebrow, headline, paragraph, and a bordered action to
  the public repository that opens in a new tab.
- Closing panel: a centred eyebrow, headline, paragraph and filled action to
  sign-in, on a tinted card.

Every action on the page reaches a working destination; the page takes no
parameters and calls no API. The hero owns the page's only top-level heading.
Wording is ours and may evolve; the oracle pins section order, counts and
action destinations, and the layout assertions in the unit tests pin the
hero's two-column shape and the illustration. Oracle:
`marketing-home.spec.ts`.

CLOUD-004 — One shared frame (header, footer, page shell) wraps every public
page (`/home`, `/docs`, `/downloads`, `/pricing`). The header sticks to the
top of the viewport over a blurred page background and carries a 64px
content row. The brand mark is a four-piece monogram beside the product name
(the name stays hidden on the narrowest phones while the monogram keeps the
supporting link); both link to the marketing home. The actions follow it:
downloads, docs, pricing, the public repository on an external site, sign-in
as a bordered action and a start action as a filled accent action that also
leads to sign-in. The footer repeats the product links (downloads, docs, pricing,
repository) and adds the terms and privacy destinations on the external
company site, beside the brand mark and a one-line positioning note. On
cramped widths the frame degrades by hiding secondary header links rather
than breaking layout: docs, pricing and the start action drop out first,
then the repository link, while downloads and sign-in stay reachable.
Keyboard focus is visibly indicated on every header and footer link. A
decorative full-page backdrop sits behind the content, is hidden from
assistive technology, and carries no behavior: since NEWFRONT-1041 it is the
old landing page's ASCII world map, kept as one SVG file under the app's
public assets and painted as a fixed, centred, contained mask over the page
text colour at very low opacity, so it reads the same in light and dark
themes. The document carries only the asset URL, never the drawing itself,
so the map stays out of the JS and HTML payloads. Content columns sit on a
wide centred gutter that matches the old page at both desktop and mobile
widths. No API. Oracle: `marketing-frame.spec.ts` (frame repetition,
cramped-width hiding, visible focus).

CLOUD-005 — Public pages are crawlable with stable metadata. The marketing
pages and the signed-out root carry index/follow crawler directives,
canonical addresses, short descriptions, and social preview data (page URL,
preview image with dimensions and alternative text, site handle). The
application shell serves a robots document that allows crawling and points
at the sitemap, and the sitemap lists the marketing home, docs, and pricing
addresses. The static site build prerenders the root and the four marketing
pages (`/`, `/home`, `/docs`, `/pricing`, `/downloads`); every other route
stays client-rendered with a default no-index marker. To make prerendering
work, the app shell skips the authenticated provider stack on the
marketing-page routes and renders their content directly — those pages
consume no session stores. No API. Oracle: `crawlability.spec.ts` (robots
and sitemap, crawler metadata with prerendered content, root metadata
before its client redirect).

## 2. Docs, downloads and pricing (CLOUD-006–009, slice NEWFRONT-648)

CLOUD-006 — The docs page at `/docs` teaches the zero-to-runner path. It
opens with a hero (headline, short positioning line, actions into sign-in
and into the public repository), then explains the three-part model — the
hosted project system, the local command line, and the runner binding one
agent to a project — with what each part owns. A quick-start sequence walks
four steps in order: create a project (sign in, create workspace and
project, add at least one well-scoped issue); install the CLI (a shell-pipe
installer for macOS/Linux, a PowerShell-pipe installer for Windows, a
Windows MSI alternative for managed deployments, and a note that a
Linux-subsystem install only drives agents inside that subsystem); device
login (a login command against the hosted origin, normally auto-started by
the installers and needed by hand only after MSI installs, headless
installs, or re-authentication); and runner enrollment (project/agent
selection in the product, then the generated enrollment commands plus
diagnostics on the host machine). A closing stretch covers the optional
agent skill (a one-command installer giving coding sessions product access
through the same CLI), the four-phase operating loop (delegate, execute,
validate, review), and three trust notes (source and credentials stay on
the runner host; machine access is explicit and revocable; the core is open
source and self-hostable). The page takes no parameters and calls no API.
Command text is content, not code to reuse — transcribe installer and CLI
commands from the product's current release surface, never from old page
sources. Oracle: `docs.spec.ts`.

CLOUD-007 — The downloads page at `/downloads` offers desktop installers per
operating system. Each supported OS gets its own card naming the platform,
its architecture coverage, and its package format: macOS as a universal
disk image covering both chip families; Windows x64 as an executable
installer with an MSI alternative; Linux x64 as an AppImage with a
Debian-package alternative. Every download resolves through a stable
latest-release base address with a per-format filename, so the page needs
no per-release edits. A notice warns that the installers are not yet
publisher-signed and that the OS may show an unverified-publisher warning.
The page takes no parameters; its only network touch is the release-file
addresses themselves. Oracle: `downloads.spec.ts` (per-OS cards through the
stable release address).

CLOUD-008 — The same downloads page covers the standalone CLI and the web
fallback. A separate card offers the Windows CLI installer on its own
(a stable release-file address for the MSI), explains that a fresh terminal
is needed after install and that the sign-in command completes setup,
links to the CLI setup guide on the docs page, and a closing strip points
browser-preferring visitors at web sign-in. Oracle: `downloads.spec.ts`
(CLI card and web fallback).

CLOUD-009 — The pricing page at `/pricing` states the tiers, the free cap,
and answers common questions. Two tiers contrast side by side: a free tier
with no platform fee, capped at a fixed number of created issues
(five hundred), machine/agent freedom, and wallet-funded metered AI usage;
and a membership tier (monthly price billed annually plus tax, via an
external checkout address) with uncapped issue creation, larger included
usage quotas, and continued pay-as-you-go beyond them. An enterprise path
offers organization billing and tailored deployments through an external
contact address. A four-question FAQ explains what counts toward the free
cap (each created issue or work item, counted once; existing work stays
available at the cap), what the wallet covers (metered model and connected
usage; the product itself stays free within the cap), that members still
use pay-as-you-go beyond included quotas, and that self-hosted deployments
are unaffected by hosted-plan limits. Both tier cards link onward (free to
sign-in, membership to the external checkout). No API. Oracle:
`pricing.spec.ts`.

## 3. Login card, silent sign-in and return targets (CLOUD-011–014, slice NEWFRONT-652)

CLOUD-011 — The cloud sign-in page at `/login` offers a single
organizational-account action. The card presents one continue control, names
the account home visitors continue with, references the terms and privacy
destinations, and links back to the marketing home; beside it an intro panel
restates the positioning line and headline with its own marketing-home link.
Activating the continue control leaves the app for the provider flow via
`GET /api/auth/sign-in/` carrying the return target (`return_to`, plus
`return_origin` on split-origin setups where the browser origin differs from
the auth origin). Starting a manual sign-in clears the explicit-logout
marker (CLOUD-014) so later visits can resume silently again. The card also
renders a resume-failure alert when the page carries an error parameter
(CLOUD-012). Oracle: `login-card.spec.ts`.

CLOUD-012 — Returning visitors with a pending target skip the card via
silent sign-in. When a return target exists, no error parameter is present,
and no explicit logout was recorded for this tab, the login page resumes the
provider flow immediately (a replacing navigation, so the login entry leaves
no history step) instead of rendering the card. Any error parameter keeps
the card on screen with the resume-failure alert. Oracle:
`silent-sso.spec.ts` (silent resume with a pending target; card with alert
on error).

CLOUD-013 — Return targets survive login but cannot escape the app. The
pre-login path plus query string is preserved through the round trip; the
fragment is always dropped, and signing in from the login page itself
returns to the app root rather than looping back onto the login page. A
candidate target is accepted only when it is an app-local path: it must
start with a single leading slash (protocol-relative values are rejected),
must not contain backslashes (browsers normalize them into separators), and
must still resolve to the app's own origin once parsed — anything else is
discarded and the flow falls back to the root. The parsed path is checked
again after URL normalization, so encoded dot segments that would collapse
into a protocol-relative path (for example `/%2e%2e//evil.example/x`) and
encoded backslashes are discarded too. That second check is intended
behavior: the old app accepts those encoded forms (bug: NEWFRONT-777), so
the oracle pins the old result as a `bug:` scenario and the new app rejects
them. The login-loop guard decodes and case-folds the pathname before the
alias check, because the router matches `/%6Cogin` and `/LoGiN` as `/login`
and a decoded `?` or `#` ends the pathname there, so
`/%6Cogin%3Fnext_path=%2Fdocs` names the login page too. The old app's
exact-match, case-sensitive guard misses all three spellings and hands the
provider a login target (bug: NEWFRONT-778). Bounces to sign-in carry the
preserved location as the root's `next_path` parameter (omitted when the
location is the root itself); a fixed list of public and auth addresses
(root, marketing pages, login and its legacy aliases, sign-up and
account-recovery routes, with or without trailing slash) never triggers a
bounce. No API beyond the carried parameter. Oracle: `next-path.spec.ts`
(deep target with query surviving the round trip; unsafe targets
discarded; the old app's encoded and case-variant login loop pinned as a
`bug:`).

CLOUD-014 — Explicit logout disables silent sign-in until the next manual
attempt. Signing out records a tab-scoped explicit-logout marker (tab
session storage, so other tabs are unaffected); while it is set, visiting
the login page shows the card instead of auto-resuming. Starting a manual
sign-in clears the marker, so the following visit may be silent again.
Storage failures (privacy-restricted contexts) are swallowed — the marker
is a convenience, never a gate. No API. Oracle: `logout-marker.spec.ts`.

## 4. Paywall, session refresh, sign-out and desktop sign-in (CLOUD-010, CLOUD-015–020, slice NEWFRONT-653)

CLOUD-010 — Hitting the free creation cap surfaces a paywall prompt. When a
creation call is refused with a 402 whose payload carries the quota-exceeded
error and the free-limit code, the user gets a single confirmation-style
prompt naming the limit; accepting opens the payload's upgrade address (or
the pricing page when the payload carries none) in a new tab. A guard flag
prevents duplicate prompts from stacking when several calls fail together.
Refusals without that exact payload shape never prompt. Oracle:
`paywall.spec.ts`.

CLOUD-015 — Expired sessions refresh transparently; only real auth failures
bounce. A 401 response first triggers one cookie rotation via
`POST /api/auth/refresh/` plus a single replay of the original request:
concurrent 401s share one in-flight refresh, each request replays at most
once, and the refresh call itself is never replayed. Only a rejected
refresh or replay (a 401/403 _response_) sends the user to sign-in with the
current location preserved as the login target; public and auth pages never
bounce. Transport failures, timeouts, and server errors leave the page in
place — they say nothing about the session, so the next user action simply
retries with the existing cookies. Callers that resolve 401s as data rather
than rejections get the same treatment: a definitively rejected session
still bounces with the location preserved, while a transient failure is
re-raised so the caller keeps its existing user instead of recording an
error body as one. Oracle: `session-refresh.spec.ts` (single rotation with
the page intact; dead session bouncing with the location preserved; public
pages never bouncing; transport failure leaving the page in place).

CLOUD-016 — Cloud sign-out ends the app session and lands on sign-in. The
flow reads a CSRF token from `GET /api/auth/get-csrf-token/` (a missing
token aborts the sign-out with an error), posts an empty JSON body to
`POST /api/auth/sign-out/` with the token in the CSRF header, clears
desktop-local session data where present (a no-op in browsers; it runs
before the marker below because clearing wipes tab storage too), records
the explicit-logout marker (CLOUD-014), and performs a full replacing
navigation to `/login` so every store and cache starts from scratch. The
upstream provider session is intentionally left alone — signing out of the
app does not sign out of the account home. The login address (not the root,
which would bounce to marketing) is the landing target. Oracle:
`sign-out.spec.ts`.

CLOUD-017 — Generic auth screens gain an organizational provider button. The
provider option appears alongside the standard methods and carries the
user's current page through the round trip as `return_to` on the same
sign-in endpoint as CLOUD-011 — pathname plus query only, never the origin
(the server absolutizes relative paths against its inbound origin) and
never the fragment, which also keeps one-time tokens or invite codes on the
originating page out of provider URLs and server logs. Inside the desktop
shell the control instead opens the system browser with a desktop marker
and never navigates the embedded view (embedded provider flows are refused
as an anti-pattern); a desktop build without its baked API origin refuses
loudly with a logged error rather than building a dead URL, a malformed
baked base is caught and logged instead of surfacing an unhandled failure,
and a failed browser-open is logged without falling back to an in-window
navigation the desktop flow could not complete. Oracle:
`provider-button.spec.ts` (page preserved, fragment dropped; loud refusal
without a baked origin).

CLOUD-018 — Desktop sign-in completes through the system browser and a deep
link. The desktop card opens the system browser (never an embedded webview
flow) at the sign-in endpoint with the desktop marker set and an app-path
return target; the provider returns via the app deep link, which the shell
exchanges for webview cookies at `/api/auth/desktop-exchange/` — the
exchange refuses incomplete or unknown flows rather than minting partial
sessions. A blocked browser-open surfaces an error instead of hanging, and
a desktop build without its baked API origin aborts with a logged error
before building any URL. The deep-link handling and the exchange call live
in the desktop shell outside the web tree; the web side owns only the
marker, the app-path target, and the failure surfacing. Oracle:
`desktop-signin.spec.ts` (marker switching the sign-in branch; exchange
refusing bad flows; no embedded flow from the desktop card).

CLOUD-019 — The desktop shell shows the same cloud sign-in card as the web
login page. The desktop entry point renders the shared cloud card rather
than a desktop-specific form, so its behavior — single continue action,
terms and privacy references, return-target handling, desktop browser
branch — matches the web login page exactly. Same sign-in endpoint as
CLOUD-011. Oracle: `desktop-card.spec.ts`.

CLOUD-020 — Cloud onboarding asks for no password. The password-setup slot
of the onboarding profile step renders nothing and invokes none of its
callbacks, since cloud identities come from the provider. The surrounding
onboarding flow is unchanged and belongs to the auth area. No API. Oracle:
`onboarding-password.spec.ts`.

## 5. Apps shell, search, states and agent-tools toggle (CLOUD-021–023, CLOUD-029–030, slice NEWFRONT-655)

CLOUD-021 — The workspace sidebar gains an Apps entry for every role,
release-gated. With the catalog flag on, administrators, members and guests
alike see the entry with its own icon and active highlighting on its route;
installations belong to the person, not the workspace, so no role is
excluded. With the flag off the sidebar is identical to the open-source
build — no disabled row, no placeholder — and the apps route itself is never
declared, so it 404s in the router rather than mounting a page whose
requests the server would refuse. The flag is read at build time from the
environment (opt-in; unset means off) and mirrors a server setting — both
sides are set together, and the server stays the authority: a stale bundle
with the flag on reaches unregistered endpoints and renders the catalog
error state. No API. Oracle: `apps-shell.spec.ts` (entry with icon for
every role; highlighting with working navigation; flag-off builds carrying
neither entry nor route).

CLOUD-022 — The apps page lives in workspace chrome but lists the visitor's
own apps. The route nests inside the workspace shell (sidebar, command
palette with its shared modal hosts — without the latter the sidebar's
create action would be inert here) with the catalog as its full content; no
second navigation column shrinks it. An intro line states plainly that
installations belong to the viewer rather than the workspace and that
installed apps extend the assistant automatically — two people opening the
same address see different lists. A status read to
`GET /api/openhub/status/` gates the catalog: only a connected status loads
it (CLOUD-030 covers the other outcomes). Nothing is cached locally — the
service owns installation state and resolves it independently at agent-run
time, so a stale copy would misreport what an agent can do. Oracle:
`apps-shell.spec.ts` (workspace chrome with the catalog as content;
per-viewer lists on the same address).

CLOUD-023 — The app catalog supports search and cursor paging. Typing
filters the catalog through `GET /api/openhub/apps/marketplace/` with a
query, cursor and page-size parameter: keystrokes settle through a short
debounce (a few hundred milliseconds), the settled query is trimmed, and
every new query resets paging to the first window — a query change never
pages into the previous result set (the cursor is part of the fetch key).
Previous and next controls step through result windows over a history stack,
with a page indicator and disabled states at the ends and while loading;
the previous page renders stale content while the next window loads rather
than flashing empty. Oracle: `apps-search.spec.ts` (one fetch per settled
query; paging with reset on new queries).

CLOUD-029 — One switch governs whether agents may use installed apps. The
control states plainly that enabling it exposes every installed app's tools
— including ones that create, change or delete data in those services — to
every agent run the viewer starts, and that disabling keeps agents to the
product's own tools; there is no per-project, per-issue or read/write
scoping. The preference lives in the namespaced profile settings
(`GET` + `PATCH /api/users/me/profile/`, keyed under the catalog's own
settings namespace so builds cannot clobber each other) and defaults to
off. Toggling applies optimistically and rolls back with an error notice
when the server refuses, so the UI never claims a preference the server
rejected; the control is inert while a write is in flight. Oracle:
`apps-toggle.spec.ts` (live persistence; rollback with an error on
refusal).

CLOUD-030 — Unavailable or failing catalog states explain themselves. While
the status read is in flight the page shows a loading state. A failed
status read renders an error panel naming the failure with a retry control.
A status answer of "not connected" splits two ways: sessions whose
permissions predate catalog access point at signing out and back in, while
connected-capable-but-disconnected sessions show the server's own detail
(or an unavailability fallback). A failed catalog fetch shows an inline
retry and never masquerades as an empty catalog. Genuine emptiness has two
distinct states: no apps available at all, versus no matches for the
current search. Intended-behavior note (`bug:` NEWFRONT-473): a status read
that fails at the transport level — aborted request, killed connection,
offline browser, no HTTP response at all — must render the status-error
panel with its retry, exactly like a failed HTTP status read. The old app
falls through to the connected catalog chrome with the no-apps empty state
instead; the new frontend implements the error surface, not the
fallthrough. Reads: `GET /api/openhub/status/` plus the marketplace fetch.
Oracle: `apps-states.spec.ts` (stale-session guidance; server-detail
surface; unreachable status with retry; the transport-failure deviation;
inline catalog retry; distinct empty states).

## 6. Install, provider connect, credential dialog and cards (CLOUD-024–028, CLOUD-031, slice NEWFRONT-659)

CLOUD-024 — Installing an app also connects it. The install action posts to
`POST /api/openhub/apps/{id}/install/`, refreshes the catalog first, and
only then starts the connect step — so a later failure reads as
not-connected rather than not-installed. Outcomes report distinctly: a
success notice naming the app; an installed-but-not-connected notice when
the connect leg fails (any reserved tab is closed); and an install-failed
notice when the install itself is rejected, in which case connecting never
starts. Removal deletes via `DELETE /api/openhub/apps/{id}/install/`,
refreshes, and reports with an informational notice; a rejected removal
reports a failure. Per-card busy state guards double clicks through both
flows. Oracle: `apps-install.spec.ts` (commit with refresh before
connecting; failed installs never connecting; distinct
installed-but-unconnected outcome with its tab closed).

CLOUD-025 — Provider-based connects run in a pre-opened tab. Because a tab
opened after an await is no longer attributable to the click and browsers
block it, the tab is opened synchronously during the click — with
placeholder content and detached from its opener so the provider page
cannot navigate the product — for every app whose catalog metadata marks a
provider mechanism or omits the mechanism (older catalogs). The tab is then
pointed at the server consent handoff,
`GET /api/openhub/apps/{id}/auth/oauth/begin/?return_path=…`, which
redirects to the provider consent screen; the return path carries the
current page with any previous round-trip outcome stripped, and is used
only when starting fails — success finishes in the handoff tab while this
page learns the new state on its next refresh (data revalidates on window
focus, so returning here picks it up). A blocked popup explains itself with
a notice instead of failing silently, and the product tab is never
navigated away — replacing it would break the focus revalidation the flow
depends on. Oracle: `apps-oauth.spec.ts` (pre-opened tab reaching the
consent handoff; blocked popups explaining themselves).

CLOUD-026 — Failed connects report once and clean the address. A connect
that never started bounces back with an error query parameter, which fires
a single failure notice and is then stripped from the address (with its
sibling outcome parameter) via a history replacement — so later searches
and pages never re-fire it. No API. Oracle: `apps-error.spec.ts` (single
report with address cleanup; later searches staying quiet).

CLOUD-027 — Key-based apps connect through a generic credential dialog. The
dialog builds its fields entirely from the server's connection manifest for
the app — labels, placeholders, secret masking, format hints — with
publisher instructions rendered as plain text (emphasis markers dropped,
markup never interpreted) and the credential-issuance link shown only when
it is an absolute secure address without embedded credentials. A bare
key-type answer with no manifest falls back to a single secret field. Every
field is required and format hints are checked, with problems reported
inline per field and the whole set surfaced on a submit attempt; an invalid
hint pattern never blocks the form. Submit posts the trimmed values to
`POST /api/openhub/apps/{id}/auth/api-key/`, clears entered secrets from
memory, and on success closes with a confirmation and refreshes the list —
on failure the dialog stays open with the rejection (usually a wrong
credential, which the service verifies before storing). Cancelling also
refreshes, since the install already committed server-side and the card
must stop offering installation. Secrets never travel in addresses. Oracle:
`apps-dialog.spec.ts` (manifest-built fields; inline blocking; trimmed
submit with cleared secrets; issuance-link gating; single-field fallback
with failures staying open).

CLOUD-028 — App cards communicate install and credential state at a glance.
An app counts as installed from the marketplace payload alone (explicit
installed flag or installed status), so badges need no extra fetch; the
unfinished state — installed but credential-less, exposing no tools at all
— comes from the service's own suggested-action field rather than being
derived locally. Installed cards carry an installed marker, unfinished ones
an unfinished marker, and actions follow state: uninstalled cards offer
installation; unfinished ones offer connecting plus removal; fully
connected ones offer removal only. Icons prefer the catalog's self-hosted
graphic, fall back to the proxied external address
(`GET /api/openhub/apps/icon/?url=…`), and land on a letter tile when both
fail. Per-card busy state guards double clicks. Oracle: `apps-cards.spec.ts`
(state, actions and icon sources; letter-tile fallback; double-click
guard).

CLOUD-031 — Already-connected provider apps open in the app hub. Choosing
such an app navigates the handoff tab to the hub's detail page for it,
built from the status answer's hub base plus the app's identity (preferred
slug, then platform id). Bases that are not plain secure addresses — except
local loopback over plain HTTP, which stays navigable for development — or
that embed credentials are refused: the tab closes and the failure reports
instead. No API; address building only. Oracle: `apps-oauth.spec.ts`
(connected apps opening in the hub; unsafe bases refused with the tab
closed; loopback staying navigable).

## 7. Assistant tab connections and credential lane (CLOUD-032–034, CLOUD-036–037, slice NEWFRONT-660)

CLOUD-032 — The AI Assistant tab manages provider connections. The list
comes from `GET /api/users/me/ai-assistant/connections/` alongside the
server's managed-model options, account-plan labels, provider
sign-in availability, and hosted-lane availability. Each row shows a
kind-appropriate title — the hosted-lane name, the provider-account name,
or the self-supplied endpoint host — an active marker on the selected row,
a model control, and the last-verified time when known. Managed lanes use a
fixed picker (server options, falling back to a small built-in list with
one-line descriptions when the server sends none; the row's current model
stays selectable even when absent from the options; provider display labels
win over descriptions); self-supplied rows use free text with suggestions
that commits on blur or Enter. Row actions: switching the active row posts
to the per-connection select endpoint and confirms; testing the active row
posts to the config test endpoint and reports success or the failure detail
inline; self-supplied rows can be removed (delete endpoint with
confirmation); the provider-account row is permanent — its account is
disconnected via a disconnect endpoint (the row stays) rather than removed.
Plan-backed rows carry a plan-usage marker with an external manage-usage
link; hosted-lane rows carry an included-with-account marker; a
provider-account row with no signed-in account cannot be selected and shows
a sign-in prompt (or a not-available-on-this-deployment note). An empty
list points at adding the first connection. Oracle:
`assistant-connections.spec.ts`.

CLOUD-033 — Adding a connection covers self-supplied keys and provider
sign-in. The key form requires a provider kind (compatible-endpoint vs
single-vendor), a reachable base address for the compatible kind, a model,
and a secret before enabling save; saving posts the trimmed values to
`POST /api/users/me/ai-assistant/connections/`, confirms, and returns to
the list with the new row selected. Provider sign-in reserves a popup
during the click (opener detached), starts the flow via
`POST /api/users/me/ai-assistant/openai-login/`, navigates the popup to the
returned authorization address, and polls
`GET /api/users/me/ai-assistant/openai-login/status/?state=…` every couple
of seconds for up to five minutes from the page (not the row button, which
may unmount when a refetch reports the row connected). Outcomes report
distinctly: completion confirms and refreshes; provider-side failure or
expiry, timeout, blocked popup, a tab closed before authorization, and a
rejected start each report their own error — a rejected start also closes
the empty tab, while transient poll failures are ignored until the
timeout. The whole provider sign-in path hides where the deployment cannot
support it. Oracle: `assistant-add.spec.ts`.

CLOUD-034 — The hosted-model lane degrades gracefully on stale sessions.
The connections answer carries a hosted-lane availability flag (defaulting
to available so older backends never banner everyone); when the session
predates gateway permissions, affected rows carry a notice directing
sign-out-and-back-in — a refresh alone cannot pick up the new permissions —
instead of silently offering a lane that cannot work. Where the deployment
gates the lane off entirely, the list carries no hosted row at all. Oracle:
`assistant-stale.spec.ts`.

CLOUD-036 — The AI Assistant tab keeps shared sections reachable. The
personal tool-server configuration section from the open-source page renders
inside the cloud page (the cloud overlay replaces the whole page, so it
must mount the shared section or cloud users lose it), and a pointer sends
app browsers to the sidebar Apps section instead of duplicating the
catalog. No API. Oracle: `assistant-shared.spec.ts`.

CLOUD-037 — Assistant entry points know which credential lane is active. A
shared hook reads the provider config (`GET
/api/users/me/ai-assistant/config/`, with its credential-lane field and
per-lane credential flags) and reports the active lane — hosted, provider
account, or self-supplied — plus whether setup is still needed for that
lane (the lane carries no credential). Older backends without the lane
field default to the self-supplied lane, and an unloaded config never
reports setup-needed. Oracle: `assistant-lane.spec.ts`.

## 8. Security suppression, plan badge, keyboard and desktop agent (CLOUD-035, CLOUD-038–040, slice NEWFRONT-661)

CLOUD-035 — Password security settings are hidden on cloud. The profile
sidebar omits the security entry and a direct visit to its address forwards
to the general tab (with a neutral loading state meanwhile), since
credentials live in the external account home; the gate reads the instance
config, and self-managed builds keep the tab. Generic tab-shell behavior
belongs to the profile area — this row covers only the cloud suppression.
No API. Oracle: `profile-security.spec.ts` (omitted entry; direct-address
forward; self-managed retention).

CLOUD-038 — No plan badge renders in the sidebar footer. Owner decision
(NEWFRONT-1045, superseding the `bug:` NEWFRONT-474 intended badge): the
sidebar footer holds only the user menu, which spans the full width, in
every plan state — no paid plan name, no migrate prompt, no upgrade label,
no version hover. The running version still surfaces in the help menu and
the product-updates dialog. The old app never mounts the badge either (a
sidebar restructure left the override dead and the open-source menu entry
renders instead); the new frontend likewise renders no badge, and its user
menu carries no Community entry. `GET /api/billing/me/plan/` still serves
its snapshot — the backend is untouched. Oracle: `billing-badge.spec.ts`
(absence pinned on both targets, plus the surviving plan API).

CLOUD-039 — No custom keyboard shortcuts in this area; native behavior
everywhere. Every action is a reachable control (radios, pickers, buttons),
text fields commit on Enter, and dialogs and pagers are ordinary buttons —
adding a connection, editing a model, and firing row actions all complete
with Tab and Enter alone, with focus landing where each step needs it.
Nothing requires learning app-specific keys. No API. Oracle:
`cloud-keyboard.spec.ts`.

CLOUD-040 — The desktop agent explains cloud-only model requirements. The
desktop engine runs on the hosted lane only, and fetches its CSRF token
from the cloud API path (`GET /api/auth/get-csrf-token/` — the same
proxying reason as cloud sign-out). When the engine cannot run, the reason
names the fix, with each situation keeping its own message: feature
disabled on the server; self-supplied lane selected on desktop (directing
the hosted lane while noting the visitor's own key still serves cloud
runs); no model selected; revoked gateway session (sign in again);
desktop-session flow required (sign out and in through the desktop app);
unconnected desktop app (open and sign in to it); stale gateway permissions
(sign out and in again); and a temporarily unavailable gateway (automatic
retry). Related-but-distinct situations are never collapsed into one
message. Oracle: `desktop-agent.spec.ts`.

## 9. Server contracts used by the area

Auth, session and billing (contracts issue NEWFRONT-638):

- `GET /api/auth/sign-in/` — provider handoff. Carries `return_to`
  (app-local path plus query; app-path form on desktop), `return_origin`
  (browser origin, only when it differs from the auth origin), and the
  desktop marker (desktop shell only).
- `POST /api/auth/refresh/` — cookie rotation. No body.
- `GET /api/auth/get-csrf-token/` — CSRF token read for sign-out and the
  desktop agent.
- `POST /api/auth/sign-out/` — session logout. Empty JSON body with the
  CSRF token header.
- `/api/auth/desktop-exchange/` — deep-link code exchange, called by the
  desktop shell (outside the web tree) to mint webview cookies.
- `GET /api/billing/me/plan/` — plan snapshot: payment-state field, plan
  name, paid flag, legacy-migration flag, entitlement limits, and the
  external account-center address.
- Creation endpoints answering 402 with the quota-exceeded error, the
  free-limit code, and the upgrade address.

Catalog and assistant (contracts issue NEWFRONT-641):

- `GET /api/openhub/status/` — catalog gate: connected flag, gateway-scope
  flag, server detail, hub base address.
- `GET /api/openhub/apps/marketplace/` — catalog page: query, cursor,
  page-size parameters; items with install state, suggested action,
  mechanism, and icon sources; next cursor.
- `GET /api/openhub/apps/installations/` and
  `GET /api/openhub/apps/{id}/manifest/` — client methods with no surface
  in this UI (kept for contract completeness).
- `POST` + `DELETE /api/openhub/apps/{id}/install/` — install and remove.
- `POST /api/openhub/apps/{id}/activation/` — activation toggle (client
  method; no control in this UI).
- `GET /api/openhub/apps/{id}/auth/status/` — connection state with the
  key-manifest (fields, setup address, instructions) where applicable.
- `GET /api/openhub/apps/{id}/auth/oauth/begin/?return_path=…` — consent
  handoff address for the pre-opened tab; plus the POST handoff-start
  variant carrying the return path in its body.
- `POST /api/openhub/apps/{id}/auth/api-key/` — credential submit
  (trimmed field map plus optional account hint).
- `GET /api/openhub/apps/icon/?url=…` — proxied icon fallback.
- `GET` + `PATCH /api/users/me/profile/` — namespaced settings bag
  carrying the catalog's agent-tools flag.
- `GET /api/users/me/ai-assistant/connections/` — connection list with
  managed-model options, plan labels, provider sign-in availability, and
  hosted-lane availability; per-connection select, model-update, delete,
  and provider-account disconnect endpoints.
- `POST /api/users/me/ai-assistant/openai-login/` and
  `GET /api/users/me/ai-assistant/openai-login/status/?state=…` —
  provider sign-in start and completion poll.
- `GET /api/users/me/ai-assistant/config/` — provider config with the
  credential-lane field and per-lane credential flags; plus the config
  test endpoint used by the connection test action.

## 10. Intended-behavior deviations (slices implement the target)

- CLOUD-030 / NEWFRONT-473 — transport-failed catalog-status reads: the
  old app renders connected chrome with the no-apps empty state; the new
  frontend renders the status-error panel with its retry (§5).
- CLOUD-038 / NEWFRONT-1045 (retired NEWFRONT-474) — workspace plan badge:
  both apps render no badge; the new frontend's footer holds only the
  user menu, with no Community entry either (§8).

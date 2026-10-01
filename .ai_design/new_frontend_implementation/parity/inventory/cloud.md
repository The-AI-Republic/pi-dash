# Feature inventory: Cloud edition — home, docs, downloads, pricing, login, apps, profile tabs

Area: Cloud edition marketing surface, cloud login/SSO, workspace Apps catalog, cloud profile-tab
deltas (Phase 3). ID prefix `CLOUD-`.
Editions: cloud.
Source behavior learned by reading the cloud overlay (`ee-overlay/apps/web`, 44 files listed in the
coverage checklist below) against the OSS counterparts it replaces. The old app was not run in this
session; the oracle run (NEWFRONT-68) verifies each row against the live app later. Written from
reading notes; no old code, strings, or assets copied. Endpoint paths below are backend contracts.

Row format per the Parity page: ID, capability, who, edition, old entry point, API, acceptance,
parity test (empty for now), status (`not started`).

Generic profile-tab shell behavior (tab switching, layout, loading state) is shared with the OSS page
and belongs to the Profile inventory (NEWFRONT-65); this file covers only cloud deltas. Generic
sign-in/out and onboarding behavior belongs to the Auth inventory (auth.md); this file covers only
what the cloud edition changes or adds.

| ID | Capability | Who | Edition | Old entry point | API | Acceptance | Parity test | Status |
|----|------------|-----|---------|-----------------|-----|------------|-------------|--------|
| CLOUD-001 | Signed-out root URL forwards to the marketing surface while preserving a deep target | Signed-out visitor | cloud | `/` | none | Visiting `/` with no session lands on the marketing home; a `next_path` value survives as a login parameter so the visitor returns to their original target after signing in |  | not started |
| CLOUD-002 | Legacy sign-in URLs resolve to the cloud login experience | Signed-out visitor | cloud | `/login`, `/sign-in`, `/signin` | none | `/login` renders the cloud sign-in page directly; the older `/sign-in` and `/signin` aliases bounce through the root redirect instead of rendering their own screens |  | not started |
| CLOUD-003 | Marketing home page presents the product and routes visitors onward | Signed-out visitor | cloud | `/home` | none | The page shows a hero with entry actions toward sign-in, downloads, and docs, followed by product-loop, capability, open-source, and closing-call sections; every action reaches its destination |  | not started |
| CLOUD-004 | Shared marketing header and footer on every public page | Signed-out visitor | cloud | header/footer on `/home`, `/docs`, `/downloads`, `/pricing` | none | The header links home, downloads, docs, pricing, the public repository, sign-in, and start actions; the footer repeats key links plus terms and privacy on external sites; cramped widths hide secondary links rather than breaking layout; keyboard focus is visible on every link |  | not started |
| CLOUD-005 | Public pages are crawlable with stable metadata | Search crawler; signed-out visitor | cloud | `/`, `/home`, `/docs`, `/pricing`, `/downloads`, sitemap, robots | none | Marketing pages carry index/follow metadata, canonical URLs, and social preview data; the sitemap lists the public pages and robots points at it; the app shell skips the authenticated provider stack on static pages so their content prerenders |  | not started |
| CLOUD-006 | Docs page teaches the zero-to-runner path | Signed-out visitor | cloud | `/docs` | none | The page explains the three-part model (cloud, CLI, runner), then walks project creation, per-OS CLI install commands, device login, runner enrollment, and the optional agent skill, with a link into sign-in and one to the public repository |  | not started |
| CLOUD-007 | Downloads page offers desktop installers per operating system | Signed-out visitor | cloud | `/downloads` | stable release-file URLs | Each supported OS gets a card with architecture and format notes plus a primary download that resolves through a stable latest-release URL; secondary formats are offered where they exist; a notice warns installers are not yet publisher-signed |  | not started |
| CLOUD-008 | Downloads page covers the standalone CLI and the web fallback | Signed-out visitor | cloud | `/downloads` CLI section | stable release-file URL | A separate card offers the Windows CLI installer on its own, explains the post-install sign-in step, links to the CLI setup guide in docs, and points browser-preferring visitors at web sign-in |  | not started |
| CLOUD-009 | Pricing page states the tiers, the free cap, and answers common questions | Signed-out visitor | cloud | `/pricing` | none | The page contrasts the free tier (capped created-issue count, wallet-funded metered usage) with membership (uncapped creation, larger included quotas, external checkout) and an enterprise contact path; the FAQ explains what counts toward the cap, what the wallet covers, and that self-hosting is unaffected |  | not started |
| CLOUD-010 | Hitting the free creation cap surfaces a paywall prompt | Signed-in free-tier user | cloud | any issue/project creation call | creation endpoints; 402 `quota_exceeded` payload with upgrade URL | When the server refuses creation over the free cap, the user gets a single confirmation-style prompt naming the limit; accepting opens the upgrade destination in a new tab; no duplicate prompts stack |  | not started |
| CLOUD-011 | Cloud sign-in page offers a single organizational-account button | Signed-out visitor | cloud | `/login` | `GET /api/auth/sign-in/` with return target (+ origin on split setups) | The card presents one continue action, names the account type, references terms and privacy, and links back to the marketing home; activating it leaves the app for the provider flow carrying the return target |  | not started |
| CLOUD-012 | Returning visitors with a pending target skip the card via silent sign-in | Signed-out visitor with a live provider session | cloud | `/login` with a return target | same sign-in endpoint | When a return target exists, no error is present, and no explicit logout was recorded, the page immediately resumes the provider flow instead of rendering the card |  | not started |
| CLOUD-013 | Return targets survive login but cannot escape the app | Signed-out visitor arriving via deep link | cloud | `/login?next_path=…`, post-login landing | none client-side beyond the parameter | The pre-login path plus query is preserved through the round trip; protocol-relative, off-origin, backslashed, or non-relative values are discarded rather than followed; signing in from the login page itself never loops back onto it |  | not started |
| CLOUD-014 | Explicit logout disables silent sign-in until the next manual attempt | Recently signed-out visitor | cloud | `/login` after sign-out | none (tab-scoped marker) | After signing out, visiting login shows the card instead of auto-resuming; starting a manual sign-in clears the marker so the next visit can be silent again |  | not started |
| CLOUD-015 | Expired sessions refresh transparently; only real auth failures bounce | Signed-in user with an aging session | cloud | any authenticated API call | `POST /api/auth/refresh/` | A 401 first triggers one cookie rotation plus a replay of the original request (shared across concurrent calls); only a rejected refresh/replay sends the user to sign-in with the current location preserved; public pages never bounce; transport or server failures leave the page in place |  | not started |
| CLOUD-016 | Cloud sign-out ends the app session and lands on sign-in | Signed-in user | cloud | sign-out controls | `GET /api/auth/get-csrf-token/`; `POST /api/auth/sign-out/` | Sign-out posts to the API logout view, wipes desktop-local session data where present, records the explicit-logout marker, and performs a full navigation to `/login` so all client state resets; the upstream provider session is intentionally left alone |  | not started |
| CLOUD-017 | Generic auth screens gain an organizational provider button | Signed-out visitor | cloud | sign-in / sign-up cards | same sign-in endpoint as CLOUD-011 | The provider option appears alongside the standard methods and preserves the current page through the round trip without leaking fragments or sensitive query values; inside the desktop shell it opens the system browser with a desktop marker instead of navigating the webview, and refuses loudly when the build lacks its baked API origin |  | not started |
| CLOUD-018 | Desktop sign-in completes through the system browser and a deep link | Signed-out desktop user | cloud | desktop login card; `pidash://auth/callback` | same sign-in endpoint with desktop marker; `/api/auth/desktop-exchange/` | The desktop card opens the system browser (never an embedded webview flow); the provider returns via the app deep link, which the shell exchanges for webview cookies; a blocked browser open surfaces an error instead of hanging |  | not started |
| CLOUD-019 | Desktop shell shows the same cloud sign-in card as the web login page | Signed-out desktop user | cloud | desktop overlay home | same as CLOUD-011 | The desktop entry point renders the shared cloud card rather than a desktop-specific form, so its behavior matches the web login page |  | not started |
| CLOUD-020 | Cloud onboarding asks for no password | Onboarding user | cloud | onboarding profile step | none | The password-setup slot renders nothing and invokes no callbacks, since cloud identities come from the provider |  | not started |
| CLOUD-021 | Workspace sidebar gains an Apps entry for every role, release-gated | Any workspace member incl. guests | cloud | workspace sidebar | none (build flag) | With the catalog flag on, all roles see the entry with its own icon and active highlighting; with the flag off the sidebar is byte-identical to OSS — no disabled row, no placeholder — and the route itself 404s |  | not started |
| CLOUD-022 | Apps page lives in workspace chrome but lists the visitor's own apps | Signed-in user | cloud | `/<workspaceSlug>/apps` | `GET /api/openhub/status/` | The page mounts the standard workspace shell (sidebar, command palette) with the catalog as its content, and states that installations belong to the viewer, not the workspace — two people on the same URL see different lists |  | not started |
| CLOUD-023 | App catalog supports search and cursor paging | Signed-in user | cloud | Apps page search + pager | `GET /api/openhub/apps/marketplace/` (query, cursor, limit) | Typing filters the catalog after a short debounce and resets paging; previous/next step through result windows with a page indicator; changing the query mid-window never mixes result sets |  | not started |
| CLOUD-024 | Installing an app also connects it | Signed-in user | cloud | app card install action | `POST /api/openhub/apps/{id}/install/` then `GET /api/openhub/apps/{id}/auth/status/` | Install commits first and refreshes the list before the connect step starts, so a later failure reads as not-connected rather than not-installed; success, partial, and failure outcomes each produce a distinct notification |  | not started |
| CLOUD-025 | Provider-based connects run in a pre-opened tab | Signed-in user | cloud | app card connect action | `GET /api/openhub/apps/{id}/auth/oauth/begin/?return_path=…` | The tab opens synchronously on click (placeholder content, detached opener) and is then pointed at the server consent handoff; the flow finishes in that tab while this page learns the new state on its next refresh; a blocked popup explains itself instead of failing silently |  | not started |
| CLOUD-026 | Failed connects report once and clean the URL | Signed-in user | cloud | Apps page with `?error=` | none | A connect that never started bounces back with an error parameter, which fires a single failure notification and is then stripped from the URL so later searches do not re-fire it |  | not started |
| CLOUD-027 | Key-based apps connect through a generic credential dialog | Signed-in user | cloud | app card connect action on a manual app | `POST /api/openhub/apps/{id}/auth/api-key/` | The dialog builds its fields from the server manifest (labels, secrets masked, format hints checked, publisher instructions as plain text, issuance link only when safely absolute-HTTPS); empty or malformed values block submit inline; entered secrets are cleared from memory after submit and never travel in URLs |  | not started |
| CLOUD-028 | App cards communicate install and credential state at a glance | Signed-in user | cloud | catalog grid | icon proxy `GET /api/openhub/apps/icon/?url=…` | Installed apps carry an installed marker, installed-but-credentialless ones an unfinished marker (they expose no tools until connected), and per-card busy states guard double clicks; icons prefer the self-hosted graphic with a proxied fallback and a letter tile when both fail |  | not started |
| CLOUD-029 | One switch governs whether agents may use installed apps | Signed-in user | cloud | Apps page toggle | `GET` + `PATCH /api/users/me/profile/` (namespaced settings; `OpenHubService.setAgentToolsEnabled`) | The control states plainly that enabling exposes every installed app's tools — including mutating ones — to every agent run the user starts; the change applies optimistically and rolls back with an error notice if the server refuses |  | not started |
| CLOUD-030 | Unavailable or failing catalog states explain themselves | Signed-in user | cloud | Apps page | `GET /api/openhub/status/`; marketplace fetch | A session that predates catalog access points at sign-out-and-back-in; an unreachable service names itself with a retry; catalog failures show a retry inline and never masquerade as an empty catalog; distinct empty states cover no apps available vs no search matches |  | not started |
| CLOUD-031 | Already-connected provider apps open in the app hub | Signed-in user | cloud | app card on a connected OAuth app | none (URL building only) | Choosing the app navigates the handoff tab to the hub's detail page for it; bases that are not plain HTTPS (or local loopback over HTTP) or that embed credentials are refused |  | not started |
| CLOUD-032 | AI Assistant tab manages provider connections | Signed-in user | cloud | `/settings/profile/ai-assistant` | `GET /api/users/me/ai-assistant/connections/`; select, model-update, delete per connection | Each connection shows its kind-appropriate title, active marker, model control (fixed list for managed kinds, free text with suggestions for custom endpoints), and last-verified time; switching, testing the active one, and removing non-built-in ones all confirm or report errors inline; an empty list points at adding the first connection |  | not started |
| CLOUD-033 | Adding a connection covers self-supplied keys and provider sign-in | Signed-in user | cloud | AI Assistant tab add view | `POST /api/users/me/ai-assistant/connections/`; `POST /api/users/me/ai-assistant/openai-login/` + status poll | The key form requires provider, reachable base for custom endpoints, model, and secret before enabling save; provider sign-in opens a popup and polls for completion with a timeout, reporting blocked popups, failures, and expiry distinctly; the provider-sign-in path hides entirely where the deployment cannot support it |  | not started |
| CLOUD-034 | Hosted-model lane degrades gracefully on stale sessions | Signed-in user with an older session | cloud | AI Assistant tab hosted-model rows | connections response availability flag | When the session predates gateway permissions, affected rows carry a notice directing sign-out-and-back-in (a refresh alone cannot fix it), instead of silently offering a lane that cannot work |  | not started |
| CLOUD-035 | Password security settings are hidden on cloud | Signed-in cloud user | cloud | `/settings/profile/security`, profile sidebar | instance config | The sidebar omits the security entry and a direct visit to its URL forwards to the general tab (with a neutral loading state meanwhile), since credentials are managed by the external account home; self-managed builds keep the tab |  | not started |
| CLOUD-036 | AI Assistant tab keeps shared sections reachable | Signed-in user | cloud | `/settings/profile/ai-assistant` lower sections | none | The personal server-configuration section from OSS renders inside the cloud page, and a pointer sends app browsers to the sidebar Apps section instead of duplicating the catalog |  | not started |
| CLOUD-037 | Assistant entry points know which credential lane is active | Signed-in user | cloud | assistant surfaces | provider-config read | A shared hook reports the active credential lane and whether setup is still needed for that lane, defaulting to the self-supplied lane on older backends |  | not started |
| CLOUD-038 | Workspace badge shows the cloud plan and links out for billing | Signed-in user | cloud | workspace sidebar badge | `GET /api/billing/me/plan/` | The badge labels the current plan (paid name, migrate prompt for legacy plans, upgrade otherwise) and opens the external account center; hovering reveals the app version; the plan refreshes on window focus without redundant re-renders, and fetch failures keep the last known label |  | not started |
| CLOUD-039 | No custom keyboard shortcuts in this area; native behavior everywhere | Keyboard user | cloud | all screens in this area | none | Every action is a reachable control, text fields submit on Enter, and dialogs/pagers are ordinary buttons — nothing requires learning app-specific keys |  | not started |
| CLOUD-040 | Desktop agent explains cloud-only model requirements | Desktop user | cloud | desktop agent status | cloud CSRF path; agent profile classification | When the desktop engine cannot run, the reason names the fix: enablement off, self-supplied keys unsupported on desktop (pointing at the hosted lane while noting keys still serve cloud runs), missing model selection, revoked gateway session, missing desktop session, unconnected app, stale permissions, or a temporarily unavailable gateway — each with its own message |  | not started |

## Coverage checklist

Every route file, top-level component folder/file, service, and API endpoint in the assigned sources,
mapped to rows. Shared OSS shell behavior (profile tab switching/layout, generic sign-in forms,
onboarding flow) is covered by the Profile (NEWFRONT-65) and Auth (auth.md) inventories.

### Route files (`ee-overlay/apps/web/app/**`)

| Source | Covering rows |
|--------|---------------|
| `(home)/page.tsx` (signed-out root redirect) | CLOUD-001 |
| `(home)/layout.tsx` (public route group shell) | CLOUD-003, CLOUD-004, CLOUD-005 |
| `(home)/marketing.tsx` (brand mark, header, footer, page frame, buttons) | CLOUD-003, CLOUD-004 |
| `(home)/world-map.svg` (decorative page backdrop asset; no behavior) | CLOUD-004 |
| `(home)/home/page.tsx` | CLOUD-003 |
| `(home)/docs/page.tsx` | CLOUD-006 |
| `(home)/downloads/page.tsx` | CLOUD-007, CLOUD-008 |
| `(home)/pricing/page.tsx` | CLOUD-009 |
| `(home)/login/page.tsx` (card, silent-SSO trigger, desktop branch) | CLOUD-011, CLOUD-012, CLOUD-013, CLOUD-014, CLOUD-018, CLOUD-019 |
| `(all)/[workspaceSlug]/apps/page.tsx` + `layout.tsx` | CLOUD-021, CLOUD-022 |
| `(all)/settings/profile/[profileTabId]/page.tsx` (security-tab guard; shell rows live in the Profile inventory) | CLOUD-035 |
| `root.tsx` (static-page provider bypass) | CLOUD-005 |
| `routes/extended.ts` (marketing routes; gated apps route) | CLOUD-001 through CLOUD-009, CLOUD-021 |
| `routes/redirects/core/login.tsx` (legacy path serves cloud login) | CLOUD-002 |

### Components, hooks, constants

| Source | Covering rows |
|--------|---------------|
| `core/components/openhub/apps-page.tsx` | CLOUD-022, CLOUD-023, CLOUD-024, CLOUD-025, CLOUD-026, CLOUD-030 |
| `core/components/openhub/app-card.tsx` | CLOUD-028 |
| `core/components/openhub/connect-dialog.tsx` | CLOUD-027 |
| `core/components/openhub/agent-tools-toggle.tsx` | CLOUD-029 |
| `core/components/openhub/connect-navigation.ts` | CLOUD-025, CLOUD-031 |
| `core/components/settings/profile/content/pages/ai-assistant.tsx` | CLOUD-032, CLOUD-033, CLOUD-034, CLOUD-036 |
| `core/components/settings/profile/sidebar/item-categories.tsx` (security-tab suppression) | CLOUD-035 |
| `core/components/assistant/use-llm-config.ts` | CLOUD-037 |
| `core/components/onboarding/steps/profile/set-password.tsx` (renders nothing) | CLOUD-020 |
| `core/hooks/oauth/extended.tsx` (provider button, incl. desktop branch) | CLOUD-017, CLOUD-018 |
| `core/constants/extended-navigation.tsx` (Apps sidebar item) | CLOUD-021 |
| `core/constants/openhub-flags.ts` (build-time release gate) | CLOUD-021 |
| `ce/components/workspace/edition-badge.tsx` | CLOUD-038 |
| `ce/components/desktop/sign-in-card.tsx` (re-exports cloud card) | CLOUD-019 |
| `ce/components/desktop/agent-runtime-edition.ts` (CSRF path, reason messages) | CLOUD-040 |
| `ee/components/.gitkeep` | dead placeholder: no behavior, no row |

### Services

| Source | Covering rows |
|--------|---------------|
| `core/services/openhub.service.ts` | CLOUD-022 through CLOUD-031 (endpoint contracts below) |
| `core/services/silent-sso.ts` | CLOUD-012, CLOUD-013, CLOUD-014, CLOUD-015 |
| `core/services/silent-sso.test.ts` | test for the above rows; no new behavior |
| `core/services/auth-signout.ts` | CLOUD-016 |
| `core/services/billing.service.ts` | CLOUD-038 |
| `core/services/api.service.ts` (refresh/replay interceptor, paywall prompt) | CLOUD-010, CLOUD-015 |

### API endpoints observed from these sources

| Endpoint | Covering rows |
|----------|---------------|
| `GET /api/auth/sign-in/` (return target, origin, desktop marker) | CLOUD-011, CLOUD-017, CLOUD-018 |
| `POST /api/auth/refresh/` | CLOUD-015 |
| `GET /api/auth/get-csrf-token/`; `POST /api/auth/sign-out/` | CLOUD-016 |
| `/api/auth/desktop-exchange/` (deep-link code exchange; called by the shell, referenced by login/desktop code) | CLOUD-018 |
| `GET /api/billing/me/plan/` | CLOUD-038 |
| `GET /api/openhub/status/` | CLOUD-022, CLOUD-030 |
| `GET /api/openhub/apps/marketplace/` (query, cursor, limit) | CLOUD-023 |
| `GET /api/openhub/apps/installations/`; `GET /api/openhub/apps/{id}/manifest/` (client methods; installations/manifest views not surfaced in this UI) | CLOUD-022, CLOUD-028 |
| `POST` + `DELETE /api/openhub/apps/{id}/install/` | CLOUD-024, CLOUD-028 |
| `POST /api/openhub/apps/{id}/activation/` (client method; no UI control in these sources) | CLOUD-028 |
| `GET /api/openhub/apps/{id}/auth/status/` | CLOUD-024, CLOUD-025, CLOUD-027 |
| `GET /api/openhub/apps/{id}/auth/oauth/begin/`; `POST …/auth/oauth/start/` (`OpenHubService.startOAuth`) (handoff URL builders) | CLOUD-025 |
| `POST /api/openhub/apps/{id}/auth/api-key/` | CLOUD-027 |
| `GET /api/openhub/apps/icon/` (icon proxy) | CLOUD-028 |
| `GET` + `PATCH /api/users/me/profile/` (namespaced app-settings) | CLOUD-029 |
| `GET /api/users/me/ai-assistant/connections/`; create/select/model-update/delete per connection | CLOUD-032 |
| `POST /api/users/me/ai-assistant/openai-login/`; `GET …/openai-login/status/` | CLOUD-033 |
| provider-config read (assistant lane state) | CLOUD-037 |
| 402 `quota_exceeded` payload on creation calls (upgrade URL) | CLOUD-010 |

No source mapped to zero rows. No `bug:` rows: nothing observed contradicted its evident intent; the
oracle run marks any such scenario if the live app disagrees. Easy-to-miss behaviors called out as
their own rows: CLOUD-002, CLOUD-005, CLOUD-010, CLOUD-012, CLOUD-013, CLOUD-014, CLOUD-015,
CLOUD-020, CLOUD-021, CLOUD-026, CLOUD-030, CLOUD-034, CLOUD-035, CLOUD-039, CLOUD-040. No
drag-and-drop, exports/imports, or real-time collaboration exist in this area; focus-triggered
refreshes are covered inside CLOUD-025 and CLOUD-038.

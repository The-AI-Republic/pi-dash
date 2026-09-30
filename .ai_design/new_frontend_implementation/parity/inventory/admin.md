# Feature inventory — Instance admin (god-mode) (Phase 3)

- Area: Instance admin console ("god-mode") — first-run instance setup, admin sign-in/sign-out,
  and the instance-wide settings dashboard (general, email, authentication providers, AI, image,
  loop, workspaces). New home is the route subtree `/god-mode/*` in `apps/web_new` with its own layout
  and its own instance-admin session, **web build only** (not desktop).
- ID prefix: `ADM-`
- Editions: oss, cloud — web only. The old admin app is `apps/admin`; cloud overrides live in
  `private-pi-dash/ee-overlay/apps/admin`.
- Status of this file: draft for human sign-off (H-signoff-3 via NEWFRONT-10)
- Method: read of the old sources listed in the coverage checklist (the `(home)` and `(dashboard)`
  route trees, `components`, `store`, `hooks`, `helpers`, `providers`, plus the backing instance/auth/
  workspace/loop services in `packages/services`). The old app was **not** run against a seeded stack
  in this pass (no local backend here); every row must still pass the oracle run (NEWFRONT-95) against
  the live old app before implementation starts. The cloud `private-pi-dash/ee-overlay/apps/admin` tree
  is **not** in this checkout, so cloud-specific behavior could not be read directly; the OSS admin code
  contains **no** runtime `edition`/`isCloud`/`pro` branches, and the only edition seam is a shared-axios
  interceptor hook (`packages/services` `ee/init` + `_axios-setup`, empty in OSS) into which cloud builds
  inject an auth-refresh interceptor. Rows are therefore written from OSS behavior and marked
  "cloud n/c" (needs cloud oracle confirmation) where the overlay could add or change behavior.
  Descriptions are paraphrased; no old strings, code, class names or styles are reused.
- Scope notes:
  - The whole console is gated to a single role, the **signed-in instance admin**, established by
    `GET /api/instances/admins/me/` (a 403 maps to "authentication not done"). There are no member/guest
    roles inside this app; server-side it sits behind an instance-admin permission. "Who" is therefore
    "instance admin" for every dashboard row, and "unauthenticated visitor" / "first user" for the
    setup and sign-in rows.
  - Several settings here change behavior in the **main web app** (`apps/web`), not inside this console:
    enabling auth providers changes the main app's sign-in options; AI/image keys enable features across
    every workspace; workspace-creation and open-sign-up toggles change what non-admins may do. Those
    cross-screen effects are called out per row.
  - Negative rows (ADM-069 … ADM-073) record load-bearing absences the new app must preserve (no realtime,
    no drag-and-drop/exports, no keyboard shortcuts beyond browser/tab-order, web-only/no-desktop, and the
    delete-workspace / delete-loop-confirmation gaps).

| ID | Capability | Who | Edition | Old entry point | API | Acceptance | Parity test | Status |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| ADM-001 | Instance bootstrap gate at the root URL resolves to one of four screens from a single instance-info fetch | unauthenticated visitor | oss, cloud | route `/` (`(home)/page.tsx`) | GET `/api/instances/` (once, `validateStatus:null`, no auto-retry, no revalidate-on-focus) | Exactly one branch renders per the server's reported setup state: a full-screen loader while instance state is unresolved, a fetch-failure screen on error, the first-run setup form when setup is not done, or the admin sign-in form when setup is done | | not started |
| ADM-002 | Full-screen brand loader while instance info or auth state is still resolving | unauthenticated visitor | oss, cloud | `(home)/page.tsx`; `(dashboard)/layout.tsx` while login state is undefined | none (waits on ADM-001 / ADM-009 fetches) | A centered pulsing brand spinner shows only while state is genuinely unknown; it never shows alongside a form or the shell, preventing a flash of the wrong screen | | not started |
| ADM-003 | Instance-info fetch failure screen with manual retry and no automatic retry | unauthenticated visitor | oss, cloud | `components/instance/failure.tsx` when the info fetch errors | GET `/api/instances/` (retry count zero); retry action is a full page reload | A theme-aware illustration explains the instance details could not be fetched and suggests a connectivity cause; a retry control reloads the page, which re-issues the single info fetch; there is no silent auto-retry | | not started |
| ADM-004 | First-run instance-admin creation form (first user becomes admin) | first user (unauthenticated, un-set-up instance) | oss, cloud | `components/instance/setup-form.tsx` | native full-page form POST to `/api/instances/admins/sign-up/`; hidden CSRF from GET `/auth/get-csrf-token/` | Fields for first name, last name, email, company, password, confirm-password and a telemetry opt-in; server creates the admin and marks the instance set up, then redirects; a hidden telemetry flag rides along on the POST | | not started |
| ADM-005 | Setup-form validation and input constraints | first user | oss, cloud | `components/instance/setup-form.tsx` | client (pre-submit) | Submit is blocked unless first name, email and password are non-empty, the password meets the strong-strength level, and confirm matches; name fields reject invalid characters and cap length; company caps length; a live strength meter shows while typing and a live mismatch hint appears for confirm; password fields have independent show/hide toggles kept out of tab order | | not started |
| ADM-006 | Setup-form URL prefill and server-error surfacing via query params | first user | oss, cloud | `components/instance/setup-form.tsx` reading URL params after the server redirect | none (reads redirect params) | First name, last name, company and email prefill from query params; telemetry defaults on unless explicitly disabled; server error codes round-tripped in the URL render inline under the email or password field for invalid-email / invalid-password, and as a top banner for all other codes (not-configured, admin-exists, missing-fields, user-exists) | | not started |
| ADM-007 | Admin sign-in form on a set-up instance | unauthenticated visitor | oss, cloud | `(home)/sign-in-form.tsx` | native full-page form POST to `/api/instances/admins/sign-in/`; hidden CSRF from GET `/auth/get-csrf-token/` | Email and password fields with a show/hide toggle; submit is disabled until both are non-empty and shows a busy state; email is auto-focused and prefilled from a query param; a successful server round-trip establishes the session cookie and lands on the dashboard | | not started |
| ADM-008 | Admin sign-in error surfacing: generic banner plus mapped dismissible banner | unauthenticated visitor | oss, cloud | `(home)/sign-in-form.tsx`, `auth-banner.tsx`, `auth-helpers.tsx` | none (reads redirect params) | Known error codes render a top error banner with the server message; recognized admin auth codes additionally map (client-side) to a dismissible info banner with human-readable copy, some carrying a sign-in link back to the console or a support-email reference; unknown codes render no banner | | not started |
| ADM-009 | Route guards redirect by auth state with a tri-state to avoid screen flashes | all | oss, cloud | `(home)/layout.tsx` and `(dashboard)/layout.tsx` | GET `/api/instances/admins/me/` (drives logged-in state; retry disabled; 403 → auth-not-done) | An authenticated admin on the home/sign-in tree is replace-navigated to the general settings page; an unauthenticated visitor on any dashboard route is replace-navigated to the root; while auth state is unresolved a loader shows; redirects add no history entry | | not started |
| ADM-010 | Admin sign-out from the sidebar account menu with client-state reset | instance admin | oss, cloud | `(dashboard)/sidebar-dropdown.tsx` | native form POST to `/api/instances/admins/sign-out/`; hidden CSRF from GET `/auth/get-csrf-token/` | The server clears the session; the client store also resets (theme back to system, fresh stores, current user cleared) so the dashboard guard bounces the user to the root | | not started |
| ADM-011 | Catch-all 404 screen with a shortcut back into the console | all | oss, cloud | `components/404.tsx` (wildcard route) | none | Any unmatched path renders a not-found illustration with an explanatory message and a single action that navigates to the general settings page | | not started |
| ADM-012 | First-load "create your first workspace" popup when the instance has no workspaces | instance admin | oss, cloud | `components/common/new-user-popup.tsx` (rendered at layout level) | none (flag set by the ADM-001 info fetch when no workspaces exist) | On the first instance-info load that reports no existing workspaces, a bottom-corner card invites creating the first workspace with a shortcut to the create page and a dismiss action; dismissing clears the flag | | not started |
| ADM-013 | Rename the instance | instance admin | oss, cloud | `(dashboard)/general/form.tsx` | PATCH `/api/instances/` | A required text field pre-filled with the current name; on save the name persists and a success notice shows; empty fails validation with an inline required message | | not started |
| ADM-014 | Admin email shown read-only on general settings | instance admin | oss, cloud | `(dashboard)/general/form.tsx` | GET `/api/instances/admins/` (populates upstream) | The first instance admin's email renders in a disabled field; it is display-only and never submitted | | not started |
| ADM-015 | Instance ID shown read-only on general settings | instance admin | oss, cloud | `(dashboard)/general/form.tsx` | GET `/api/instances/` (populates upstream) | The instance's unique ID renders in a disabled field; display-only | | not started |
| ADM-016 | Telemetry (anonymous usage) toggle on general settings | instance admin | oss, cloud | `(dashboard)/general/form.tsx` | PATCH `/api/instances/` (saved with the same form) | A switch with explanatory copy (no PII collected) and an external policy link; the value persists only when the shared save action is pressed, not on flip; disabled while saving; treated as off when unset | | not started |
| ADM-017 | General settings save action commits name and telemetry together | instance admin | oss, cloud | `(dashboard)/general/form.tsx` | PATCH `/api/instances/` | One primary save commits both fields; it shows a busy label while in flight; on success a success notice; on failure only a console log (no visible error notice) — a known OSS quirk the oracle should confirm | | not started |
| ADM-018 | Master email (SMTP) enable/disable toggle in the email page header | instance admin | oss, cloud | `(dashboard)/email/page.tsx` | DELETE `/api/instances/configurations/disable-email-feature/` on turn-off; turn-on reveals the form and is persisted only on save | Initial state derives from the stored SMTP-enabled flag; turning off calls the disable endpoint (optimistically blanking the local SMTP values, restoring them on failure), shows a success or error notice, and hides the form; turning on reveals the config form without a server call | | not started |
| ADM-019 | SMTP configuration form (revealed when email is enabled) | instance admin | oss, cloud | `(dashboard)/email/email-config-form.tsx` | PATCH `/api/instances/configurations/` (always forces the SMTP-enabled flag on) | Required host, port and sender-address fields plus an optional username/password auth section (password has show/hide); save is disabled unless the form is valid and dirty; on success the keys persist and a success notice shows; failure logs to console only (OSS quirk — oracle to confirm) | | not started |
| ADM-020 | Email transport-security selector backed by two mutually-exclusive flags | instance admin | oss, cloud | `(dashboard)/email/email-config-form.tsx` | PATCH `/api/instances/configurations/` (part of the ADM-019 save) | A three-way choice (TLS, SSL, none) maps onto the two boolean TLS/SSL config keys so exactly one or neither is set; the displayed choice derives from whichever key is on, defaulting to none | | not started |
| ADM-021 | Send-test-email modal validates live SMTP settings | instance admin | oss, cloud | `(dashboard)/email/test-email-modal.tsx` | POST `/api/instances/email-credentials-check/` with the receiver address | Opens from a secondary action enabled whenever the form is valid (does not require a prior save); a three-step flow — enter a receiver, then a success step (advising to check spam and re-verify settings) or a failure step showing the server's error message; the modal fully resets its state on close; it sends a real message using the currently entered SMTP settings | | not started |
| ADM-022 | Email page loading states (header pill skeleton and form skeleton) | instance admin | oss, cloud | `(dashboard)/email/page.tsx` | GET `/api/instances/configurations/` | While configurations load, a pill-shaped skeleton stands in for the master toggle and a multi-row skeleton for the form; skeletons never show alongside the real controls | | not started |
| ADM-023 | Open-sign-up toggle on the authentication landing page | instance admin | oss, cloud (affects main web app) | `(dashboard)/authentication/page.tsx` | PATCH `/api/instances/configurations/` (open-signup key) | A switch controls whether users can self-register without an invite; off makes sign-up invite-only in the main web app; a progress notice shows saving then success/failure; the whole switch group dims and disables while any config save is in flight | | not started |
| ADM-024 | Authentication-method registry list in fixed order | instance admin | oss, cloud | `(dashboard)/authentication/page.tsx`, `hooks/oauth/*`, `helpers/authentication.ts` | GET `/api/instances/configurations/` | A list of method cards renders in a fixed order — email magic-link codes, passwords, Google, GitHub, GitLab, Gitea — each with icon, name and description; the magic-link card notes SMTP must be configured first | | not started |
| ADM-025 | Enable/disable each auth method, with a guard preventing disabling the last one | instance admin | oss, cloud (affects main web app) | `(dashboard)/authentication/page.tsx`, `helpers/authentication.ts` | PATCH `/api/instances/configurations/` (per-method enable key) | Each method has its own enable flag; turning one off is blocked when it is the only enabled method (error notice, no API call); the guard applies only to the six method keys, not to open-sign-up; toggles reflect stored state after a save notice | | not started |
| ADM-026 | Auth-method card control adapts to whether the OAuth provider is configured | instance admin | oss, cloud | `(dashboard)/authentication/page.tsx`, `components/authentication/authentication-method-card.tsx` and the per-provider config components | GET `/api/instances/configurations/` | Magic-link and password cards always show only a toggle; an OAuth card that is not yet configured shows a configure action linking to its sub-page, while a configured one shows an edit link plus an enable toggle; "configured" means the required credentials are present (Gitea also requires a host); the GitHub logo swaps with the light/dark theme | | not started |
| ADM-027 | Per-provider OAuth sub-page shell with a master enable toggle in its header | instance admin | oss, cloud | `(dashboard)/authentication/{github,gitlab,gitea,google}/page.tsx` | PATCH `/api/instances/configurations/` (provider enable key) | Each provider page at `/authentication/<provider>` shows a borderless method card with the provider identity and a master enable toggle; toggling persists the enable key with a success notice; the toggle is disabled while saving or before config loads; the body shows a skeleton until config loads; the tab title is provider-specific | | not started |
| ADM-028 | GitHub OAuth credential form with sync toggle and copy-out URLs | instance admin | oss, cloud (affects main web app) | `(dashboard)/authentication/github/form.tsx` | PATCH `/api/instances/configurations/` (client id, secret, org id, sync keys) | Required client id and secret plus an optional organization id; a sync toggle labeled as refreshing user attributes at sign-in; read-only copy fields for the origin URL and the derived callback URL; save persists all fields | | not started |
| ADM-029 | GitLab OAuth credential form with host, sync toggle and callback copy | instance admin | oss, cloud (affects main web app) | `(dashboard)/authentication/gitlab/form.tsx` | PATCH `/api/instances/configurations/` (host, client id, secret, sync keys) | Required host (defaulting placeholder to the public GitLab), application id and secret; a sync toggle; a read-only copy field for the derived callback URL | | not started |
| ADM-030 | Gitea OAuth credential form with host-derived help link and callback copy | instance admin | oss, cloud (affects main web app) | `(dashboard)/authentication/gitea/form.tsx` | PATCH `/api/instances/configurations/` (host, client id, secret, sync keys) | Required host, client id and secret; a sync toggle; a read-only copy field for the derived callback URL; the help link in the field description is built from the currently typed host | | not started |
| ADM-031 | Google OAuth credential form with origin and callback copy | instance admin | oss, cloud (affects main web app) | `(dashboard)/authentication/google/form.tsx` | PATCH `/api/instances/configurations/` (client id, secret, sync keys) | Required client id and secret; a sync toggle; read-only copy fields for the origin URL and the derived callback URL | | not started |
| ADM-032 | Provider "refresh attributes at sign-in" sync toggle, distinct from the enable toggle | instance admin | oss, cloud | the four provider forms (`controller-switch.tsx`) | PATCH `/api/instances/configurations/` (per-provider sync key) | A separate switch from the master enable, controlling whether user attributes are refreshed from the provider on each sign-in; stored as a string flag | | not started |
| ADM-033 | Copy-to-clipboard fields for origin/callback URLs | instance admin | oss, cloud | the four provider forms (`components/common/copy-field.tsx`) | client (clipboard) | Clicking a copy field writes the shown URL to the clipboard and shows an informational notice; the base origin is the configured API base or the current window origin, and callback URLs are derived as `<origin>/auth/<provider>/callback/` | | not started |
| ADM-034 | Unsaved-changes discard guard when leaving a provider form dirty | instance admin | oss, cloud | the four provider forms (`components/common/confirm-discard-modal.tsx`) | client | The back action links to the authentication landing page; if the form is dirty the navigation is intercepted and a confirm-discard modal offers keep-editing or leave-and-discard; a clean form navigates directly | | not started |
| ADM-035 | Provider-form save gating and reset-to-server-values behavior | instance admin | oss, cloud | the four provider forms | PATCH `/api/instances/configurations/` | Save is disabled unless the form is dirty and shows a busy label; on success a success notice fires and the form's default values are re-read from the server response so it is no longer dirty | | not started |
| ADM-036 | Authentication landing-page loading skeleton | instance admin | oss, cloud | `(dashboard)/authentication/page.tsx` | GET `/api/instances/configurations/` | Before config loads, the page shows a multi-bar skeleton in place of the sign-up toggle and the method cards | | not started |
| ADM-037 | Restrict-workspace-creation toggle | instance admin | oss, cloud (affects main web app) | `(dashboard)/workspace/page.tsx` | PATCH `/api/instances/configurations/` (disable-workspace-creation key) | A switch that, when on, prevents anyone but the admin from creating workspaces; a progress notice shows saving then success/failure; the row is a skeleton while config loads and the group disables while saving | | not started |
| ADM-038 | List every workspace on the instance with a count and per-row detail | instance admin | oss, cloud | `(dashboard)/workspace/page.tsx`, `components/workspace/list-item.tsx` | GET `/api/instances/workspaces/` (cursor-paginated) | A titled section with a count badge; each row shows a logo (image or a lettered fallback), the workspace name and slug (slug has an explanatory tooltip), the owner email when present, and project/member counts when present | | not started |
| ADM-039 | Workspace row deep-links into the main web app in a new tab | instance admin | oss, cloud (opens main web app) | workspace list rows | client (external navigation) | Each row is an external link opening the main web app at the workspace's slug in a new tab; there is no in-console workspace detail | | not started |
| ADM-040 | Load-more pagination for the workspace list | instance admin | oss, cloud | `(dashboard)/workspace/page.tsx` | GET `/api/instances/workspaces/?cursor=<next>` | When more results exist a load-more control appends the next page; it shows a busy state and disables while paginating | | not started |
| ADM-041 | Workspace list loading and mutation states | instance admin | oss, cloud | `(dashboard)/workspace/page.tsx` | GET `/api/instances/workspaces/` | The whole section is a skeleton on first load; a small spinner appears next to the count during pagination/refresh; an empty instance shows a zero count with no rows and no special empty illustration | | not started |
| ADM-042 | Workspace management limits surfaced to the admin (no delete/edit) | instance admin | oss, cloud | `(dashboard)/workspace/page.tsx` | none | A helper line states workspaces cannot be deleted here and can only be opened if the admin is a member; rows expose only the open-in-new-tab link, no delete or edit action | | not started |
| ADM-043 | Create-workspace form with name, auto-derived editable slug and required org size | instance admin | oss, cloud | `(dashboard)/workspace/create/form.tsx` | (validation client-side; see ADM-044/045 for the calls) | The slug prefixes the web base URL and auto-derives from the name (lowercased, spaces to hyphens) while remaining independently editable and forced lowercase; org size is a required select; name and slug are validated, slug accepts only alphanumerics, hyphen and underscore; submit is disabled until valid and shows a busy label | | not started |
| ADM-044 | Slug-availability check gates workspace creation | instance admin | oss, cloud | `(dashboard)/workspace/create/form.tsx` | GET `/api/instances/workspace-slug-check/?slug=<slug>` then POST `/api/instances/workspaces/` | On submit the slug is checked first; creation proceeds only if the slug is available and not in the restricted-URL list, otherwise an inline "URL taken" error shows and nothing is created | | not started |
| ADM-045 | Create a workspace and return to the list; owner assigned server-side | instance admin | oss, cloud | `(dashboard)/workspace/create/form.tsx` | POST `/api/instances/workspaces/` | On success a success notice shows and the app redirects to the workspace list; on create or slug-check failure an error notice shows and the user stays; no owner field is exposed — ownership is assigned by the server and later shown in the list row; typed values persist on unmount | | not started |
| ADM-046 | Configure the OpenAI LLM model and API key for the whole instance | instance admin | oss, cloud (affects every workspace) | `(dashboard)/ai/form.tsx` | GET `/api/instances/configurations/`; PATCH `/api/instances/configurations/` (model and key) | A model text field and a password API-key field (with show/hide) under an OpenAI heading with helper links; both are optional so an empty save is allowed and the save action is always enabled; on success a success notice; configuring enables AI features across all workspaces; save failure logs to console only (OSS quirk — oracle to confirm) | | not started |
| ADM-047 | AI page other-vendor informational callout | instance admin | oss, cloud | `(dashboard)/ai/form.tsx` | none | A static callout invites admins who prefer a different AI vendor to make contact; it is messaging only, with no configurable alternative provider on this page | | not started |
| ADM-048 | AI page loading skeleton | instance admin | oss, cloud | `(dashboard)/ai/page.tsx` | GET `/api/instances/configurations/` | A header/grid/button skeleton shows until configurations load | | not started |
| ADM-049 | Configure the Unsplash access key for third-party image search | instance admin | oss, cloud (affects every user) | `(dashboard)/image/form.tsx` | GET `/api/instances/configurations/`; PATCH `/api/instances/configurations/` (Unsplash key) | A required password field (with show/hide) with a helper link to Unsplash developer docs; on success a success notice; it enables third-party image search for users; the save relies on field-level required validation and is otherwise always clickable; save failure logs to console only (OSS quirk — oracle to confirm) | | not started |
| ADM-050 | Image page loading skeleton | instance admin | oss, cloud | `(dashboard)/image/page.tsx` | GET `/api/instances/configurations/` | A two-row skeleton shows until configurations load | | not started |
| ADM-051 | List loop (auto project management) jobs in a table | instance admin | oss, cloud | `(dashboard)/loop/page.tsx` | GET `/api/instances/loop/jobs/` | A table with columns for name (linking to the job detail), slug, recurrence rule, minimum role (numeric role mapped to admin/member/guest labels), builtin yes/no and an inline enabled toggle | | not started |
| ADM-052 | Toggle a loop job enabled/disabled inline with optimistic update and rollback | instance admin | oss, cloud | `(dashboard)/loop/page.tsx` | PATCH `/api/instances/loop/jobs/{id}/` (enabled flag) | The toggle flips optimistically then revalidates; on failure an error notice shows and the state rolls back | | not started |
| ADM-053 | Loop list empty state | instance admin | oss, cloud | `(dashboard)/loop/page.tsx` | GET `/api/instances/loop/jobs/` | With no jobs the list shows a short inline message prompting creation of the first job (an inline paragraph, not the shared empty-state component) | | not started |
| ADM-054 | Loop list loading skeleton | instance admin | oss, cloud | `(dashboard)/loop/page.tsx` | GET `/api/instances/loop/jobs/` | Row-height skeleton bars show while the list loads | | not started |
| ADM-055 | Create a loop job via a modal | instance admin | oss, cloud | `(dashboard)/loop/page.tsx`, `(dashboard)/loop/form-modal.tsx` | POST `/api/instances/loop/jobs/` | A header action opens a create modal with fields for slug, admin name, user-facing name and description, prompt, minimum role (defaulting to member), timezone (defaulting to UTC) and a recurrence rule (defaulting to a daily rule); on save the modal closes and the list revalidates | | not started |
| ADM-056 | Edit a loop job via the same modal, with builtin slug locked | instance admin | oss, cloud | `(dashboard)/loop/form-modal.tsx`, `(dashboard)/loop/detail.tsx` | PATCH `/api/instances/loop/jobs/{id}/` | The modal opens pre-filled; the slug field is disabled for builtin jobs; on save the modal closes and the job/list revalidates | | not started |
| ADM-057 | Loop job modal validation and error surfacing | instance admin | oss, cloud | `(dashboard)/loop/form-modal.tsx` | POST/PATCH `/api/instances/loop/jobs/` | Save is disabled unless slug, admin name, user-facing name and prompt are all non-blank; description, min role, timezone and recurrence have defaults and are not gated; a save failure shows an error notice preferring the server-provided detail, then error, then a generic fallback; the modal body scrolls within a capped height | | not started |
| ADM-058 | Loop job detail with per-run stat tiles and a targets table | instance admin | oss, cloud | `(dashboard)/loop/detail.tsx` | GET `/api/instances/loop/jobs/{id}/`; GET `/api/instances/loop/jobs/{id}/targets/` | The page shows the job, four stat tiles (targets, and 24h completed/failed/skipped) when stats are present, and a targets table with columns for workspace, user, next run, last run status, last-run tokens and last skip reason; unloaded jobs show a skeleton and empty targets show a placeholder row | | not started |
| ADM-059 | Filter loop targets by skip reason | instance admin | oss, cloud | `(dashboard)/loop/detail.tsx` | GET `/api/instances/loop/jobs/{id}/targets/?skip_reason=<reason>` | A dropdown of skip reasons (blank for all, plus a fixed set such as user-disabled, master-paused, min-role, missing-LLM-config, membership-gone, active-turn and dispatch-error) refetches the targets filtered by the chosen reason | | not started |
| ADM-060 | Delete a loop job with no confirmation | instance admin | oss, cloud | `(dashboard)/loop/detail.tsx` | DELETE `/api/instances/loop/jobs/{id}/` | A danger-styled delete action deletes immediately with no confirmation dialog; on success a notice shows and the app returns to the list; on failure an error notice shows | | not started |
| ADM-061 | Loop detail loading skeleton | instance admin | oss, cloud | `(dashboard)/loop/detail.tsx` | GET `/api/instances/loop/jobs/{id}/` | Skeleton bars show under a placeholder header until the job loads | | not started |
| ADM-062 | Dashboard sidebar navigation to each settings area | instance admin | oss, cloud | `(dashboard)/sidebar-menu.tsx`, `hooks/use-sidebar-menu/*` | none | The sidebar lists general, email, authentication, workspaces, AI, loop and image, each with icon, name and description, routing to the matching page; the active item is highlighted by path prefix; no per-item permission or edition gating | | not started |
| ADM-063 | Sidebar collapse with persistence and responsive auto-collapse | instance admin | oss, cloud | `(dashboard)/sidebar.tsx`, `store/theme.store.ts` | none (localStorage) | A collapse control narrows the sidebar to icons with hover tooltips; the collapsed state persists in localStorage; the sidebar auto-collapses at narrow viewports and on outside click on small screens, and is off-canvas on mobile | | not started |
| ADM-064 | Account dropdown showing admin identity | instance admin | oss, cloud | `(dashboard)/sidebar-dropdown.tsx` | GET `/auth/get-csrf-token/` (for the embedded sign-out form) | The dropdown shows the admin label, avatar and email and hosts the theme switch and sign-out; it renders only once the current user is loaded, and works both expanded and collapsed | | not started |
| ADM-065 | Light/dark theme toggle | instance admin | oss, cloud | `(dashboard)/sidebar-dropdown.tsx`, `store/theme.store.ts` | none (persisted client-side) | A menu item flips between light and dark; the label reflects the target mode; the choice persists and resets to system on sign-out | | not started |
| ADM-066 | Header breadcrumb trail derived from the path | instance admin | oss, cloud | `components/common/header/*`, `components/common/breadcrumb-link.tsx` | none | A leading settings crumb links to general settings; subsequent crumbs come from path segments (last segment dropped), each linking to its cumulative path; segment labels come from a fixed label map with an uppercased fallback; long labels truncate with a tooltip | | not started |
| ADM-067 | Sidebar help section with external links and instance version | instance admin | oss, cloud | `(dashboard)/sidebar-help-section.tsx` | none | A control opens the main web app, a help popover offers documentation, forum and bug-report links (opening externally), and the popover footer shows the running instance version; controls restack with tooltips when the sidebar is collapsed | | not started |
| ADM-068 | Mobile hamburger toggles the sidebar | instance admin | oss, cloud | `components/common/header/index.tsx`, `(dashboard)/sidebar.tsx` | none | On small screens a hamburger control shows/hides the off-canvas sidebar | | not started |
| ADM-069 | No real-time updates: settings reflect server state only on fetch/save | instance admin | oss, cloud | all dashboard pages | none (SWR fetch-on-mount only) | No websocket/SSE/polling exists; the AI/image/email pages share one cached configurations fetch, and lists refresh only via explicit revalidation after a mutation — the new app must not silently add live updates that change parity | | not started |
| ADM-070 | No drag-and-drop, import or export anywhere in the console | instance admin | oss, cloud | whole area | none | The console has no drag-reorder, no file import and no export; this absence is intentional and must be preserved | | not started |
| ADM-071 | No custom keyboard shortcuts beyond browser/tab-order and form defaults | instance admin | oss, cloud | whole area | none | Navigation and forms rely only on native focus order, Enter-to-submit and show/hide toggles removed from tab order; there are no bespoke keyboard shortcuts to reproduce | | not started |
| ADM-072 | The console is web-only and absent from the desktop build | instance admin | oss, cloud (web only) | whole area | none | The god-mode console ships only in the web build; the desktop app never mounts it, so no desktop parity rows apply | | not started |
| ADM-073 | Destructive actions lack confirmation where the old app omits it (loop delete; no workspace delete) | instance admin | oss, cloud | `(dashboard)/loop/detail.tsx`, `(dashboard)/workspace/page.tsx` | DELETE `/api/instances/loop/jobs/{id}/` | Loop-job deletion happens immediately with no confirm step; workspace deletion is not offered at all — the new app must preserve these as-is unless a linked bug row changes them | | not started |

## Coverage checklist

Every route file, top-level component folder and API endpoint in the named sources, mapped to the rows
that cover it. Anything mapping to no row is flagged as a missing row or explained as dead code.

### Route files — `apps/admin/app/(all)/(home)`

| File | Covering rows | Notes |
| --- | --- | --- |
| `(home)/page.tsx` | ADM-001, ADM-002, ADM-012 | bootstrap branch selector; also triggers the new-user popup flag |
| `(home)/layout.tsx` | ADM-002, ADM-009 | authenticated-visitor redirect + loader |
| `(home)/sign-in-form.tsx` | ADM-007, ADM-008 | admin sign-in form + error banners |
| `(home)/auth-banner.tsx` | ADM-008 | dismissible mapped-error banner |
| `(home)/auth-header.tsx` | ADM-003, ADM-004, ADM-007 | shared logo header on the unauthenticated screens; presentational, no standalone behavior |
| `(home)/auth-helpers.tsx` | ADM-008 | client mapping of admin auth error codes to copy/CTAs |

### Route files — `apps/admin/app/(all)/(dashboard)`

| File | Covering rows | Notes |
| --- | --- | --- |
| `(dashboard)/layout.tsx` | ADM-002, ADM-009, ADM-012 | auth gate + shell + hosts the new-user popup |
| `(dashboard)/sidebar.tsx` | ADM-063, ADM-068 | collapse/responsive container |
| `(dashboard)/sidebar-menu.tsx` | ADM-062 | nav items |
| `(dashboard)/sidebar-dropdown.tsx` | ADM-010, ADM-064, ADM-065 | account menu, sign-out, theme toggle |
| `(dashboard)/sidebar-help-section.tsx` | ADM-067 | help popover + version + open-main-app + collapse control |
| `(dashboard)/general/page.tsx` + `general/form.tsx` | ADM-013, ADM-014, ADM-015, ADM-016, ADM-017 | general settings |
| `(dashboard)/email/page.tsx` | ADM-018, ADM-022 | email master toggle + loading |
| `(dashboard)/email/email-config-form.tsx` | ADM-019, ADM-020 | SMTP form + security selector |
| `(dashboard)/email/test-email-modal.tsx` | ADM-021 | test-email flow |
| `(dashboard)/authentication/page.tsx` | ADM-023, ADM-024, ADM-025, ADM-026, ADM-036 | landing: sign-up toggle, method list, per-method enable, card controls, loading |
| `(dashboard)/authentication/github/page.tsx` + `github/form.tsx` | ADM-027, ADM-028, ADM-032, ADM-033, ADM-034, ADM-035 | GitHub provider page + form |
| `(dashboard)/authentication/gitlab/page.tsx` + `gitlab/form.tsx` | ADM-027, ADM-029, ADM-032, ADM-033, ADM-034, ADM-035 | GitLab provider page + form |
| `(dashboard)/authentication/gitea/page.tsx` + `gitea/form.tsx` | ADM-027, ADM-030, ADM-032, ADM-033, ADM-034, ADM-035 | Gitea provider page + form |
| `(dashboard)/authentication/google/page.tsx` + `google/form.tsx` | ADM-027, ADM-031, ADM-032, ADM-033, ADM-034, ADM-035 | Google provider page + form |
| `(dashboard)/ai/page.tsx` + `ai/form.tsx` | ADM-046, ADM-047, ADM-048 | AI settings |
| `(dashboard)/image/page.tsx` + `image/form.tsx` | ADM-049, ADM-050 | image settings |
| `(dashboard)/loop/page.tsx` | ADM-051, ADM-052, ADM-053, ADM-054, ADM-055 | loop list |
| `(dashboard)/loop/form-modal.tsx` | ADM-055, ADM-056, ADM-057 | create/edit modal |
| `(dashboard)/loop/detail.tsx` | ADM-056, ADM-058, ADM-059, ADM-060, ADM-061, ADM-073 | loop detail |
| `(dashboard)/workspace/page.tsx` | ADM-037, ADM-038, ADM-039, ADM-040, ADM-041, ADM-042, ADM-073 | workspace list + creation-restriction toggle |
| `(dashboard)/workspace/create/page.tsx` + `create/form.tsx` | ADM-043, ADM-044, ADM-045 | create-workspace form |

### Route registration and framework plumbing

| File | Covering rows | Notes |
| --- | --- | --- |
| `app/routes.ts` | ADM-001, ADM-009, ADM-011, ADM-062 | route table (home, dashboard subtree, wildcard 404) |
| `app/root.tsx`, `app/entry.client.tsx` | — | framework bootstrap (document shell, client entry); no user-observable behavior of its own |
| `app/compat/next/*` (`link.tsx`, `image.tsx`, `navigation.ts`, `helper.ts`) | — | Next.js→react-router compatibility shims; plumbing, no behavior |
| `app/components/404.tsx` | ADM-011 | catch-all screen |
| `app/types/*` | — | type definitions only |

### Top-level component folders — `apps/admin/components`

| Folder / file | Covering rows | Notes |
| --- | --- | --- |
| `components/authentication/authentication-method-card.tsx` | ADM-024, ADM-026 | method card |
| `components/authentication/email-config-switch.tsx` | ADM-025 | magic-link enable control |
| `components/authentication/password-config-switch.tsx` | ADM-025 | password enable control |
| `components/authentication/github-config.tsx` | ADM-026, ADM-028 | GitHub card body / config panel |
| `components/authentication/gitlab-config.tsx` | ADM-026, ADM-029 | GitLab card body / config panel |
| `components/authentication/gitea-config.tsx` | ADM-026, ADM-030 | Gitea card body / config panel |
| `components/authentication/google-config.tsx` | ADM-026, ADM-031 | Google card body / config panel |
| `components/common/banner.tsx` | ADM-006, ADM-008 | inline success/error banner (used by setup + sign-in) |
| `components/common/breadcrumb-link.tsx` | ADM-066 | breadcrumb item with truncation/tooltip |
| `components/common/code-block.tsx` | ADM-028, ADM-030, ADM-031 | inline styled snippet used in OAuth-form help text (github/gitea/gitlab/google) |
| `components/common/confirm-discard-modal.tsx` | ADM-034 | unsaved-changes guard (OAuth forms) |
| `components/common/controller-input.tsx` | ADM-005, ADM-013, ADM-019, ADM-028, ADM-029, ADM-030, ADM-031, ADM-046, ADM-049 | shared labeled input with required-validation and password show/hide |
| `components/common/controller-switch.tsx` | ADM-032 | shared labeled toggle (provider sync) |
| `components/common/copy-field.tsx` | ADM-033 | copy-to-clipboard URL field |
| `components/common/empty-state.tsx` | — | shared empty-state component; not referenced by any admin screen (loop uses an inline message, ADM-053) — dead code for parity purposes |
| `components/common/header/*` (`index.tsx`, `core.ts`, `extended.ts`) | ADM-066, ADM-068 | header breadcrumbs (label map in `core.ts`; `extended.ts` map is empty) + mobile hamburger |
| `components/common/logo-spinner.tsx` | ADM-002 | full-screen loader |
| `components/common/new-user-popup.tsx` | ADM-012 | first-workspace popup |
| `components/common/page-header.tsx` | ADM-062 and per-page rows | sets document title/meta only; metadata helper, no visible chrome |
| `components/common/page-wrapper.tsx` | all dashboard settings rows | shared header band + scroll body; presentational shell |
| `components/instance/failure.tsx` | ADM-003 | instance fetch-failure screen |
| `components/instance/form-header.tsx` | ADM-004, ADM-007 | shared setup/sign-in heading; presentational |
| `components/instance/setup-form.tsx` | ADM-004, ADM-005, ADM-006 | first-run admin creation |
| `components/instance/instance-not-ready.tsx` | — | defined but not referenced anywhere in the admin app — dead code (implies an intended `/setup?auth_enabled=0` deep link that the live flow does not use; the inline setup form ADM-004 is used instead) |
| `components/instance/loading.tsx` | — | defined but not referenced; the live loading states use the logo spinner (ADM-002) — dead code |
| `components/workspace/list-item.tsx` | ADM-038, ADM-039 | workspace row |

### Client state, hooks, helpers, providers

| Module | Covering rows | Notes |
| --- | --- | --- |
| `store/instance.store.ts` | ADM-001, ADM-003, ADM-012, ADM-067 | instance info fetch, error state, new-user-popup flag trigger, version |
| `store/user.store.ts` | ADM-009, ADM-010, ADM-014, ADM-064 | current-admin fetch, logged-in tri-state, sign-out reset, admin list |
| `store/theme.store.ts` | ADM-063, ADM-065, ADM-012 | theme, sidebar-collapsed persistence, new-user-popup flag |
| `store/workspace.store.ts` | ADM-038, ADM-040, ADM-043, ADM-044, ADM-045 | workspace list/pagination/create/slug-check |
| `store/root.store.ts` | ADM-010 | root store + sign-out reset orchestration |
| `hooks/store/*` (`use-instance`, `use-user`, `use-theme`, `use-workspace`, `index`) | ADM-001, ADM-009, ADM-038, ADM-063 | thin store bindings; no standalone behavior |
| `hooks/oauth/*` (`core.tsx`, `index.ts`, `types.ts`) | ADM-024, ADM-026 | auth-method registry (names, descriptions, icons, enable keys, config panels) |
| `hooks/use-sidebar-menu/*` (`core.ts`, `index.ts`, `types.ts`) | ADM-062 | sidebar menu item definitions and ordering |
| `helpers/authentication.ts` | ADM-024, ADM-025 | mode map + last-method-disable guard |
| `providers/core.tsx`, `providers/index.tsx`, `providers/extended.tsx` | ADM-001, ADM-009 | provider nesting; `extended` is an empty passthrough seam (no behavior) |
| `providers/instance.provider.tsx` | ADM-001, ADM-003 | one-shot instance-info SWR fetch |
| `providers/user.provider.tsx` | ADM-009, ADM-063 | current-user + admins SWR fetch; restores sidebar-collapsed pref |
| `providers/store.provider.tsx` | — | MobX store context wiring; plumbing, no behavior |
| `providers/toast.tsx` | success/error notices across rows | toast host; the notice behavior is rowed with each action |
| `lib/b-progress/*` (`AppProgressBar.tsx`, `index.tsx`) | — | top navigation progress bar; presentational plumbing, no parity behavior |
| `utils/public-asset.ts` | — | static asset path helper; no behavior |
| `app/assets/*` (images, logos, favicon, instance) | presentational for ADM-002/003/011/012 | illustrations/logos only, no behavior |

### API endpoints

| Endpoint | Covering rows |
| --- | --- |
| GET `/api/instances/` (instance info bootstrap) | ADM-001, ADM-003, ADM-015 |
| PATCH `/api/instances/` (update instance name/telemetry) | ADM-013, ADM-016, ADM-017 |
| GET `/api/instances/admins/` (list instance admins) | ADM-014 |
| GET `/api/instances/admins/me/` (current admin / auth check) | ADM-009 |
| POST `/api/instances/admins/sign-up/` (first-run admin creation) | ADM-004 |
| POST `/api/instances/admins/sign-in/` (admin sign-in) | ADM-007 |
| POST `/api/instances/admins/sign-out/` (admin sign-out) | ADM-010 |
| GET `/auth/get-csrf-token/` (CSRF for the native auth forms) | ADM-004, ADM-007, ADM-010, ADM-064 |
| GET `/api/instances/configurations/` (fetch all config key/values) | ADM-019, ADM-022, ADM-024, ADM-027, ADM-036, ADM-046, ADM-048, ADM-049, ADM-050, ADM-069 |
| PATCH `/api/instances/configurations/` (update config keys) | ADM-018, ADM-019, ADM-020, ADM-023, ADM-025, ADM-027, ADM-028, ADM-029, ADM-030, ADM-031, ADM-032, ADM-035, ADM-037, ADM-046, ADM-049 |
| DELETE `/api/instances/configurations/disable-email-feature/` (disable email) | ADM-018 |
| POST `/api/instances/email-credentials-check/` (send test email) | ADM-021 |
| GET `/api/instances/workspaces/` (list workspaces, cursor-paginated) | ADM-038, ADM-040, ADM-041 |
| GET `/api/instances/workspace-slug-check/?slug=` (slug availability) | ADM-044 |
| POST `/api/instances/workspaces/` (create workspace) | ADM-044, ADM-045 |
| GET `/api/instances/loop/jobs/` (list loop jobs) | ADM-051, ADM-054 |
| POST `/api/instances/loop/jobs/` (create loop job) | ADM-055 |
| GET `/api/instances/loop/jobs/{id}/` (loop job detail) | ADM-058, ADM-061 |
| PATCH `/api/instances/loop/jobs/{id}/` (update job / inline enable) | ADM-052, ADM-056 |
| DELETE `/api/instances/loop/jobs/{id}/` (delete loop job) | ADM-060, ADM-073 |
| GET `/api/instances/loop/jobs/{id}/targets/?skip_reason=` (job targets) | ADM-058, ADM-059 |
| GET `/api/instances/changelog/` | — dead code for this area: exists on the instance service but has no caller in `apps/admin` |
| POST `/auth/sign-out/` (via `AuthService.signOut(baseUrl)`) | — dead code for this area: the console's sign-out uses the `/api/instances/admins/sign-out/` form (ADM-010), not this helper path |
| POST `/auth/magic-generate/`, `/auth/email-check/`, `/auth/forgot-password/`, `/auth/set-password/` | — not used by the admin app: end-user auth-service helpers reachable through the shared service classes but not called by any admin screen |
| external: `WEB_BASE_URL/<slug>`, `WEB_BASE_URL/`, provider OAuth-console and project GitHub links | ADM-039, ADM-067; provider help links supporting ADM-028…ADM-031 | browser navigations to the main app / external sites, not admin API calls |
| external: `<origin>/auth/{github,gitlab,gitea,google}/callback/` | ADM-033 | callback URLs shown as copy-out fields, pasted into external provider consoles; not called by this app |

### Sweeps with no admin behavior (no rows; confirmed absent)

- Real-time channels (websocket/SSE/polling): none — the stores fetch on mount via SWR and revalidate only
  after a mutation. Covered by ADM-069.
- Drag-and-drop, exports, imports: none anywhere in the console. Covered by ADM-070.
- Keyboard shortcuts beyond native focus order / Enter-to-submit / show-hide toggles removed from tab order:
  none. Covered by ADM-071.
- Desktop build: the console is web-only and is never mounted by the desktop app. Covered by ADM-072.
- Edition (OSS vs cloud) runtime branches: none in `apps/admin`. The only seam is the shared-axios EE
  interceptor hook (`packages/services` `ee/init` + `_axios-setup`, empty in OSS) used by cloud builds to
  add an auth-refresh interceptor; the cloud `private-pi-dash/ee-overlay/apps/admin` tree is not in this
  checkout, so any cloud-only screens/overrides must be confirmed by the oracle run (NEWFRONT-95) and added
  here as rows if found.

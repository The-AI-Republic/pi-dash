# Workspace settings area spec — general, members, integrations, webhooks, tokens, billing, exports entry (WSET-001–041)

Parent epic NEWFRONT-89. Inventory
`.ai_design/new_frontend_implementation/parity/inventory/workspace-settings.md`
(rows WSET-001–041, all oracle green). Oracle scenarios
`apps/web_new/e2e/parity/workspace-settings/` (`shell-general.spec.ts`,
`members.spec.ts`, `developer.spec.ts`, `api-tokens.spec.ts`,
`billing.spec.ts`, `cross-cutting.spec.ts`, green on `apps/web` via
NEWFRONT-410–415).

Method: the old frontend was read as a behavior reference only — the
workspace settings routes, the workspace/integration/web-hooks/api-token/
license/instance component trees (core plus the cloud-edition overrides),
the backing services and stores, and the permission constants. Nothing is
imported, copied or translated from those sources; this spec restates what
was learned in prose. The only code-like tokens below are API paths, query
parameter names and payload field names, which the contracts issues
(NEWFRONT-532, NEWFRONT-533) turn into zod contracts. New code is written
from this spec, the wiki pages (Reference, not source; Parity;
Architecture; Data layer; Platform and editions) and the kit — never with
old files open.

Status convention: this spec owns no inventory Status changes. Slices
append `built` to the rows they implement.

Pinned-text convention: where the oracle asserts exact user-visible text,
this spec quotes it exactly and the new UI renders it exactly — those
strings are behavior the gate checks. Everything else is paraphrased and
may be redesigned.

Observed-vs-target convention: where the running old app contradicts the
inventory row text, the oracle pinned the observed behavior and filed a bug
issue. Those deviations are recorded in §43. Slices implement the target
stated in each section below; the gate proves the target.

Row-coverage note: the split (NEWFRONT-89) assigned WSET-001–040 to
slices; the inventory has since grown WSET-041 (webhook list-row switch),
which no slice owns yet. This spec covers it in §24; its natural owner is
the webhooks slice (NEWFRONT-541), which builds the list the switch lives
in — that run should implement it and mark it built.

## 1. Settings shell, sidebar, gates (WSET-001, WSET-002, slice NEWFRONT-534)

Workspace settings live under `/{workspaceSlug}/settings/` with one page
per section: general (`settings/`), members, billing, exports, webhooks,
integrations. The workspace menu header carries a Settings entry that
lands on the general page (the entry itself is shell chrome; this area
owns only the landing behavior). Oracle: `shell-general.spec.ts`,
`cross-cutting.spec.ts` (deep links, §36).

The sidebar groups entries under Administration (General, Members,
Billing & Plans, Exports) and Developer (Webhooks, Integrations). A
third group, Features, exists in the grouping but holds no entries and
never renders. Entries the current role may not open are hidden, and a
group with no visible entries is hidden as a whole: admins see both
groups with every entry; members see Administration with General,
Members and Exports only; guests see no entries at all. The entry for
the current section reads active, and following an entry navigates to
its address and moves the highlight. The integrations entry renders
with a provider suffix — the oracle pins the exact entry text
`Integrations (github)` — and the billing entry reads `Billing &
Plans`. The sidebar header carries a back control to the workspace home
plus the workspace identity (logo or fallback, name, role); the plan
pill beside it renders nothing in this checkout.

On narrow viewports the sidebar is replaced by a top bar with a
hamburger control that opens the same entries as a dropdown panel
(closing on an outside click) beside a crumb naming the current
section. The hamburger hides on wide viewports.

Gates: general, members and exports admit admins and members; billing,
webhooks and integrations are admin-only; guests are refused every
section. The gate key derives from the first two path segments after
the workspace slug, so nested addresses (a webhook detail URL) inherit
their section's gate. Refusal renders an explanatory not-authorized
view (illustrated, with a heading stating the visitor is not authorized
to view the page) instead of any content — never a redirect or a blank
page — with the settings sidebar hidden and the tab title left empty
(§2). The gates are UI-only: out-of-role writes are refused
server-side (member and guest workspace PATCH/DELETE answer 403) while
reads stay allowed (GET answers 200 for both), so the pages deny what
the API would permit reading.

Members admitted to the general page see it dimmed and inert: the name
field disabled, no save control, no logo action, no delete section —
but the workspace URL control still copies (§5). Individual pages
repeat their own gate check, but the layout gate fires first, so a
page-level denial branch (for example the webhook detail page's own
denial paragraph) is unreachable through the UI; slices build one gate,
in the shell, not per-page duplicates.

Reads: the gate reads the signed-in user's workspace role (membership
read, §44). No other API traffic belongs to the shell.

## 2. Tab titles (WSET-007, slice NEWFRONT-534)

Each settings page titles the browser tab with the workspace name
followed by the section name: `{workspace} - General settings`,
`{workspace} - Members`, `{workspace} - Billing & Plans`, `{workspace}

- Exports`, `{workspace} - Webhooks`, `{workspace} - Integrations`.
The webhook detail page uses `{workspace} - Webhook`. Navigating
between sections updates the title; refused sections leave it empty.
Oracle: `shell-general.spec.ts`.

## 3. General details form (WSET-003, slice NEWFRONT-535)

The general page shows the workspace name heading, the URL line (§5),
and a form with three editable fields: workspace name, organization
size, and timezone, plus a read-only workspace URL field echoing the
same host-and-slug text as the URL line. The form prefills from the
stored workspace. Oracle: `shell-general.spec.ts`.

The name validates on save against the shared workspace-name rule:
blank is rejected with `Workspace name is required`; longer than 80
characters is rejected with a length message; injection-risk
characters (angle brackets, quotes, braces, brackets and similar
symbols) are rejected with a message naming special characters; anything
outside letters, numbers, spaces, hyphens and underscores is rejected.
The error renders inline under the field and no write is issued.
Organization size offers exactly six presets: `Just myself`, `2-10`,
`11-50`, `51-200`, `201-500`, `500+`. The timezone picker is a
searchable select over the server timezone list (grouped by zone with
merged city labels, searchable by zone, city and offset, plus a UTC
option); the stored value is the zone identifier (for example selecting
the Auckland entry stores `Pacific/Auckland`).

Saving sends PATCH `/api/workspaces/{slug}/` with the name,
organization size and timezone fields only. While the write is in
flight the save control reads `Updating`; on success a `Success!` /
`Workspace updated successfully` notice shows and the form reflects
the stored values. A failed save shows no notice at all — the form
keeps the attempted values with no error — which the oracle pins as
observed. Only admins see an enabled form and the save control (§1).

Escape-closing the size dropdown wedges reopening until an outside
click lands (NEWFRONT-420, §43); the new frontend implements the
intended behavior where reopening works immediately.

## 4. Workspace logo (WSET-004, slice NEWFRONT-535)

With no logo the avatar block shows the workspace initial — the first
character of the workspace name, uppercased. Admins get an upload/edit
action that opens the image dialog; members see the block dimmed with
no action. Oracle: `shell-general.spec.ts`.

The dialog accepts a single dropped or picked image (jpeg, png, webp;
at most 5 MB). Oversize files are refused inline with a size message
and wrong types with a format message. The footer holds Remove
(disabled with no logo, `Removing` while working), Cancel, and the
primary upload action (disabled until a file is picked, `Uploading`
while working).

Uploading posts the file metadata to the workspace asset endpoint with
the workspace identifier and the workspace-logo entity type, uploads
the bytes to the returned signed URL, confirms the asset, then PATCHes
the workspace with the returned logo URL. Upload and replace close the
dialog with no notice; the block shows the new image immediately.
Upload failure shows an error notice and keeps the dialog.

Removal deletes the stored asset (the legacy asset path for full-URL
values, the current asset endpoint otherwise), then PATCHes the
workspace with an emptied logo field. Success closes the dialog with a
`Success!` / `Workspace picture removed successfully.` notice; a
failed workspace write shows an error notice instead. The old app
leaves the removed image rendered until a reload (NEWFRONT-416, §43);
the new frontend swaps to the initial-letter fallback as soon as the
removal succeeds.

## 5. Copy workspace URL (WSET-005, slice NEWFRONT-535)

A control under the workspace name on the general page (also visible,
dimmed, to members) copies the full workspace address —
`{origin}/{workspaceSlug}` — to the clipboard and confirms with a
`Workspace URL copied to the clipboard.` notice. Clipboard failure
stays silent: no notice, no page disturbance, nothing written
server-side. Oracle: `shell-general.spec.ts`.

## 6. Delete workspace (WSET-006, slice NEWFRONT-535)

Admins see a delete section at the bottom of the general page: a boxed
control naming workspace deletion with an irreversibility explanation
and a Delete button. Members see no delete section. The button opens a
confirmation dialog that names the workspace, warns that all work data
is lost without recovery, and requires two exact inputs: the workspace
name, and the fixed phrase `delete my workspace`. Confirm stays
disabled until both match exactly. Oracle: `shell-general.spec.ts`
(uses a throwaway workspace, never the seeded one).

Cancelling (or dismissing) closes the dialog, resets both inputs, and
leaves the workspace untouched. Confirming shows a working state,
issues DELETE `/api/workspaces/{slug}/`, refreshes the user's
workspace settings, closes the dialog, navigates to the redirection
target — the last/fallback workspace home when it is still in the
user's list, else workspace creation — and shows a `Workspace deleted.`
notice. A failed delete shows an error notice and keeps the dialog
with its input intact.

## 7. Members roster (WSET-008, slice NEWFRONT-537)

The members page (`settings/members/`) admits admins and members;
guests get the not-authorized view. The header names the section with
a count chip, beside a persistent auto-focused search field, a role
filter, the activity entry (§8), the admin-only invite control (§10),
and the billing entry (§14). The browser tab follows §2. Reads on
entry: GET `/api/workspaces/{slug}/members/` and GET
`/api/workspaces/{slug}/invitations/`. Oracle: `members.spec.ts`.

The count chip shows the roster size excluding bot memberships
(suspended rows included). Search matches members by first, last and
display name plus email, case-insensitively, narrowing the
role-filtered set; pending invitations are searched separately by
email only and ignore the role filter. The role filter offers `Admin`,
`Member`, `Guest` and `Suspended` as multi-select toggles with an
active badge; `Suspended` matches exactly the deactivated memberships
(`is_active` false), the role values match active members by numeric
role, and with no filter active every non-bot membership shows.
Filter and search combine.

Default order is the server order with active members stably above
inactive ones. Each sortable column header (full name, display name,
email, role, joining date) additionally offers ascending/descending
ordering, applied within the active-first rule. Note for the slice:
no inventory row or oracle scenario names header sorting explicitly —
the oracle pins only the active-first default — so the new frontend
implements the menus per this paragraph and the slice should add a
row for them if it judges the behavior uncovered (see §43).

## 8. Member rows (WSET-011, slice NEWFRONT-537)

Each roster row shows, in columns: full name with avatar, display
name, email address, account type (role), authentication method, and
joining date. Suspended rows render dimmed with a `Suspended` marker
instead of a role, a suspended avatar instead of the photo or
initial, and no authentication, joining date, profile link or row
menu. Active rows link their avatar to the member profile page
(`/{workspaceSlug}/profile/{userId}/`); the avatar is the photo when
set, else the initial letter of the email or display name. The
authentication column names the last sign-in medium (email, magic
code, GitHub, GitLab, Google and similar providers) and hides when
unknown. Oracle: `members.spec.ts`.

A hover menu on active rows offers `Remove` to admins for other
members and `Leave` to anyone for their own row (§13); members see
only their own Leave entry, and suspended rows offer no menu. The
role column is covered in §12.

No member-activity entry renders in this checkout: the header control
beside the invite button is an empty placeholder with no menu item,
profile link or view behind it, and no member-activity view exists in
the surveyed sources. The inventory acceptance line that promises an
activity control opening a member-activity view does not match what
the oracle pinned; the slice should amend that line (the gate proves
identity plus profile links only). If a cloud-only activity entry
exists, it is unobservable here and needs oracle confirmation on the
cloud build.

## 9. Members loading and no-match states (WSET-014, slice NEWFRONT-537)

The roster area shows a skeleton loader until the members and
invitations reads resolve. The old app never renders its loader (its
guard tests for reads that are already empty arrays, so it can never
pass — NEWFRONT-422, §43); the new frontend implements the intended
skeleton. When search and filters exclude every member and every
invitation, an inline `No matching members` message appears; clearing
the search restores the full roster, and the underlying data is never
touched. Oracle: `members.spec.ts`.

## 10. Bulk invitations (WSET-009, slice NEWFRONT-538)

The invite control is admin-only: members and guests see no invite
control on the members page. It opens a dialog titled for inviting
collaborators, starting with one email-plus-role row defaulted to
Member. Rows append Member-defaulted via an add-more control and drop
via a per-row remove control that appears past one row; emptying the
dialog re-appends a single blank Member row. Oracle: `members.spec.ts`.

Each row takes an email address and a role. Email is required (blank
shows a required message) and must match email shape (else an `Email
is invalid` message); errors render inline per row and block submit
with input intact. Role options are the roles at or below the actor's
own workspace role — an admin sees `Guest`, `Member`, `Admin`. Submit
sends POST `/api/workspaces/{slug}/invitations/` with an `emails`
array of email-plus-role pairs. Success closes the dialog (resetting
it), shows a success notice reporting the invitations sent, and
refreshes the pending group, where the new invitations appear with
their roles. Failure (for example inviting an address that is already
a member, which the server refuses) shows a generic error notice and
keeps the dialog open with input intact. Cancelling closes and resets
with nothing sent.

The dialog ignores Escape and backdrop clicks — only its explicit
Cancel/Send controls dismiss it (NEWFRONT-418, §43); the new frontend
closes it on Escape like its siblings (§37).

## 11. Pending invitations (WSET-010, slice NEWFRONT-538)

Pending invitations render in their own collapsible `Pending invites`
group under the roster, expanded by default, with a count chip showing
the number of search-matching invitations. The group renders only for
admins and only while at least one invitation matches the search; it
collapses to just its header on toggle and reopens with its rows.
Each row shows an email-initial avatar, the email address, a pending
marker, a role control, and a menu. Oracle: `members.spec.ts`.

The role control offers `Guest`, `Member`, `Admin` and persists
through PATCH `/api/workspaces/{slug}/invitations/{invitationId}/`
with the role; options above the actor's own role are not offered.
Failure shows an error notice and the row keeps its previous role.
The menu holds `Copy link` (only when the invitation carries a link;
copies the absolute invite URL built from the stored link and confirms
with a copied notice; copy failure stays silent) and admin-only
`Remove`, which opens the shared remove confirmation (§13) titled
`Remove {email}?`. Cancelling keeps the invitation; confirming
withdraws it through DELETE
`/api/workspaces/{slug}/invitations/{invitationId}/` with a success
notice reporting the removal. A failed withdrawal still closes the
dialog but shows an error notice and leaves the row visible.

Note for the slice: the inventory Who column promises member viewing,
but the oracle pins the group as admin-only — members see no group at
all. Amend the Who column to admin-only; the gate proves the observed
behavior.

## 12. Member role changes (WSET-012, slice NEWFRONT-538)

Admins change another member's role from the roster's account-type
column, which offers `Guest`, `Member` and `Admin` for every active
row except the actor's own — self role-change is never offered, and
members are offered no role control on any row. Suspended rows show
their marker instead of any control. Oracle: `members.spec.ts`.

Changing a role persists through PATCH
`/api/workspaces/{slug}/members/{membershipId}/` with the role; the
address carries the membership identifier, resolved from the row's
user. Success keeps the new role; failure shows an error notice and
the row reverts to its previous role. The server independently refuses
self role-change (400 naming the actor's own role) and member-actor
role writes (403).

## 13. Member removal and self-leave (WSET-013, slice NEWFRONT-538)

Removal, leaving and invitation withdrawal share one confirmation
dialog. For another member it is titled `Remove {display name}?` with
text naming the person and warning they lose workspace access
irreversibly; for the actor themselves it reads `Leave workspace?`
with the matching leave text. The dialog always closes after confirm —
even when the write fails — with Cancel/Escape closing it unchanged.
Oracle: `members.spec.ts`.

Confirming removal issues DELETE
`/api/workspaces/{slug}/members/{membershipId}/`. Removal deactivates
server-side (`is_active` false); the UI flips the row to its
Suspended presentation (§8) with no success notice, and the row sorts
last and persists across reloads. A failed removal shows an error
notice carrying the server's reason and the row stays active and
visible.

Self-removal instead issues POST
`/api/workspaces/{slug}/members/leave/` with no body, refreshes the
user's workspace settings, and navigates to the redirection target.
Observed on the running app, the target resolves to the just-left
workspace home (from the stale workspace list), which renders a
not-authorized gate explaining the visitor is not a member and
offering pending invites or workspace creation — not the workspace
picker the inventory acceptance names. The spec records the observed
landing; the slice should either file the picker discrepancy as an old
bug with intended behavior, or amend the acceptance line to the gate,
before the gate locks one of them in. A failed leave shows an error
notice.

## 14. Billing upsell entry (WSET-015, slice NEWFRONT-538)

The members header reserves a billing action control beside the invite
button for admins. In this checkout that control renders nothing for
any role — the oracle pins the absence for admins and members alike —
so on OSS and self-managed builds there is no seat/plan upsell
affordance on the members page. Any cloud seat/plan entry is
unobservable here (cloud overlay absent) and needs oracle confirmation
on the cloud build; the gate revisits it there. The slice builds no
upsell UI beyond the reserved placement. Oracle: `members.spec.ts`.

## 15. Integrations page (WSET-016, slice NEWFRONT-539)

The integrations page (`settings/integrations/`) is admin-only:
members and guests get the not-authorized view with no provider
content. Admins see a banner heading naming Integrations above two
provider cards, GitHub (§16) and GitLab (§17), rendered directly
rather than from the legacy app-integrations list endpoint, which no
longer exists upstream (§42). The browser tab follows §2. Oracle:
`developer.spec.ts`.

## 16. GitHub provider (WSET-017, slice NEWFRONT-539)

The GitHub card reads its state from GET
`/api/workspaces/{slug}/integrations/github/`, whose payload carries
a connected flag with the verified login and timestamp. While the
read is unresolved the card shows a loading state. Oracle:
`developer.spec.ts`.

Disconnected, the card shows the provider mark and name, an explainer
naming the fine-grained personal token with issue read access and the
mirroring behavior, an external help link to token creation, a masked
token field, and a Connect control that stays disabled until the field
holds a non-blank token. Connecting sends POST
`.../integrations/github/connect/` with the trimmed token. Success
clears the field, re-reads the status, and shows a `Connected` notice
reporting the verified credential. A rejected token must show a
`Connection failed` notice inline, keep the typed token, and stay on
the page — the old app instead answers 401, which trips the global
session interceptor into a full reload that swallows the notice and
clears the card (NEWFRONT-421, §43); the new frontend implements the
intended inline failure.

Connected, the card marks itself verified and reports `Connected as
{login}` with a pointer to bind repositories from project settings,
plus a Disconnect control (danger, with a working state). Disconnect
sends POST `.../integrations/github/disconnect/` with an empty body;
success re-reads the status and shows a `Disconnected` notice
reporting the removal and the disabled project syncs. Disconnect
failure shows an error notice. Server-side, disconnect keeps project
repository bindings in place but disables their syncing and revokes
the linked provider account; other workspaces' bindings are untouched.

## 17. GitLab accounts (WSET-018, slice NEWFRONT-539)

The GitLab card lists provider accounts from GET
`/api/workspaces/{slug}/integrations/git/accounts/` and tracks the
active GitLab account (first account of that provider whose status is
not revoked). While the read is unresolved the card shows a loading
state. Oracle: `developer.spec.ts`.

Disconnected, the card shows the provider mark and name, an explainer
naming the GitLab token with API access and the bind-and-mirror
behavior, an optional host URL field, a masked token field, and a
Connect control disabled until the token field is non-blank.
Connecting sends POST `.../integrations/git/accounts/` with the
provider, the trimmed token, the personal-token auth type, and the
host URL only when one was typed. Success clears the token, re-reads
the accounts, and shows a `Connected` notice reporting the verified
credential. A rejected token behaves per §16: intended inline
`Connection failed` with the typed token kept (NEWFRONT-421); the old
app reloads instead.

Connected, the card marks itself verified and reports `Connected as
{login} on {host}` (falling back through display name to the provider
name when the login is missing), plus a Disconnect control with a
working state. Disconnect sends DELETE
`.../integrations/git/accounts/{accountId}/`; success re-reads and
shows a `Disconnected` notice reporting the disabled project syncs,
and the account reads back revoked with its project bindings kept but
no longer syncing. Disconnect failure shows an error notice.

## 18. Webhooks list (WSET-019, slice NEWFRONT-541)

The webhooks page (`settings/webhooks/`) is admin-only: members and
guests get the not-authorized view with no list, loader or creation
entry. Admins see a heading explaining the automation purpose with an
`Add webhook` control. The list reads GET
`/api/workspaces/{slug}/webhooks/`; a loader covers the list until it
resolves. With no webhooks an illustrated empty state offers the same
creation action, opening the same dialog. Each row shows its target
URL and links to its detail page (§21), and carries an active switch
(§24). The browser tab follows §2. Oracle: `developer.spec.ts`.

## 19. Webhook creation (WSET-020, slice NEWFRONT-541)

Both creation entries open a `Create webhook` dialog with a target
URL field and an event-mode choice defaulting to all events; the
individual event toggles render only in individual mode. Oracle:
`developer.spec.ts`.

An empty URL blocks submit with an inline `URL is required` error and
no write. Individual mode offers five toggles — Projects, Cycles,
Work items, Modules, Work item comments — all on by default, each
with a one-line description of what it triggers; toggling flips the
matching event flag. Cancel or Escape closes the dialog without
saving and without any server write.

Submit sends POST `/api/workspaces/{slug}/webhooks/` with the URL and
the five event flags (`project`, `cycle`, `module`, `issue`,
`issue_comment`) — all true in all-events mode, the toggle values in
individual mode. Success shows a `Webhook created` notice, downloads
the secret backup file, and moves the dialog to the one-time reveal
(§20). A failed create (for example a duplicate target URL, which the
server rejects as non-unique per workspace) shows a `Webhook not
created` notice and keeps the dialog with the typed URL intact.

## 20. Webhook secret reveal (WSET-021, slice NEWFRONT-541)

After creation the dialog swaps to a `Key created` reveal step
explaining that the key cannot be seen after Close and that a backup
file has been downloaded. The backup downloads automatically at
creation as `webhook-secret-key-{timestamp}.csv` carrying the webhook
identity, target URL, timestamps, active state, event flags, owning
workspace and the secret itself. Oracle: `developer.spec.ts`.

The secret renders masked (a row of dots) with show/hide and copy
controls. Showing reveals the in-clear key (a long generated string);
hiding masks it again. Copying writes the key to the clipboard and
confirms with a copied notice; copy failure shows an error notice.
Escape does nothing on the reveal step — only the explicit Close
dismisses it — and closing clears the key from memory: the detail
page afterwards shows the secret block with no key and no actions, so
the value can never be viewed again.

## 21. Webhook detail (WSET-022, slice NEWFRONT-541)

The detail page (`settings/webhooks/{webhookId}/`) is admin-only; the
layout gate intercepts members and guests with the shared
not-authorized view before the page's own denial can render. Admins
see a spinner while GET
`/api/workspaces/{slug}/webhooks/{webhookId}/` resolves, then a form
prefilled with the stored target URL, the active switch, and the
event selection: the mode reads all events exactly when all five
stored flags are true, else individual with the toggles reflecting
the stored flags. The tab title is `{workspace} - Webhook`. Oracle:
`developer.spec.ts`.

Saving sends PATCH `.../webhooks/{webhookId}/` with the URL, active
state and all five flags. The update control shows an `Updating`
state while in flight; success shows a `Webhook updated` notice and
failure shows an error notice with the form and server state kept. An
unknown webhook id 404s server-side; the old app then spins forever
(NEWFRONT-417, §43) — the new frontend instead explains that the
webhook is missing or deleted.

## 22. Secret regeneration (WSET-023, slice NEWFRONT-541)

The detail page's secret block always renders with its regenerate
control; key actions appear only while a freshly issued key sits in
memory. Regenerating sends POST
`.../webhooks/{webhookId}/regenerate/` with no body. The control shows
a `Re-generating` working state while in flight; success shows a
regenerated notice, reveals the new key once (masked by default, with
show/hide and copy per §20), and downloads a fresh backup file
carrying the new key. The previous key stops working. Regenerating a
deleted webhook shows an error notice. Oracle: `developer.spec.ts`.

The old app's regenerate backup is stale: on a fresh load no file
downloads at all, and on a repeat the file carries the previous key
(NEWFRONT-419, §43). The new frontend downloads a fresh file with the
new key on every successful regenerate.

## 23. Webhook deletion (WSET-024, slice NEWFRONT-541)

The detail page ends in a collapsed danger-zone section explaining
that deletion is permanent and stops future deliveries. Expanding it
reveals the delete control, which opens a confirmation dialog warning
the action cannot be undone. Cancelling (or Escape) closes the dialog
and keeps the webhook. Oracle: `developer.spec.ts`.

Confirming shows a working state, issues DELETE
`.../webhooks/{webhookId}/`, navigates back to the webhooks list, and
shows a deleted notice. Deleting an already-deleted webhook shows a
`could not be deleted` error notice and keeps the detail form with
its values.

## 24. Webhook list-row switch (WSET-041, slice NEWFRONT-541)

Each webhooks list row carries an active switch inside the row link.
Flipping it sends PATCH `.../webhooks/{webhookId}/` with only the
flipped active state and the row reflects the persisted state.
Members never reach the list (§18). Oracle: `developer.spec.ts`.

## 25. Token list (WSET-025, slice NEWFRONT-544)

The personal access token page lives at `/settings/profile/api-tokens`
under the profile settings Developer group (seam with the profile
epic, §41) and lists the signed-in user's own tokens. A loader shows
until GET `/api/users/api-tokens/` resolves. With no tokens an
illustrated empty state offers its own creation action; with tokens
each renders as a row (§28). An `Add personal access token` control
sits beside the `Personal Access Tokens` heading in both states, and
the profile sidebar entry opens the same page. Tokens are per-user:
a fresh user starts with an empty list. Oracle: `api-tokens.spec.ts`.

## 26. Token creation (WSET-026, slice NEWFRONT-544)

The create dialog takes a title, a free-text description, and an
expiry. The title is required (empty and blank both report `Title is
required` inline) and capped at 255 characters (longer reports `Title
should be less than 255 characters`); cancelling leaves no token
behind. Oracle: `api-tokens.spec.ts`.

Expiry offers four presets — `1 week`, `1 month`, `3 months`, `1
year` — plus `Custom`, with nothing selected initially. A
never-expires toggle (off by default) disables the picker and hides
the preview; flipping it back restores the previous selection. The
custom date picker floors at tomorrow (today disabled). A live line
previews the resulting expiry as `Expires {Mon D, YYYY} at {H:MM}`
for presets and chosen custom days. Submitting with a title but no
date while expiry is required reports the missing expiration date as
an error notice and keeps the dialog open.

Submit sends POST `/api/users/api-tokens/` with the label,
description and expiry: null when never-expires is on, else the
preset offset (a week, a month, three months, a year from now) or the
custom day with the current time of day, as an ISO timestamp. The
generate control shows a working state while in flight. Success moves
to the one-time reveal (§27) with a backup download; failure shows an
error notice and keeps the dialog.

## 27. Token reveal (WSET-027, slice NEWFRONT-544)

After creation the dialog swaps to a `Key created` reveal showing the
raw token in clear with a copy control and the expiry line (`Expires
...` or `Never expires`). A backup file downloads automatically as
`secret-key-{timestamp}.csv` with the header
`Title,Description,Expiry,Secret key` carrying the label,
description, formatted expiry (or `Never expires`) and the raw
secret. Copying writes the raw token to the clipboard and confirms
with a copied notice. Oracle: `api-tokens.spec.ts`.

Closing the reveal makes the value unrecoverable: only the metadata
row stays listed, the list read never returns the secret, and the
page text no longer contains it. Like its webhook sibling, the reveal
requires explicit dismissal — and the create dialog ignores Escape
throughout (NEWFRONT-418, §43); the new frontend closes the
pre-reveal create dialog on Escape while keeping the explicit-Close
rule for the reveal itself (§37).

## 28. Token rows and deletion (WSET-028, slice NEWFRONT-544)

Each token row shows the label with an `Active`/`Expired` marker
driven by the server liveness flag, the description when non-blank,
and a human expiry line: `Expires {Mon D, YYYY} at {H:MM}` for a
future date, `Never expires` when unset, or relative `Expired {...}
ago` wording once lapsed. Row liveness always agrees with the server.
Oracle: `api-tokens.spec.ts`.

Deletion runs behind a confirmation dialog naming personal-access-
token deletion and warning that dependent applications lose access
irreversibly. Cancelling keeps the row; confirming sends DELETE
`/api/users/api-tokens/{tokenId}` (no trailing slash), shows a
successfully-deleted notice, and drops the row (restoring the empty
state when the last token goes). A failed delete shows an error
notice and keeps the row.

## 29. Billing page (WSET-029, slice NEWFRONT-546)

The billing page (`settings/billing/`) is admin-only: members and
guests get the not-authorized view. Admins see a `Billing & Plans`
heading with a plan-management description, a `Community` summary box
(whose description promises unlimited projects, issues, cycles,
modules, pages and storage), and an `All plans` comparison. On OSS
and self-managed builds the page makes no API calls at all — every
card, price and link below is static product data. The browser tab
follows §2. Oracle: `billing.spec.ts`.

The comparison renders three plan cards — `Pro`, `Business`,
`Enterprise` — in that order; the Free and One plans never render.
Pro carries the `Popular` marker. Below the cards an always-visible
Highlights row lists each plan's headline features, and a toggle
(`Compare all features` ⇄ `Collapse comparison`, collapsed by
default) expands the full feature comparison grouped in titled
sections (including `Project + work tracking` among several).

Each card shows its name, price line, a monthly/yearly frequency
switch defaulting to monthly, and an action button (§30). Prices are
per user per month with a billing-frequency qualifier: Pro shows $8
monthly and $6 yearly with a `-25%` discount badge; Business and
Enterprise show `Quote on request` with no price and no badge, but
still carry the frequency switch. The discount badge appears only
where the yearly price undercuts the monthly one. Frequency switches
are per-card and independent.

Anything beyond this static page — subscription reads, current-plan
management, in-app purchase — is unobservable in this checkout (no
such calls or controls exist in the surveyed sources; the cloud
overlay is absent) and needs oracle confirmation on the cloud build;
the gate revisits it there.

## 30. Plan actions (WSET-030, slice NEWFRONT-546)

Built typing (NEWFRONT-546): both link targets above are literal hrefs on
an anchor that opens a new tab, so no request is ever issued by the app and
the page state is untouched. The two pricing-page targets carry the chosen
frequency in `plan`; the enterprise contact page carries none. Per-card
frequency state lives on the billing page, so two cards never share a
choice.

Every plan action opens an external page in a new tab; nothing is
purchased in-app and no workspace state changes. The button reads
`Upgrade to {plan}` on purchasable plans and `Talk to Sales`
otherwise. Targets follow the selected frequency: Pro opens the
per-frequency Pro pricing page
(`https://airepublic.com/pricing/pro?plan={month|year}`); Business —
despite its `Talk to Sales` label — opens the per-frequency Business
pricing page (`.../pricing/business?plan={month|year}`), an observed
quirk the oracle pins; Enterprise opens the sales contact page
(`https://airepublic.com/contact`) for both frequencies. After any
action the billing page sits unchanged: no dialog, no notice, no
plan change. Oracle: `billing.spec.ts`.

Managing an existing subscription through a billing portal, and
cancelled-or-failed checkout behavior, are cloud-only paths
unobservable in this checkout; they need oracle confirmation on the
cloud build and the gate revisits them there.

## 31. No paid-plan upgrade modal (WSET-031, slice NEWFRONT-546)

Built typing (NEWFRONT-546): the new billing body renders no dialog at
all, the shell user menu has no community entry (§1), and nothing outside
the billing page links to any plan surface. The oracle's pre-removal
`bug:` scenario keeps its observed leg for the old app and asserts the
intended absence on `apps/web_new` (target branch in the spec). The
active-cycles pill leg is skipped on `apps/web_new` until the cycles route
lands (NEWFRONT-527 carries the leg).

Target behavior, from the inventory acceptance text and main PR #572
(`Delete the dead Community upgrade/pricing modal
(PaidPlanUpgradeModal) and its plan constants`, open at spec time):
no paid-plan upgrade dialog exists anywhere in the new frontend.
Gated features never open a free/pro/business/enterprise pricing
pop-up, and no trial, checkout redirect or plan-card UI is reachable
outside `settings/billing`. Concretely the #572 target deletes the
modal tree, the plan-feature and community-product constants, the
edition badge component, and the user-menu `Community` entry that
opened the dialog; the billing page itself is unchanged (it keeps
its discount display). The new shell already asserts no `Community`
menu entry. Oracle: `billing.spec.ts`.

On the current checkout the removal has not landed yet: the
user-menu `Community` entry still opens the dialog (free-plan pitch
plus Pro/Business/Enterprise cards with external checkout links, no
trial) — NEWFRONT-423, §43 — while the former pill and badge trigger
points are already inert (the `Pro` pills are static labels with no
click handler; the edition badge has no mount point). The oracle pins
the pre-removal behavior with a `bug:` scenario; the gate proves the
intended absence. If #572 merges first, that oracle scenario needs
reworking per NEWFRONT-156.

Note for the slice: NEWFRONT-546's own description still phrases this
row as building the modal — that text predates the removal target.
This spec and the inventory negative row win: the slice builds
nothing here and proves the absence.

## 32. OSS billing (WSET-032, slice NEWFRONT-546)

Built typing (NEWFRONT-546): the page reads no endpoint on any build, so
the OSS and cloud bodies are identical today. Every plan action is an
anchor to the external pages in §30; no trial, portal, checkout,
current-plan or cancellation wording exists in the body.

On OSS builds the billing page renders the community summary and the
plan comparison of §29 with no purchase, portal, trial or
subscription-management controls: the page text names no trial,
portal, checkout, current plan or subscription cancellation, every
plan action leaves the app for an external page (§30), and no upgrade
dialog exists (§31). Oracle: `billing.spec.ts`.

## 33. Exports entry (WSET-033, slice NEWFRONT-548)

The exports page (`settings/exports/`) admits admins and members;
guests get the not-authorized view. Admitted roles see an `Exports`
heading with an explanatory paragraph about exporting project data in
several formats and reaching past exports through download links,
plus a breadcrumb naming the section and the §2 tab title. Below the
heading mounts the export guide — the format/project export form and
the previous-exports history with download links and paging — which
the exports area (NEWFRONT-59) owns end to end. Oracle:
`cross-cutting.spec.ts` (entry) and the EXP suite (guide behaviors).

Seam (see §41): the exports-entry slice builds the page frame above
plus a typed registration seam (a named slot where the exports area
registers its guide component); it does not build, copy or stub the
form, history, downloads or polling. The exports area provides the
guide content against that seam. The slice proves the frame, the
denial and the seam contract with unit tests (including a stub
registrant mounting through the seam); the workspace gate proves the
full entry scenario once the exports provider exists.

## 34. Instance maintenance (WSET-034, slice NEWFRONT-548)

The app shell reads GET `/api/instances/` once at startup. While that
read fails, the app renders a full-screen maintenance view instead of
any workspace content: a themed illustration (light and dark variants
track the color scheme) with a startup-failure message explaining
that services may not have started, pointing at container logs, and
offering a `Contact Support` link to
`mailto:support@airepublic.com`. No sidebar or navigation renders
underneath. Once the read succeeds the app renders normally; a
request-level error status that still returns data does not trigger
the view, and the initial load shows a full-screen spinner until the
read first settles. Oracle: `cross-cutting.spec.ts`.

## 35. Instance not-ready (WSET-035, slice NEWFRONT-548)

When the instance-info read reports setup unfinished
(`is_setup_done` false), the app renders a full-screen branded
not-ready view instead of any workspace content: product branding, a
`Welcome to Pi Dash` heading with setup explainer text, and a `Get
started` control leading to the administrator sign-in at `/god-mode`.
No sidebar or workspace content renders underneath. Once setup reads
finished the app renders normally. Oracle: `cross-cutting.spec.ts`.

## 36. Deep links (WSET-036, slice NEWFRONT-534)

Every settings address is directly openable and never redirects:
authorized roles land on the section content (each section's marker
content renders — the general save control, the members invite
control, the billing upgrade control, the exports refresh control,
the webhooks creation control, the integrations connect control —
with members seeing the dimmed/role-appropriate variants), while
unauthorized roles see the not-authorized view with the sidebar
hidden, on the same address. Oracle: `cross-cutting.spec.ts`.

An unknown webhook id 404s server-side; the old app leaves the admin
detail page spinning forever with the address unchanged
(NEWFRONT-417, §43) — the new frontend explains the webhook is
missing or deleted instead. Forbidden roles opening an unknown
webhook id meet the denial view like any other forbidden address.

## 37. Keyboard (WSET-037, slice NEWFRONT-534)

Escape closes the webhook-create dialog (pre-reveal only), the
remove/leave/withdraw confirmation, the delete-webhook confirmation,
the delete-workspace dialog and the logo dialog — each closing
without any server write. The secret-reveal steps (webhook and
token) require explicit Close and ignore Escape by design. The old
app's invite and token-create dialogs also ignore Escape
(NEWFRONT-418, §43); the new frontend closes those two on Escape as
well. Dialogs trap focus in a logical tab order that cycles without
leaving (the invite dialog cycles its email rows, role controls and
Cancel/Send actions; the webhook-create dialog cycles its URL field,
mode choice and Cancel/Create actions). The members search is a
persistent inline input: Escape neither clears nor collapses it. No
other settings shortcut exists — content keys pressed outside a text
field open nothing, navigate nowhere and download nothing. Oracle:
`cross-cutting.spec.ts`.

## 38. No live updates (WSET-038, slice NEWFRONT-534)

Settings lists read on mount only. Members, invitations, webhooks,
tokens and billing never refresh from other actors' changes without
navigation or manual reload: server-side mutations stay invisible,
with zero list re-reads and zero socket traffic while a page sits
open. Only the acting user's own mutations update what they see (an
edited role, a flipped switch, a withdrawn invitation). The carved-out
exception is the exports history, which re-polls while an export runs
(owned by EXP-009 under NEWFRONT-59, not this area). Oracle:
`cross-cutting.spec.ts`.

## 39. No drag, import or in-place export (WSET-039, slice NEWFRONT-534)

Member and webhook order comes only from the sort and filter
controls (§7, §18): no settings landing page carries drag targets,
rows cannot be reordered by dragging (a dragged member row changes
neither the UI nor the server order), no import action exists
anywhere in settings, and no file input exists on any settings
landing page — the logo dialog holds the area's only file picker
(§4). Browsing settings downloads nothing; file downloads are limited
to the webhook/token secret backup files (§20, §27) and the exports
history (§33). Oracle: `cross-cutting.spec.ts`.

## 40. Desktop parity (WSET-040, slice NEWFRONT-534)

Settings behave exactly as on web inside the desktop shell: no
desktop-only settings capability, layout difference, offline behavior
or edition branch was found in the surveyed sources, no settings
route overrides exist in the desktop overlay, and the desktop shell
serves the same pages. Slices build nothing desktop-specific for
this area. The oracle pins the absence of desktop-only hooks on every
settings section and the token page; the native desktop build itself
is not executed by the parity stack, a gap the gate inherits.
Oracle: `cross-cutting.spec.ts`.

## 41. Cross-epic seams

Token screens and the profile epic (NEWFRONT-65/NEWFRONT-90).
WSET-025–028 and PROF-020 pin the same screens — the token tab at
`/settings/profile/api-tokens` — and both oracles drive them with
the same driver methods, so the content is built exactly once:

- The workspace-settings tokens slice (NEWFRONT-544) owns the token
  tab content end to end (§§25–28: page, loader, empty state, rows,
  create dialog, reveal, delete dialog, data hooks) delivered as the
  leaf route component at the profile tokens path. The leaf renders
  its content self-sufficiently: it must not require the profile
  parent layout to render, so everything except shell, entry and
  titles is provable from this area alone. There is no
  workspace-settings sidebar entry for tokens to build — the old app
  has none.
- The profile epic owns the profile settings shell (parent layout,
  grouped sidebar with the Developer-group `Personal Access Tokens`
  entry, tab switching, titles, unknown-tab handling) and the legacy
  workspace-scope forward `/{workspaceSlug}/settings/api-tokens` to
  the tab (pinned by PROF-026, including its query-dropping quirk
  NEWFRONT-431, which the profile side owns).
- The profile tokens slice (NEWFRONT-583) adopts the leaf into the
  shell's tab seam — reusing the workspace-built content, never
  rebuilding it — and proves PROF-020 at the profile gate.

Gate proof for WSET-025–028 runs the full token scenarios, which
drive the profile sidebar entry and the tab: the workspace gate
therefore waits on the profile shell slice (NEWFRONT-565, entry plus
shell) and the profile tokens slice (NEWFRONT-583, mount) in addition
to this area's own tokens slice. The gate run wires those waits; no
workspace slice waits on profile (slices never run parity). The
profile spec (NEWFRONT-558, unwritten at spec time) must adopt the
complementary side of this seam.

Exports entry and the exports epic (NEWFRONT-59/NEWFRONT-87). The
export form and history are EXP rows owned end to end by the exports
area (form, history, downloads, polling, expiry, keyboard, entry
frame EXP-001/002); WSET-033 owns only the settings entry point:

- The exports-entry slice (NEWFRONT-548) builds the §33 page frame
  (heading, explainer, breadcrumb, title, admin/member gate with
  guest denial) plus a typed registration seam — a named slot in the
  new architecture where the exports area registers its guide
  component. It builds no export internals, no export contracts, and
  no stub guide.
- The exports area provides the guide (form plus history) registered
  into that seam, proving EXP-001–013 at its own gate on this same
  page.

Gate proof for WSET-033 runs the entry scenario including the mounted
guide (form defaults, history count, guest denial): the workspace
gate therefore waits on the exports settings-page provider. The
exports epic is still unsplit at spec time, so the gate run wires
that wait once the providing slice exists. The future exports spec
must adopt the complementary side of this seam.

Billing editions. On OSS and self-managed builds the billing area is
fully proven by §§29–32 (static page, external actions, modal
absence). The cloud-only paths — subscription reads and management,
in-app checkout or portal flows, the members-page seat/plan entry
(§14) and any cloud member-activity entry (§8) — are unobservable in
this checkout and need oracle confirmation on the cloud build. The
workspace gate proves the OSS rows on the OSS build and revisits the
cloud paths on a cloud build; slices build no cloud-only billing UI
on speculation.

## 42. Dead code the slices must not build

The inventory coverage audit found the following with no caller or
mount point in the surveyed tree. Contracts issues exclude their
endpoints; slices build no UI for them; the similarity-sensitive
review rejects any port of them:

- Git provider listing: GET
  `/api/workspaces/{slug}/integrations/git/providers/` — the GitLab
  card lists accounts, never providers.
- Repository listings: GET
  `/api/workspaces/{slug}/integrations/git/accounts/{accountId}/repos/`
  and GET `/api/workspaces/{slug}/integrations/github/repos/` —
  project repository binding goes through the project-service bind
  calls rowed in project-settings, never these.
- Legacy integrations: GET `/api/integrations/` (the upstream-removed
  app list the page used to render from; the page now renders stub
  provider cards directly), GET
  `/api/workspaces/{slug}/workspace-integrations/` and DELETE
  `.../workspace-integrations/{id}/provider/` (only caller is the
  unmounted single-integration card).
- Importers: GET `/api/workspaces/{slug}/importers/` plus the
  orphaned importer calls (`getJiraProjectInfo`, `createJiraImporter`,
  `deleteImporterService`, `getImporterServicesList`) — no caller and
  no import UI anywhere (§39).
- The duplicate member client: the packages member service self-read
  has no importer — but its endpoint GET
  `/api/workspaces/{slug}/workspace-members/me/` is live through the
  workspace service self-membership read that feeds every settings
  gate (§1), so contracts cover the endpoint once via the live path,
  not the dead client.
- The self-record request config builder on the user service —
  returns a config nothing consumes.
- UI with no mount: the Slack channel picker (and its app-install
  channel calls), the single-integration card, the edition badge.
- The paid-plan upgrade modal tree and its plan constants (§31).

Not dead, owned elsewhere: the user-scoped GitHub App connection
calls (`/api/users/me/integrations/github/app/...`) belong to the
profile area (PROF-025), not this one; the export modal and
single-export card duplicates belong to NEWFRONT-59. The empty
activity, billing-action and subscription placeholders (§§8, 14, 1)
are reserved seams, not code to port.

## 43. Observed-vs-target record and slice notes

Old bugs the oracle pinned with `bug:` scenarios (slices implement
the intended behavior; the gate proves it):

- NEWFRONT-416 (WSET-004): removed logo stays rendered until reload.
  Intended: the initial-letter fallback shows as soon as removal
  succeeds.
- NEWFRONT-417 (WSET-022/036): unknown webhook id spins forever.
  Intended: a missing-or-deleted explanation.
- NEWFRONT-418 (WSET-009/026/037): invite and token-create dialogs
  ignore Escape (and backdrop). Intended: Escape closes every
  settings create/edit/confirm dialog except the secret-reveal steps.
- NEWFRONT-419 (WSET-023): regenerate skips the backup on a fresh
  load and writes the previous key on a repeat. Intended: every
  successful regenerate downloads a fresh file with the new key.
- NEWFRONT-420 (WSET-003): Escape-closing the size dropdown wedges
  reopening until an outside click. Intended: reopening works
  immediately.
- NEWFRONT-421 (WSET-017/018): rejected provider token reloads the
  page through the session interceptor instead of a failure notice.
  Intended: inline failure notice with the typed token kept; the new
  client must not treat provider 401s as session expiry.
- NEWFRONT-422 (WSET-014): the members skeleton guard can never pass,
  so no loader renders. Intended: skeleton until the reads resolve.
- NEWFRONT-423 (WSET-031): the user-menu `Community` entry still
  opens the paid-plan upgrade dialog. Intended: removed per main
  #572 (§31).

Inventory-vs-observed notes for the slices (amend the lines; the gate
proves the observed behavior unless a bug from the list above says
otherwise):

- WSET-008: column-header sorting (§7) is implemented behavior no row
  names; add a row if judged uncovered.
- WSET-010: the Who column promises member viewing; the group is
  admin-only (§11). Amend to admin-only.
- WSET-011: the acceptance promises an activity control; none
  renders (§8). Amend the line.
- WSET-013: the acceptance promises a workspace-picker redirect on
  self-leave; the app lands on a not-authorized gate (§13). File a
  bug with intended behavior or amend the line before the gate.
- WSET-015: the acceptance promises a cloud upsell entry; nothing
  renders in this checkout (§14). Cloud confirmation needed.
- WSET-029/030: cloud subscription management, portal and
  checkout-failure paths need cloud confirmation (§§29–30).
- WSET-031: NEWFRONT-546's description still phrases the row as
  building the modal; the negative target in §31 wins.
- WSET-041: no slice owns the row yet; NEWFRONT-541 should absorb it
  (§24).

## 44. Endpoint table for the contracts issues

Every endpoint this area uses, with the fields the UI consumes.
Already-covered reads (sign-in user, workspace list and similar) are
not re-rowed; the contracts issues confirm coverage against this
table and add what is missing. Dead endpoints are excluded — see §42.

Workspace and self (NEWFRONT-532):

- GET `/api/workspaces/{slug}/` — details read: name, slug, logo
  URL, organization size, timezone, role, member count.
- PATCH `/api/workspaces/{slug}/` — details write: name,
  organization size, timezone; logo write: logo URL field.
- DELETE `/api/workspaces/{slug}/` — workspace deletion.
- GET `/api/workspaces/{slug}/workspace-members/me/` — self
  membership: role and workspace standing; feeds every settings
  gate.
- GET `/api/timezones/` — timezone list: zone value, label and
  offsets.
- Workspace asset flow for the logo (§4): POST
  `/api/assets/v2/workspaces/{slug}/` (workspace identifier,
  logo entity type, file metadata) → signed-URL byte upload → PATCH
  `.../{assetId}/` confirmation; DELETE
  `/api/assets/v2/workspaces/{slug}/{assetId}/` for current assets
  and DELETE `/api/workspaces/file-assets/{workspaceId}/{assetKey}/`
  for legacy full-URL assets.

Members and invitations (NEWFRONT-532):

- GET `/api/workspaces/{slug}/members/` — roster rows: membership
  id, nested user (id, names, display name, email, avatar, bot flag,
  last sign-in medium), role, active flag, creation date.
- PATCH `/api/workspaces/{slug}/members/{membershipId}/` — role
  write: role.
- DELETE `/api/workspaces/{slug}/members/{membershipId}/` —
  removal, deactivates server-side.
- POST `/api/workspaces/{slug}/members/leave/` — self-leave, no
  body.
- GET `/api/workspaces/{slug}/invitations/` — pending list: id,
  email, role, invite link.
- POST `/api/workspaces/{slug}/invitations/` — bulk invite: an
  `emails` array of email-plus-role pairs.
- PATCH `/api/workspaces/{slug}/invitations/{invitationId}/` —
  invite role write: role.
- DELETE `/api/workspaces/{slug}/invitations/{invitationId}/` —
  withdrawal.

Integrations, webhooks, tokens, billing (NEWFRONT-533):

- GET `/api/workspaces/{slug}/integrations/github/` — provider
  status: connected flag, verified login, verification timestamp.
- POST `.../integrations/github/connect/` — connect: trimmed token.
- POST `.../integrations/github/disconnect/` — disconnect, empty
  body.
- GET `/api/workspaces/{slug}/integrations/git/accounts/` —
  account list: id, provider, host URL, login and display name,
  status.
- POST `.../integrations/git/accounts/` — register: provider,
  token, optional host URL, personal-token auth type.
- DELETE `.../integrations/git/accounts/{accountId}/` —
  disconnect.
- GET `/api/workspaces/{slug}/webhooks/` — webhook list: id, URL,
  active flag, five event flags.
- POST `/api/workspaces/{slug}/webhooks/` — create: URL plus five
  event flags; the response carries the one-time secret key.
- GET `/api/workspaces/{slug}/webhooks/{webhookId}/` — detail:
  same shape as the list rows.
- PATCH `/api/workspaces/{slug}/webhooks/{webhookId}/` — edit:
  URL, active flag, five event flags; the list-row switch sends the
  active flag alone.
- DELETE `/api/workspaces/{slug}/webhooks/{webhookId}/` — delete.
- POST `/api/workspaces/{slug}/webhooks/{webhookId}/regenerate/`
  — new secret, no body; the response carries the one-time key.
- GET `/api/users/api-tokens/` — own token list (user-scoped, not
  workspace-scoped): id, label, description, expiry, liveness; never
  the secret.
- POST `/api/users/api-tokens/` — create: label, description,
  expiry timestamp or null; the response carries the one-time raw
  token.
- DELETE `/api/users/api-tokens/{tokenId}` — delete (no trailing
  slash).
- Billing on OSS/self-managed makes no API calls: plan cards,
  prices and external action URLs (§§29–30) are static product
  data. Any cloud subscription reads need cloud confirmation.

Instance (no new contracts — UI-only reads owned with the shell):

- GET `/api/instances/` — instance info: setup-done flag and
  instance standing; drives §§34–35.

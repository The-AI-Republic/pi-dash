# Public boards (space) — area build spec (SPC-001–065)

Area: Public boards (space): published project boards and the public issue
view. Parent epic NEWFRONT-99. Inventory
`.ai_design/new_frontend_implementation/parity/inventory/space.md`
(rows SPC-001–065, every row currently `oracle green`). Oracle scenarios
`apps/web_new/e2e/parity/space/` (nine spec files). This file consolidates and
supersedes the six earlier area notes `space-board-access.md`,
`space-board-chrome.md`, `space-layouts-cards.md`, `space-filters-peek.md`,
`space-engagement-comments.md` and `space-auth.md`; those may be retired once
this file is accepted.

Method: the old space frontend (`apps/space/**`), the Django views and
serializers under `apps/api/pi_dash/space/` and
`apps/api/pi_dash/authentication/views/space/`, and the running old app against
the seeded parity stack were read and driven as a **reference only**. Nothing
is imported, copied or translated from them. This prose is the only thing the
build slices and the area gate write code from. It contains no UI copy,
selectors, class names or styles — the only literals are addresses, parameter
names, API paths and field names. User-visible wording is written by each
slice from the kit and restated in the oracle scenarios, which must match the
old app's meaning without reusing its strings.

Status convention: the oracle (NEWFRONT-97, rows already green) pins today's
behavior on the old app. Build slices write the new app from this spec, cover
every changed line and branch with unit tests, and append `built` to the
Status cell of each row they implement. The area gate NEWFRONT-763 runs the
area's whole parity suite against `apps/web_new`, and turns green rows into
`new green`. Where this spec conflicts with the issue that created it, this
spec and the run rules in the issue description win; the slices are told to
remark it.

## 0. Reading guide

- §1 is the architecture the slices must follow: route shape, public session
  model, lazy loading, editions, and the address contract.
- §2 resolves every cross-epic seam: what this area builds itself and what it
  reuses read-only from the kit and from the Issues epic. Read it before
  writing each slice; it names the Issues slice that owns each shared piece.
- §3 is the behavior, row by row, grouped by slice. Each row states screens,
  states, rules, validation, empty/error states, permissions, shortcuts and
  the calls that back it. Row IDs are the inventory IDs and the oracle tags.
- §4 lists the rows that used to live inside a range heading, restated
  explicitly, and the five inventory confirmations resolved.
- §5 is the API endpoint and parameter table, with the fields the UI reads.
- §6 is the F-06 reuse map: what `apps/web_new` already has, and per slice what
  is added or reworked.
- §7 is the bug ledger: every `bug:` scenario, its filed issue and the
  behavior the new app builds instead.
- §8 is the proof plan: how each row becomes `new green`, how editions are
  proven, and what the gate must not do.

## 1. Architecture (decisions the slices must not reopen)

### 1.1 Route shape

Public boards are a separate web-only route subtree, not a part of the
workspace shell. The canonical addresses are:

| Address | What it is | Row |
| --- | --- | --- |
| `/spaces/` | Landing: auth gate for anonymous visitors, paste-a-link guidance for signed-in visitors | SPC-005, SPC-064, SPC-058–063 |
| `/spaces/issues/:anchor/` | The published board for one anchor | SPC-001, SPC-003, SPC-004, SPC-010–065 |
| `/spaces/:workspaceSlug/:projectId/` | Legacy workspace/project link that resolves to an anchor | SPC-002 |
| `/spaces/*` (anything else) | Generic not-found page | SPC-009 |

- **It is one subtree, not the workspace shell.** `/spaces/*` must not render
  the sidebar, the title bar, the workspace switcher, the command palette, the
  workspace session gate or any workspace-bound component. A visitor here has
  no workspace and often no account.
- **It is lazy.** The subtree's route modules, its board, its auth card and
  its editor are code-split so a visitor loading `/spaces/...` never downloads
  the workspace shell, the Issues epic's board/kanban code or the rest of the
  app. The shell chunk stays out of the space entry chunk. The gate proves
  this by loading a board in a fresh browser context and checking the shell's
  chunk never arrives, plus the size budget (`pnpm --filter web_new size`).
- **Web build only.** `/spaces/*` exists in the `web` build and is absent from
  the desktop build and its navigation. No desktop parity row exists for this
  area; the slices add no platform-specific code, no Tauri access, and no
  edition-specific branches inside the subtree.

### 1.2 Public, no-session model

The space app is public. That has concrete consequences the slices implement:

- **No workspace session gate.** The subtree never calls the session gate,
  never redirects to the workspace sign-in, and never imports workspace
  membership, permissions or sidebar features. It is reachable while signed
  out. Signed in or not, the board itself is the same: the same data, the same
  rows, the same filters and layout. What differs is confined to two slots —
  the account area (a sign-in control for an anonymous visitor, the avatar
  menu for a signed-in one; SPC-013/SPC-014) and the write affordances
  (composer versus invitation, vote and reaction write state; SPC-049,
  SPC-055). Nothing in the board body is hidden, gated, or rearranged by
  session state.
- **No session-expiry middleware.** `apps/web_new`'s default API client marks
  the workspace session expired on any 401 and bounces to sign-in. A 401 on
  the space subtree is a legitimate anonymous state (current-user,
  comment writes, vote writes, reactions), never a session expiry. The space
  subtree uses a **second client instance on the same transport and base URL**
  with no session middleware. The singleton builder lives in
  `core/api`; the subtree gets its own scoped client/query client factory
  from its public-shell module (§6, slice A) rather than reusing
  `getAppClient()`. No network I/O happens outside `core/api`; the space API
  modules call through an `ApiClient` argument exactly like the Issues feature
  does.
- **Workspace cache isolation.** Space data lives in its own query keys and
  its own query client. It must never read, write or invalidate the
  workspace cache, and signing out of a workspace must not clear a visitor's
  public board (and the reverse). Keys are rooted at `["spaces", anchor, ...]`
  and `["spaces", "auth", ...]`, never `["ws", ...]`.
- **Who the visitor is.** The subtree reads the current user through the
  public client (`GET /api/users/me/`): a signed-in visitor gets identity,
  avatar and name; an anonymous visitor gets 401, which the subtree treats as
  "anonymous" (not an error, not a redirect). That identity gates the
  composer, the owner menu and the navbar account slot (§3).
- **Signed-in visitors are not re-gated.** A signed-in visitor opening a board
  sees the same board plus their account controls; nothing forces them back to
  `/spaces/` guidance. The landing's guidance view is only what `/spaces/`
  itself shows to a signed-in visitor with no valid return path.

### 1.3 Address and parameter contract

All names are lower-case/camelCase exactly as below; the oracle asserts these
spellings.

| Parameter | Where | Meaning | Notes |
| --- | --- | --- | --- |
| `anchor` (path) | `/spaces/issues/:anchor/` | Published board's anchor | Matches `[A-Za-z0-9_-]+`; anything else is treated as unknown and renders SPC-003 |
| `workspaceSlug`, `projectId` (path) | `/spaces/:workspaceSlug/:projectId/` | Legacy identify of the project | Resolved server-side to the anchor |
| `board` | board address | Active layout | Values `list`, `kanban` only (the two renderable ones). Unknown or missing settles on the first publisher-enabled layout |
| `peekId` | board address | The open issue's UUID | Present = overlay open; absent = closed. Invalid values are dropped |
| `priority` | board address | Comma-joined priority values; five values total | `urgent`, `high`, `medium`, `low`, `none` (the fifth, `none`, is the unset level) |
| `state` | board address | Comma-joined state UUIDs | One key, comma-separated values |
| `labels` | board address | Comma-joined label UUIDs | Address-only facet (§3, SPC-036/037) |
| `next_path` | landing address and every sign-in/sign-out link/form | Return address, **relative to the space subtree root** | One leading `/`, **no `/spaces` prefix**, query preserved; validated (§3, SPC-006). Two shapes exist — see the next rule |
| `error_code` | landing address | Server error code after a native form post | Lands on the fixing step (§3, SPC-063) |
| `email` | landing address | Pre-fills the address field | Optional |

Rules that apply everywhere:

- **Two `next_path` shapes — do not confuse them.** The carried value is
  **subtree-relative**: it names a position inside `/spaces` and drops the
  mount segment. A board at `/spaces/issues/<anchor>/` yields a value that
  starts `/issues/<anchor>` and echoes the trailing slash the visitor is on;
  with a filter, overlay or both the query rides along
  (`/issues/<anchor>?board=kanban&peekId=…`, or with the slash before `?`).
  The oracle carries the slash-less form and asserts only the anchor and the
  parameters, so both spellings pass; the value must not gain a mount
  segment. The server is what re-attaches
  the spaces base (`base_host(..., is_space=True)` in
  `apps/api/pi_dash/authentication/utils/host.py`, consumed by
  `authentication/views/space/email.py` and the provider callbacks), so a
  value that already carried `/spaces` would redirect to `/spaces/spaces/…`.
  Only the **document address of the landing link** spells the mount out
  (`/spaces/?next_path=…`) — that is a link the browser follows, not the value
  in the hidden form field. SPC-006 and SPC-013 use the same split.

- **View state lives in the address.** Board, open issue and filters are
  parsed with a zod `validateSearch`; invalid values fall back to defaults and
  the address is rewritten to the settled form (SPC-022).
- **Trailing slash.** `/spaces/*` addresses settle with a trailing slash
  under the router's `trailingSlash: "preserve"` because the links the subtree
  authors carry it; pasted slash-less addresses are rewritten on the first
  client navigation. The oracle checks the canonical `/spaces/issues/:anchor/`
  form only, not whether a server redirect added the slash.
- **Legacy links keep only what the old app kept.** The legacy link carries
  `board` and `peekId` across; it does not carry filter parameters (old
  behavior, SPC-002). The new app preserves that set exactly — inventing
  extra parameter forwarding would be a divergence the oracle cannot prove.
- **Opening a card preserves filters.** Activating the card's open region — the
  **title link** in list rows, the **card body link** on board cards (SPC-027)
  — rewrites the address keeping `board` and every filter key and adding
  `peekId`; closing drops only `peekId`. (This is the target for SPC-030/042;
  the old app dropped filters when a card was clicked from a filtered board —
  see §7, no separate bug ID, folded into the same close/open address rule.)
- **Masked spelling.** The old app's parameter names are part of its public
  links; the new app uses the same names. A future rename would need a
  redirect table and a new decision; the slices do not rename them.

### 1.4 Editions

Rows are tagged `oss, cloud` identically. Reading `apps/space/**` found **no
edition branching**: no `ce/` alias, no `ee-overlay` swap, no build-time
edition constant. The only variation is instance configuration:

- whether mail is configured and magic-link login is enabled (code vs password
  step, SPC-059–061),
- whether password login/email password auth is enabled (SPC-059, SPC-061),
- which third-party providers are enabled (SPC-062),
- whether signup is enabled (an instance/server setting: the card does not
  read it when choosing a step; the server refuses a sign-up post with
  `SIGNUP_DISABLED`, which the card renders as a banner -- SPC-059),
- the product site address for the badge (a build-time constant, SPC-012),
- which locale packs ship (SPC-065; OSS ships English only).

**Edition proof:** the space subtree carries no `edition` flags, slots, route
extensions or `if (edition)` branches. Every row is proven per edition (OSS and
cloud) by serving the same subtree under both stacks and running the same
scenarios. The two behaviors that differ are proven by *configuring the
instance* rather than by edition: (a) magic-link/mail on and off gives both
auth method paths (SPC-059/060/061), and (b) provider flags on and off gives
both provider states (SPC-062). OSS-only constraints are proven on OSS; any
cloud config that is not available on the OSS stack is recorded by the gate as
a config-dependent extension of the same scenario, not as a new row. Server
endpoints used by this area are `AllowAny` public endpoints with no edition
gate.

### 1.5 Translations

Every visible string in the subtree goes through `core/i18n` (`useLocale`,
locale packs). Identifiers (issue keys, anchor, UUIDs), server names (project,
state, label, member display names) and date/number shapes are data, not text,
and never go through translation. OSS ships English and falls back to English
for unsupported stored locales (SPC-065).

## 2. Cross-epic seams

Every row must be provable at the area gate without depending on Backlog
epics: the gate runs after this area's slices and the Issues area gate
NEWFRONT-658, nothing else. So each slice either builds its piece itself or
reuses a component whose Issues slice is already merged or guaranteed by
NEWFRONT-658. Read-only reuse is the rule: the space subtree never mutates
Issues feature state, never mounts workspace-bound chrome, and never forks a
component to get "a slightly different" version. When an Issues component does
not fit, the slice asks for it through the Issues feature's public barrel
(`features/issues/index.ts`) or writes its own in the spaces feature. Both are
new code; nothing is copied from `apps/space`.

| Need | Owner | How space uses it |
| --- | --- | --- |
| Generic primitives (button, icon button, input, textarea, switch, avatar, badge, menu, popover, tooltip, dialog, skeleton, spinner, toast host, virtual list, markdown view, empty state, theme helpers) | `@pidash/kit` (already merged) | Direct import. This is the default for everything generic. |
| Layout icons and the five layout names | Issues `layouts/kinds.ts` + `components/LayoutIcon.tsx` (merged) | Import through the Issues barrel. The spaces board offers only `list` and `kanban`, intersected with the publisher's flags. |
| Layout switcher UI in the workspace shell | Issues `components/LayoutSwitcher.tsx` (merged) | **Not reused.** It is workspace-styled and offers five layouts. Space builds a small segmented switcher in its own feature (§3, SPC-020/021) using kit `IconButton`/`Tooltip` and `LayoutIcon`; read-only. |
| List layout rendering | Issues NEWFRONT-619 slice (merged) | **Not reused as a whole.** `ListLayout` is bound to workspace entities, drag/drop and inline property editing; the public board is read-only with different paging. Space builds its own grouped list (§3, SPC-023/026) from kit primitives. |
| Kanban layout rendering | Issues NEWFRONT-622/623 | **Not reused.** Same reason; space builds its own columns (§3, SPC-024/026). |
| Card (block) rendering | Issues card slices | **Not reused.** Cards are read-only, have a public key `IDENTIFIER-sequence` and a different footer (§3, SPC-027–029); space writes its own. |
| Dropdown primitives (filters panel, prioritize/state option lists, searchable long lists) | Issues NEWFRONT-615/616 | Reuse the **kit** primitives (`Popover`, `Menu`, `Combobox`, `Tooltip`) those slices are built on. Do not import workspace filter controls. Space's filter panel (§3, SPC-034) is its own component with two facets. |
| Filters engine (rich filter builder, saved views, condition pills) | Issues NEWFRONT-626–629 | **Not reused.** Public filters are three fixed facets carried in the address; the space feature keeps its own search schema and fetch parameters (§3, SPC-034–038). |
| Display options panel | Issues NEWFRONT-618 | **Not reused.** The public board has no display options; what is displayed is fixed (SPC-027/028). |
| Create modal / quick create | Issues NEWFRONT-630 | **Not present at all.** The public board is read-only (SPC-032); no create affordance exists anywhere. |
| Issue detail / sidebar | Issues NEWFRONT-639/640/642 | **Not reused.** The overlay is a public read-only presentation (§3, SPC-040–046) built from kit + shared read-only pieces below. |
| Shared peek panel shell | Issues NEWFRONT-646 (`shared/shell/PeekPanel`, merged) | Reuse the shell (close control, Escape handling, eyebrow/actions slots, `children` body) with **one required shell change**: the merged panel always renders the title as an editable input with a commit handler (`PeekPanel.tsx`), and SPC-043 requires the title to read as text with **no edit affordance**. The shell gains an explicit read-only title variant (e.g. `titleReadOnly` / no `onCommitTitle` renders text, not an input, with no hover/focus border and no draft state), and the workspace peek keeps its editable title unchanged. That variant is a shell/Issues dependency (SHELL slice or a small follow-up linked from NEWFRONT-646), landed before the space overlay slice depends on it. Until it lands the space H slice must not mount the editable field, and must not fork the panel. The public feature wraps the shell with its own three presentations (side/modal/full), its own property panel and read-only body; if the side/modal/full switcher is not in the shell, the space slice adds it in its own wrapper (not in the shell) so the shell stays workspace-free. |
| Bulk edit | Issues NEWFRONT-633 | **Not present.** Read-only board (SPC-032). |
| Comments thread, cards, edit, fold, speaker labels | Issues NEWFRONT-651 | Reuse **read-only presentation** pieces through the Issues barrel **only where they fit as-is**: the bot mark, fold helpers (`isCommentFolded`, SPC-051), `CommentEditForm` and the shared `RichTextView`. `CommentCard` does **not** fit without an Issues change: (a) it has no own-line speaker label — `commentAuthorName` substitutes the label into the header name, while SPC-052 needs the label on its own line above the body; (b) its props require `workspaceSlug`/`projectId` and it mounts the workspace-scoped `CommentReactionPicker`. The required Issues change is an optional own-line speaker slot plus public-scope reaction handlers (or a `readOnly` mode that suppresses the picker and takes write callbacks), with workspace behavior unchanged; it lands before J1 depends on it. If that change is not made, space renders its **own** public comment card from SPC-050–052 (new code, not a copy of the old row). Do **not** reuse the Issues `CommentThread`/`comments/api` as-is: they are keyed by workspace and hit workspace endpoints. Space keeps its own query functions for the public endpoints. If a component hardwires workspace fetches, it is not reused; the space slice writes its equivalent and notifies the Issues owner rather than forking silently. |
| Reactions (issue + comment chips, picker, grouping, own-highlight, reactor names) | Issues NEWFRONT-654 | Reuse **presentational** pieces (`ReactionBar`, `ReactionPicker`, `EmojiPicker`, and the model helpers `groupReactions`, `viewerHasReaction`, …) through the Issues barrel. Same caveat: public data goes through space's own query functions because the Issues hooks are workspace-scoped; the write path is the public reaction endpoints. |
| Composer (rich text, attachments, mentions, edit) | Issues NEWFRONT-651/654 + `shared/editor` | Reuse `shared/editor/RichTextEditor` (lazy), `RichTextEditorHandle`, `RichTextView`, and the mention extension. The space composer adds the public image upload (public asset endpoint) and submits to the public comment endpoint. Member suggestions come from the public members endpoint. |
| Shell toasts and notice helpers | `shared/shell/toaster.ts` (merged) | Reuse `notifySuccess`/`notifyError`/`notifyInfo` and mount one kit `ToastHost` in the space layout. The space layout owns its own host so it never depends on the workspace shell mounting. |
| User menu / account menu | `shared/shell/UserMenu` (merged) | **Not reused.** It offers Your-work, Settings, deactivate and workspace role; none exist for a public visitor. Space builds a one-entry account popover (sign-out only; identity lives on the avatar + name trigger) from kit `Menu` + `Avatar` and posts the sign-out form (SPC-014). |
| Clipboard copy helpers / copy-link button | `shared/shell/clipboard.ts`, `CopyWorkItemLinkButton` | The clipboard helper's notice text and the copy-link button are workspace-flavored. Space writes a tiny copy action in its own feature using `navigator.clipboard` and the shared toast (§3, SPC-045). |
| Current user / instance config | `core/session` queries + `@pidash/api-client` `auth`/`instance` contracts (merged) | Space reads current user and instance config **through the public client**; it never imports the workspace session store or gate. |
| Document head (title, robots, meta) | `shared/shell/document.tsx` `useDocumentHead` (merged) | Reuse for the board title and noindex (SPC-007/008). |
| Theme | `core/theme` (workspace, persisted to profile) | **Not reused for persistence.** A public visitor has no profile. Space uses `@pidash/kit` `resolveTheme`/`applyTheme`/`watchSystemTheme` with a space-scoped storage key and an OS default (§3, SPC-011). |
| Not-found page | `shared/shell/NotFoundPage` | Reuse for the **generic** unmatched address (SPC-009). Anchor-unknown (SPC-003) and board failure (SPC-004) are space-specific illustrated states in the space feature. |

What this area builds itself (slice by slice, §6): the `/spaces/*` shell and
route tree, the board data layer against the public endpoints, navbar and
layout switcher, list and kanban read-only renderers with per-group paging,
filters and pills, public cards, the overlay with three presentations, votes
and reactions wiring, comments and composer against the public endpoints, the
auth card that talks to `/auth/spaces/*`, and the space-specific failure
surfaces. Two shell/Issues pieces are **dependencies, not borrowables
as-is**: the peek panel's read-only title variant (SPC-043, §6.2 H) and
`CommentCard`'s own-line speaker label plus public-scope callbacks
(SPC-052, §6.2 J1). Each is a small change in its owner's slice, linked from
NEWFRONT-646 and NEWFRONT-651 respectively; space waits for it or, for the
comment card only, renders its own public card. No space slice forks either
component, and the gate runs after NEWFRONT-658, so both changes must be on
`web-new-dev` by then.

## 3. Behavior, row by row

Each subsection is one slice from the epic's split. The slice names its rows;
the rows name what the visitor sees, the rules, and the calls. "Reads" means
fields of the response the UI displays; the full table is §5.

Row-to-slice map (every row appears exactly once; the split's Done-when):

| Slice | Issue | Rows |
| --- | --- | --- |
| A — public shell, routing, failures | NEWFRONT-753 | 001, 002, 003, 004, 009, 016, 018, 019 |
| B — landing auth gate, metadata | NEWFRONT-754 | 005, 006, 007, 008 |
| C — spaces auth card and flows | NEWFRONT-755 | 058, 059, 060, 061, 062, 063, 064 |
| D — navbar, theme, account, notices | NEWFRONT-756 | 010, 011, 012, 013, 014, 015, 017, 065 |
| E — layouts, list/board, paging | NEWFRONT-757 | 020, 021, 022, 023, 024, 025, 026 |
| F — filters, pills, encoding | NEWFRONT-758 | 034, 035, 036, 037, 038, 039 |
| G — cards, read-only behavior | NEWFRONT-759 | 027, 028, 029, 031, 032, 033 |
| H — overlay shell and content | NEWFRONT-760 | 030, 040, 041, 042, 043, 044, 045, 046 |
| I — votes and reactions | NEWFRONT-761 | 047, 048 |
| J1 — comments thread and rows | NEWFRONT-762 | 049, 050, 051, 052 |
| J2 — composer, edit, delete, comment reactions | NEWFRONT-775 | 053, 054, 055, 056, 057 |

The order the epic uses is A → B → D → E → F → G → H → I → J1 → J2, with C
mounted by B. Row 030 lives in slice H's section (the split assigns the open
action to H) even though the card it activates is slice G's; G's card rows
cite it where the link region is defined.

### 3.1 Slice A — public shell, anchor routing and failure surfaces

**SPC-001 — Anonymous board load.**
A published board opens at `/spaces/issues/:anchor/` for a visitor with no
account. The page renders: the board chrome (navbar with project identity and
controls), the published issues for the board's layout, and — when anonymous —
a sign-in control that carries the exact board position (§3.1, SPC-013).
Nothing on load redirects toward sign-in, shows a workspace sign-in form, or
gates content behind the workspace session. The load sequence is: instance
probe (§3.1, SPC-018) → board settings → layout + issues. The board is
reachable in a fresh browser context with no cookies at all. Reads:
`settings` (row 1 of §5). A published board with zero issues renders the
empty-board notice (SPC-025), not an empty layout.

**SPC-002 — Legacy workspace/project link.**
`/spaces/:workspaceSlug/:projectId/` resolves the project's published board
server-side (`GET /api/public/workspaces/:slug/projects/:id/anchor/`). On
success the app forwards to `/spaces/issues/:anchor/`, carrying exactly
`board` and `peekId` when the legacy address had them (nothing else), and
the board validates them like a pasted address. While resolving, the product
spinner fills the viewport (never a blank page). Unknown workspace/project,
a malformed project id, an unpublished project (server 404), or a resolved
payload whose entity is not a project all land on the **generic** not-found
page (SPC-009), not on the anchor illustration. A project whose board exists
but whose anchor is later unknown resolves through the board and lands on
SPC-003, naturally.

**SPC-003 — Unknown or withdrawn anchor.**
Any non-2xx answer from settings other than a transient/network failure that
the app classifies as 404 renders a dedicated illustrated not-found state in
the space design: illustration, one heading, one explanatory line. The board
chrome (navbar, controls, layouts) does not render. An anchor failing the
`[A-Za-z0-9_-]+` shape never reaches the network and renders this same state.
This is distinct from SPC-004 (board-error illustration) and SPC-009 (generic
unmatched path).

**SPC-004 — Other board failures.**
A settings failure that is not "anchor unknown" (5xx, network failure,
unparseable payload) renders the **board-error illustration** with the
navbar *not* rendered. An issues-list failure after settings succeeded
(5xx, network) renders the board-error illustration under the resolved
navbar. In both cases: no spinner left spinning, no blank page, no layout
shell with empty groups. The old branch offers no retry control, so neither
does the new one; recovery is a page reload or a later navigation. The
wording is a neutral "we couldn't load this board" message — not the old
app's misleading "missing board" line, and not an invitation to publish
anything.

**SPC-009 — Unmatched paths.**
Anything under `/spaces/` that matches no route — single unknown segment, deep
unknown path, a bare `/spaces` without the trailing slash after normalization —
renders the **generic** guidance page: illustration, heading, and a line
telling the visitor to check the address. This is the same page the
unpublished legacy link lands on and the catch-all for the subtree. Do not
render board chrome on it.

**SPC-016 — Loading states.**
Four stages, each with one visible state and no flash of stale or wrong
content:
1. *Instance boot*: while the instance probe is in flight, a full-viewport
   product spinner. Nothing else.
2. *Legacy resolution*: the product spinner until the forward happens.
3. *Board resolution*: while board settings are loading or held, the navbar
   renders immediately in its placeholder form (generic project mark, a
   placeholder project name, empty/disabled controls) over an empty content
   area; this stage shows **no** full-viewport spinner and no flash of wrong
   content. Evidence: the old layout's spinner branch requires a falsy
   settings value, but its settings hook returns an empty object for an
   unresolved anchor, so that branch is unreachable; the green oracle pins
   the placeholder navbar with empty content (`placeholderWhileHeld` true and
   `sawSpinner` false for the settings stage). Keep that behavior:
   placeholder chrome, empty body.
4. *First issues fetch*: a centered product spinner under the loaded navbar.
Content (groups, cards, counts) appears only after its own data is present,
never as an empty-group flash followed by data. Once each stage settles it
hands off to the next; none of these states is a terminal error state (those
are SPC-003/004/018/019).

**SPC-018 — Unreachable backend.**
If the instance probe fails (network error, 5xx, non-JSON), the app renders the
**instance-failure screen**: the product logo/wordmark bar, a themed
illustration, an explanatory heading and a retry control. Retry performs a
hard page reload. The probe is a single attempt (no client retries), so a
single aborted request deterministically reaches this screen. While the probe
is pending, SPC-016 stage 1 applies. The space failure screen is its own
component: it fills the viewport, carries no workspace chrome, and is themed
(light/dark) from the space theme choice.

**SPC-019 — Global crash.**
A render fault anywhere in the subtree — including a malformed payload that
makes a renderer throw (e.g. an unparseable project mark) — is caught by the
space error boundary and renders the crash screen: an apologetic heading, a
refresh control that reloads, and a contact line with a support-mail link and
a community/forum link. The boundary is per subtree (it does not take down
`/spaces/` navigation once a later route renders cleanly after refresh) and
does not render the workspace crash screen. Reload with the payload fixed
returns the board.

Implementation notes for A: own `errorComponent` and `notFoundComponent` on
the space subtree; the settings error is classified into 404 → SPC-003,
anything else → SPC-004; the probe is a query under the space client; the
spinner is the kit `Spinner` in its large/full-viewport presentation. No
data-testid is added to the old app.

### 3.2 Slice B — landing auth gate and board metadata

**SPC-005 — Landing page is the auth gate.**
`/spaces/` is the public root. While the current-user probe resolves, a
branded full-viewport spinner (SPC-016 stage 1 rules, on the space design).
Then:
- *Anonymous*: the email-first sign-in/sign-up card (§3.11, SPC-058–064).
  The card's mode is not chosen by the visitor; the server's address check
  chooses sign-in vs sign-up (§3.11).
- *Signed-in, no valid `next_path`*: the **guidance view** — product mark,
  the account control (avatar + menu), an illustration and a short heading
  and line telling the visitor to paste the public link they want to open in
  the address bar. No form.
- *Signed-in with a valid `next_path`*: replace-navigate to that path; the
  guidance view is not shown. If the path is invalid it is dropped
  (SPC-006) and the guidance view shows.
- *Anonymous with a valid `next_path`*: the auth card shows and every
  auth step carries the return path through its forms; after success the
  server's redirect lands on the path and the board opens.
The landing carries the trailing slash (`/spaces/`) and the product badge
(SPC-012). It does not carry board chrome.

**SPC-006 — Return-address validation.**
One validator, used by every space entry point: landing `next_path`, the
navbar sign-in link, the sign-out form, the vote/reaction/comment sign-in
redirects, the auth forms' hidden field and every auth redirect the card
follows. **Shape:** the value is subtree-relative (§1.3) — it starts at the
space root, carries no `/spaces` segment, keeps its query, and is encoded
once wherever it appears inside a link. The server adds the mount on
redirect. Rules (all must pass or the value is dropped):
- a string,
- starts with exactly one `/` (so `//host` and scheme URLs fail),
- no backslash anywhere,
- no control characters, whitespace, or `javascript:`/`data:`/`vbscript:`
  active-content payloads anywhere (case-insensitive, decoded),
- it parses as a relative path and stays same-origin (no `scheme://`, no
  `@`-encoded authority tricks, no `%2f%2f` escape that becomes an authority).
Valid values keep their own query string (a board address with `board`,
`peekId` and filters survives the round trip). When dropped, the visitor
lands on the safe default: the spaces landing document (`/spaces/`) for the
gate (showing guidance if signed in, the card if not), and the board for the
sign-in links. The old app builds the value from the router's mount-stripped
pathname (`usePathname()` under the `/spaces` basename) and the green oracle
asserts the carried string `/issues/<anchor>/` with its query; the new app
must produce the same shape, not a mount-prefixed one.
This resolves inventory confirmation (5): `next_path` uses the same shared
validator as every other return address; there is no second, weaker checker
for the landing, and the new app implements one function in the space
feature (it does not import the workspace guard's function, which is
workspace-shaped, but the rules are the same).

**SPC-007 — Per-board metadata.**
The board's document head carries, from `GET /api/public/anchor/:anchor/meta/`
(a project-lite payload: `id`, `identifier`, `name`, `cover_image`, `icon_prop`,
`emoji`, `description`):
- title = board name, description = board summary, plus `og:title`,
  `og:description`, `og:type=website`, `twitter:card=summary`,
  `twitter:title`, `twitter:description`.
- **No cover image tags at all**, even when the project has a cover
  (`cover_image` in the meta payload is ignored). This is the target
  (main#523 — an open PR removing the inherited cover-image feature), and it
  is what the new app implements. The old head differs: it declares
  `twitter:card = summary_large_image` and, when `cover_image` is present,
  adds `og:image`, `og:image:width=800`, `og:image:height=600`,
  `og:image:alt` and `twitter:image`. The spec keeps those out by intent, not
  because the old app omitted them; an oracle that pins the old tags is
  `bug:`-shaped for this row, and the gate asserts the new output.
- Fallbacks: unknown anchor, malformed anchor, server error, or a payload
  without a name/summary → the generic product title and description. The
  loader resolves the API base the same way the rest of the app does, so the
  old root cause (NEWFRONT-453: relative base → always generic) cannot recur.
How the head is emitted depends on the host: `apps/web_new` builds a static
SPA, so the served document is one shell for every address and per-board tags
exist only in the live document, updated on navigation and on the first
board render. The build slice must therefore (a) keep the generic tags in the
served shell as the safe default and (b) set the board's tags in the live
document before the board content is announced. If the deployment can serve
per-address HTML (the way the old app did), the same function feeds it; that
is a hosting capability, not a second behavior. A malformed anchor never
produces a request with the malformed value.

**SPC-008 — Crawler directive.**
Every page in the subtree carries a `robots` directive `noindex, nofollow`:
board, landing, legacy-resolution, not-found and error pages alike. The
subtree sets it on its root (`useDocumentHead`) and no child route loosens it.
**What the oracle proves** is the live document's `<meta name="robots">` on
the board and on the landing (both read `noindex, nofollow`); it does not
fetch and parse the served HTML, so the served document is a separate
constraint below, proven by the gate's host check rather than faked. Two hard
constraints for the slices and the gate:
- The app root today claims `index, nofollow` as the workspace fallback. The
  space subtree must claim `noindex, nofollow` at its own root so the claim
  wins for every space page in the live document.
- The standard static SPA shell is shared by workspace pages, so the served
  document may not vary by address. The row's "served pages" requirement is
  met by whatever document the space addresses are served with — for the
  static build, the space subtree's host configuration (for example a
  response header/Caddy/`_headers` rule for `/spaces/*`) must add the
  directive, or the deployment must serve a space shell. This is an
  infrastructure task for slice A, not something to fake with a client-side
  script. If no host change is possible in the parity stack, the gate records
  the exact gap against SPC-008 and links the hosting issue; it does not
  relax the row.

### 3.3 Slice D — board navbar, theme, account and notices

**SPC-010 — Navbar project identity with fallbacks.**
Left side: the project's mark, then its name. Mark branches, in order:
1. a glyph icon chosen from the project's `logo_props` (icon name → kit icon),
2. an emoji from `logo_props`' emoji branch (decimal code points),
3. no configured mark → an empty slot (the old app renders nothing; do the
   same — do not invent a fallback mark for a configured-empty project).
4. no project details at all in the settings payload (placeholder during
   resolution or a malformed payload) → the bar stays intact with a generic
   project mark and a short placeholder name.
The name comes from `project_details.name`; long names truncate in the bar
with the full name on hover. Reads: `project_details.{name, logo_props,
identifier, cover_image}`. Covers are **not** rendered as a banner on the
public board (the old board has no cover banner; the cover appears only in
metadata, which SPC-007 disables).

**SPC-011 — Theme switch.**
The navbar carries a light/dark toggle. Rules:
- A first visit with no stored choice **follows the OS scheme** (dark OS →
  dark) and stores nothing; the mark follows OS changes live while nothing is
  stored.
- The first toggle from the OS default **flips** to the opposite of what is
  currently showing and stores that explicit choice. (Old bug NEWFRONT-456:
  the first click stored explicit light regardless of the current scheme;
  the new app implements the flip.)
- Later toggles alternate; the explicit choice persists across reloads in the
  space-scoped storage key; the document theme and the toggle's own state
  agree at all times.
- There is no "follow system" entry in the public switch: once an explicit
  choice is stored the OS no longer affects the board; clearing the stored key
  returns the next load to the OS default (the gate exercises this through
  storage, not through a visible control).
The kit's `resolveTheme`/`applyTheme`/`watchSystemTheme` are used with a key
namespaced for space; no profile write.

**SPC-012 — Product badge.**
A small branded badge is fixed to the bottom-right corner of three screens:
the board layout, the landing's auth view, and the landing's signed-in
guidance view. It opens the configured product site in a new tab with
`noopener noreferrer`. Its address is a build-time constant; when the
constant is absent or the badge is disabled, it renders nothing (no empty
slot, no broken link). Scope is a deliberate match of the old app: the
root-level not-found page, the crash/error screen and the legacy resolution
page do **not** mount the badge (they render outside the board layout and the
landing's two views), and neither does the in-frame or loading state. The
badge stays pinned while the board scrolls and must not intercept clicks
meant for content behind it.

**SPC-013 — Anonymous sign-in remembers the position.**
The navbar account slot for an anonymous visitor is one sign-in control
whose href is the spaces landing document with the return address as its
query — `/spaces/?next_path=<value>` — where `<value>` is the **subtree-
relative** current position (§1.3, SPC-006): `/issues/<anchor>` with
`board`, every filter key and `peekId` in its own query, URL-encoded once.
No `/spaces` segment appears inside the value. Rules:
- The return address is the full current position, encoded as one value
  (old bug NEWFRONT-465: the board address was spliced unencoded, so the
  second and later parameters leaked out; the new app encodes once).
- The validator of SPC-006 decides whether it survives.
- The slot renders nothing inside an embedding frame (SPC-015).
- The old app's sign-in link target is the landing root; keep that (the
  landing then forwards after auth).
Reads: current-user (present/absent); the link is computed from the router
search, not from a store snapshot.

**SPC-014 — Account menu.**
A signed-in visitor sees an account trigger: avatar plus a single name line.
The label fallback is display name → first + last name → email → a neutral
translated label. That fallback is the **new-app intent**: the old trigger
repeats the first name when there is no display name (NEWFRONT-823), which is
a bug the new app does not rebuild. Clicking the trigger opens a menu whose
content is **exactly one action**: sign-out. First and last name, email and
avatar all live on the trigger, not inside the menu; the menu has no identity
block, no profile link and no settings. Rules:
- Sign-out is a native form post to `POST /auth/spaces/sign-out/` with the
  CSRF token (`GET /auth/get-csrf-token/`, fetched when the menu opens or at
  mount) and a hidden `next_path` carrying the subtree-relative current board
  position — `/issues/<anchor>` plus board, filters and peekId in its query,
  no `/spaces` segment, encoded once (§1.3, SPC-006). The form is rendered
  only once the token has loaded; before that the menu is empty (old
  behavior, harmless and not worth a spinner).
- After sign-out the server redirects to the return address; the board
  reloads as anonymous chrome with the same position; the menu is gone.
- No Your-work, settings, deactivate or workspace entries exist here.
- The menu is absent inside embeds (SPC-015).
FIELD: `display_name`, `first_name`, `last_name`, `email`, `avatar_url`.
The trigger label is the only identity the oracle reads; it asserts the
display name and that sign-out is offered.

**SPC-015 — Embedded frame hides account controls.**
The subtree detects an embedding frame (`window.self !== window.top`, same
rule as the old hook, with no user-agent heuristics). Inside a frame:
- navbar account slot: nothing in either state (no sign-in button, no avatar
  menu);
- emoji add controls: not rendered (overlay and cards alike — the old app's
  card footer missed this check; the new app applies the frame gate to both,
  as SPC-015 owns the frame-wide claim);
- comment compose, owner menu and the comment sign-in invitation: not
  rendered;
- everything else stays readable: navbar identity, fields, switcher, filters,
  cards, open overlay, thread and reaction **counts**.
- vote controls render but are **inert**: the old controls return before any
  handler when the page is framed (both the up and the down button), so a
  click in a frame neither writes a vote nor redirects to sign-in. The new app
  keeps that: inside a frame the vote controls keep their counts and their
  pressed state but do nothing on activation, and they carry a disabled
  affordance (inert styling and no pointer/hover affordance) so the frame
  visitor is not misled. Outside a frame the same controls are live (SPC-047).
  The card-footer emoji add hole (the old card footer offered a working add
  control while the overlay hid it) is NEWFRONT-469; the new app hides the add
  control in both places.
  A signed-in session inside a frame still reads; the hide is about UI, not
  permissions.

**SPC-017 — Navigation progress and notices.**
- A route transition that takes long enough to matter (the legacy resolution
  is the proof) shows a top progress bar while in flight and removes it when
  settled. Instant transitions show none.
- Short notices (the kit toast host mounted by the space layout, bottom or
  top per the design) report: copy-link success, copy-link failure, comment
  post failure, comment post success where the design confirms it, vote
  failure, reaction failure, comment edit failure, comment delete success and
  failure, attachment upload failure. Notices are dismissible/auto-expiring,
  one line plus optional detail, and never block the board.
- A failed write never changes the visible data except through the
  reconcile rules of its own row (SPC-047/048/056/057).

**SPC-065 — Locale fallback with stable identifiers.**
All visible chrome, filter, auth and notice text goes through translation.
With the OSS build's single locale: an unsupported stored locale falls back to
English, the document language stays English, and the stored value heals to
English on boot. Identifiers (keys, anchors, UUIDs), server-provided names
and date shapes are unchanged by locale. The gate proves this on OSS by
storing an unsupported locale and capturing board properties, keys, navbar
and badge text and the landing title/subtitle/consent text before and after
(all stable). Cloud locale packs extend the same assertion; they are not a
separate row.

### 3.4 Slice E — layout switcher, list/board layouts and per-group paging

**SPC-020 — Only publisher-enabled layouts are offered.**
The navbar carries a small segmented layout switcher with one button per
offered layout and a tooltip naming it. The offered set is the intersection of
(a) the two layouts the public board can render — stacked **list** and
sideways **board (kanban)** — and (b) the layouts the publisher enabled in
`settings.view_props` (`list`, `kanban`; the payload also carries
`calendar`, `gantt`, `spreadsheet`, which the public board never renders and
the switcher never shows). A board with one enabled layout shows a
one-button switcher (no effective choice) but still renders it; a board with
none enabled (malformed settings) falls back to list so the board is usable —
**this fallback is a new-app choice**, the old app selects nothing in that
configuration, and the oracle pins only publisher-enabled subsets.
The old app builds the acceptable list from all five flags and then picks the
first, which can settle on a layout it cannot render; the new app restricts
the candidate set to the two renderable ones **before** settling (see
SPC-022). This changes only malformed/never-produced configurations; the
oracle's enabled subsets are unaffected.

**SPC-021 — Layout switch is shareable.**
Clicking a switcher button flips the layout immediately, keeps the loaded
issues (no issue fetch on a switch), keeps `peekId` and every filter
parameter, and rewrites `board` in the address. A pasted address carrying
`board` opens directly in that layout. Switching back to a layout already
loaded reuses its data. The switcher is keyboard operable (buttons, focus
visible, tooltip on focus too) and announces the pressed state.

**SPC-022 — View validation and fallback.**
On arrival, `board` is validated against the offered set (SPC-020). Missing
or unrecognised → the first offered layout in list-then-board order, and the
address is rewritten to name it. The old app rewrites with a history push, so
Back returns to the malformed address and it settles again; the new app
**replaces** instead, so settling adds no history entry and Back does not
loop. That is a new-app choice, not old behavior; the oracle pins that the
settled address names the layout, not the entry count. The rewrite keeps
every other parameter that was present. The settled address is what the
share and sign-in links carry.

History model, stated once for the whole board: **settling or correcting an
address replaces** the current entry (a fallback or a dropped invalid value
adds no entry); **opening the overlay pushes** exactly one entry (SPC-030);
**closing the overlay replaces** that entry with the board address, dropping
only `peekId` (SPC-042). So Back from an open overlay returns to the pre-open
board, and Back after a close leaves the board as if the overlay had never
been opened. This is the single model; SPC-030 and SPC-042 cite it rather
than picking their own.

**SPC-023 — List layout.**
Issues group under one collapsible section per workflow state, in the
project's state order (`GET .../states/`, ordered by `sequence`). Each header
shows the state's colour marker, its name and the count for that group as
returned by the issues response (`total_results` per group). States with no
issues **still render a header with a zero count**. Clicking a header
collapses/expands its rows; collapsing keeps the paging state. Rows show the
public key and title (SPC-027/028). The list is not virtualised in the
public board (group paging already caps the DOM; if the kit's `VirtualList`
is used per group it must keep the header sticky and focus order intact).

**SPC-024 — Board layout.**
One fixed-width column per workflow state in a sideways-scrolling strip;
columns keep their state order and empty states render an empty column, not
nothing. Each column header shows the same marker + name + count treatment.
The strip scrolls horizontally with touch, trackpad and shift+wheel; columns
scroll vertically on their own. The header row stays visible while a column
scrolls. Public board columns never offer drag targets (SPC-032).

**SPC-025 — Empty board.**
The empty test is the fetch's overall `total_count` — the server's count of
published issues matching the *current filter set* — not "no rows rendered".
When it is zero, neither layout renders any of its own surface: no group
headers, no column shells, no skeleton. Instead one centred notice fills the
content area (the new app writes its own line in the kit's empty-state
treatment and may add a single action such as a link back to `/spaces/`).
The test lives in one wrapper around both layouts, so list and board cannot
diverge. The navbar stays live above the notice: layout switcher, Filters and
the applied strip (when something is applied) all keep working, so a visitor
can switch layouts or undo the filter that produced the empty result. The
notice is reached in both layouts, and switching layout keeps the notice.
Neighbouring states that are deliberately not this one:
- *Fetch failed* → the error surface (SPC-004), never the empty notice.
- *Filters exclude everything* → **the same notice**, because the total is
  filtered. This matches the old app: the wrapper decides "empty" before
  either layout renders, so in both cases no group header or column shell
  appears and there is no separate filtered-empty line to add. Clearing the
  last filter brings the rows back without a reload.
- *A group with no rows while others have some* → not this state: the group
  keeps its header/column with count 0 (SPC-023/024).
The gate proves a board with zero published issues in both layouts, a board
with issues showing no notice, and the notice appearing after a live filter
change and disappearing when the filter is cleared.

**SPC-026 — Per-group paging.**
First fetch: per group, page size **50**, `group_by=state_id`,
`cursor=50:0:0` (the literal first-page cursor the old store builds:
`<per_page>:<group offset 0>:<page 0>`), `per_page=50`, ordered by
`-created_at` unless the request says otherwise. The oracle pins that exact
first cursor. If a group ever needs a fetch before its first row lands, the
old store's group fallback is `50:1:0`; the new app may keep it but never
sends it as the board's first request. Then each group pages independently:
- *List*: a manual "load more" control under the group's rows, shown only
  while that group has a next page. Pressing it fetches the next page with
  the group's own cursor; while in flight the control is replaced by pulsing
  placeholder rows; the new rows append, the count stays the server's total,
  and the control retires when the group is exhausted. Loading one group never
  touches another's rows, cursor or expanded state.
- *Board*: scrolling a column to its end (an invisible tripwire plus
  placeholder cards) loads the column's next page; there is no manual control
  on state columns. The trigger is debounced so a burst of scroll events
  fires exactly one request per page; it never duplicates cards and never
  fetches without a next page.
- Cursors are opaque and come from the response (`next_cursor`,
  `next_page_results`); they are not part of the board address.

Implementation note: the grouped response's shape and the per-group cursor
are in §5. The spaces feature keeps one query per (anchor, layout, filters,
state group) so the address and the cache agree.

### 3.5 Slice F — filters, pills and address encoding

**SPC-034 — Filter panel.**
A Filters control in the navbar opens a dropdown panel (kit `Popover`) with:
- a search box at the top that narrows the option rows below (see SPC-039);
- one collapsible section per facet the active layout supports — for both
  public layouts that is exactly **priority** and **state**;
- priority: exactly five options highest to lowest — `urgent`, `high`,
  `medium`, `low`, `none` — where `none` **is** the unset level (there is no
  sixth "unset" entry); each is a checkbox row with the priority marker and
  its name; state: one row per
  workflow state (`GET .../states/`), initially showing five with an expand
  control past that, each a checkbox row with the state marker and name;
- applied options show checked; each section header counts its applied
  values and collapses/expands its rows. There is **no per-section Clear in
  the old panel** (priority and state each offer only the count in the header
  and the view-all/less control), so the slice does not claim one: removal
  happens through the applied pills strip (SPC-035), which is the intended
  and tested removal surface. If the design later adds a per-section clear,
  it is a new-app convenience and it must behave exactly like the strip's
  facet clear (same address rewrite, same refetch);
- **no label section**: labels are never offered here (they arrive only via
  the address, SPC-037).
Rules: picking an option updates the filters, rewrites the address, and
refetches page one of every group (the panel is a dropdown; it stays open for
multi-pick and closes on outside click/Escape). The section-to-facet mapping
is driven by a per-layout table (both layouts map to
`priority` + `state`) rather than hard-wired JSX, so SPC-038 keeps holding if
a layout is added.

**SPC-035 — Applied pills strip.**
Above the layouts, one facet shell per applied facet:
- the facet's name label, one pill per value (priority pill: marker + name;
  state pill: marker + state name), a per-facet clear control;
- one global clear-all control at the end;
- the strip does not render at all when nothing is applied;
- indeterminate facets (labels, SPC-037) get only the facet label and the
  clear control.
Three removal levels — one pill, one facet, all — each of which updates the
address (surviving values stay under the **singular** keys `state`,
`priority`, `labels`, each still comma-joined; old bug NEWFRONT-464 rewrote
survivors under `states=`, which the loader never read) and refetches page one
of every group. The pill's remove control is a labelled button, keyboard
reachable.

**SPC-036 — Filters are address-encoded and reload-safe.**
`state`, `priority` and `labels` are comma-joined UUID/value lists in the
address; `board` and `peekId` travel beside them. **The initial list fetch
carries them**: opening a pasted filtered address fetches the filtered board on
the first request (old bug NEWFRONT-463 seeded pills but fetched unfiltered).
Reload, share, back/forward and sign-in round trips reproduce the same board.
Clearing all filters strips `state`, `priority`, `labels` and keeps `board` and
an open `peekId`. Values that are not valid UUIDs (for state/labels) or known
priorities are dropped rather than sent; a dropped value never renders a pill.
The filter values are also validated against what the reference endpoints
return where cheaply possible (states, labels) without blocking the first
fetch on those requests.

**SPC-037 — Address-only label filter.**
A `labels` value in the address narrows the list server-side on the first
fetch and every page (old bug NEWFRONT-463: it was never sent). The panel has
no label picker, so the only ways to remove it are the strip's facet clear and
clear-all. The strip shows the labels facet shell exactly as the facet name
plus its clear control — **no value pills, no label names** (inventory
confirmation 2). Label names are still shown on cards whenever the issue
payload carries them (SPC-028); removing a label filter does not change which
labels a card shows.

**SPC-038 — Facets follow the active layout.**
The panel's sections come from the active layout's supported-facet table.
Both public layouts support priority and state, so switching layouts shows the
same two sections; the space feature still computes them from the table.
No facet is ever offered that the layout cannot apply to its fetch.

**SPC-039 — No free-text issue search.**
There is no search over issue titles or descriptions anywhere in the
subtree (no navbar search, no palette, no command). The panel's box only
narrows filter option rows (priority names/keys and state names); it matches
case-insensitively on substrings, clears back to all options, never changes
the address, never refetches issues and never leaves the panel. A section with
no matching options shows an empty line rather than rows. Pressing Enter in
the box does nothing to the board.

### 3.6 Slice G — issue cards and read-only board behavior

**SPC-027 — Card identity.**
Every card (list row or board card) shows the public key — the project
`identifier` plus `-` plus the issue `sequence_id` — and the title truncated
to one line, with the full title in a tooltip on hover/focus. Titles and keys
come from the issue payload; no client-side padding or renumbering.
The **open region** differs by layout, and the spec, the gate and the slice
must name the same one:
- **List row:** only the **title** is the link to the issue's address (it adds
  `peekId`) and is the keyboard-focusable control that opens the overlay. The
  key is plain text in its own element beside the title, outside the link;
  the row itself and the property strip carry no activation. Activating the
  key, a property chip or empty row space does nothing.
- **Board (kanban) card:** the card body — key, title and property strip
  together — is one link to the issue's address (adds `peekId`), and that is
  the focusable control. The vote/reaction footer is a **sibling** of that
  link (SPC-030, SPC-048), so its controls never open the overlay.
That is the old app's shape and what the oracle pins (the list card's key is
read from the row sibling, the board card's link carries `peekId`). The new
app **may** widen list-row activation to the whole row as a new affordance,
but that is a labelled addition, not parity: the gate keeps pinning title
activation, and if the row is widened the key and chips must still not
navigate twice.

**SPC-028 — Card markers.**
The card's property strip shows: the state marker with its name, the priority
marker (icon; the old board card shows no priority text — the overlay's
property chip does, SPC-044; the new card may add text only if the design
does, and the oracle checks the marker, not the wording), label chips by name
(colours from the label payload), and the due-date marker as a short
month/day/year date. The priority marker renders **always**, including the
unset level, which has its own marker. Omitted when absent: labels (no
chips), due date (no marker). The start date is never shown on
cards. Cycles and modules are **not** shown anywhere on cards or groups, even
when the issue carries them (inventory confirmation 1 — no visible cycle or
module surface was found in the old board; its cycle/module fetchers are
never called by a public-board component). The new app displays exactly key, title, state, priority, labels
and due date.

**SPC-029 — Card footers.**
Board (kanban) cards carry an inline footer with the vote control (up/down
counts) and the emoji-reaction control, each gated separately by
`is_votes_enabled` / `is_reactions_enabled`. Each half renders only when its
flag is on; with both off the footer renders nothing (compact card, no empty
row). List rows never render a vote/reaction footer in any flag combination
(inventory confirmation 4). Inside a frame the reaction add control is hidden
(SPC-015; the old card footer's working add control is NEWFRONT-469) and the
vote controls stay visible but inert (SPC-015/SPC-047).

**SPC-031 — Highlight tracks the open issue.**
Exactly one card/row is highlighted and it is the one whose id equals
`peekId`: opening another issue moves the highlight; an address with
`peekId` opens with that highlight even if no click happened; closing the
overlay clears the highlight. A `peekId` that is not in the loaded groups
still opens the overlay (it is fetched by id, SPC-040) and highlights nothing
until it appears in a group after a fetch. **Parity rule:** the highlighted
card/row carries a visible emphasis (a border/ring treatment, not colour
alone). The old app changed only a border class and exposed no current-state
attribute; adding an accessible current state (`aria-current="true"` or
equivalent) is a **new-app accessibility addition**, not old behavior and not
part of the parity contract — the oracle does not pin it. The slice adds it
if the kit supports it without changing the visual outcome, and no gate
scenario may depend on it.

**SPC-032 — Read-only board.**
The board is read-only with respect to issue data, not in the sense that
it makes no writes at all. Two rules, stated separately so neither is
over-read:

*No issue-structure mutation.* No card or row is draggable (`draggable`
false/absent), there is no drag handle, no drop target, no reorder animation
and no inline editable field anywhere in the two layouts. No call that edits
an issue changes its state, priority, labels, assignee or dates, and nothing
creates, deletes, reorders, bulk-edits or moves an issue between groups.
There is no create/new-issue affordance, no bulk selection UI and no context
menu that offers edit. Attempting a drag selects text or scrolls but never
moves a card.

*Engagement writes are allowed and expected.* Vote, issue reaction,
comment, comment edit/delete and comment reaction write to the server, but
only through the controls defined in SPC-047 (votes), SPC-048 (issue
reactions), SPC-053–057 (comments and comment reactions) and only when their
publisher flag allows it. Where those controls mount is fixed: the overlay
mounts both vote and emoji controls (SPC-047/048), and board (kanban) cards
carry a footer with the same controls (SPC-029; list rows carry none).
Reactions are therefore reachable from a board card as well as from the
overlay; an anonymous attempt is a sign-in redirect, not a write.

Visitor actions are exactly: view, switch layout, filter, open an issue,
vote, react, copy a link, and — when comments are enabled — comment, edit
their own comment, delete their own comment and react to comments.

**SPC-033 — No live refresh.**
The public board does not refetch on window focus and does not open a
realtime channel. Refocusing the tab leaves the rendered rows, order and
scroll exactly as they were. Reconnecting after an offline period behaves
like refocus — **no refetch** (old bug NEWFRONT-455: reconnect refetched at
the data layer's default; the new app sets `refetchOnWindowFocus: false` and
`refetchOnReconnect: false` for every board query, and no realtime
subscription). Fresh data arrives only through an explicit action: navigating
(including a back/forward that remounts the board query), switching to a
layout that has not loaded, changing a filter, or a successful mutation
reconcile. This applies to the space queries only; it must not change the
workspace app's defaults.

### 3.7 Slice H — issue open, overlay shell, content and copy link

**SPC-030 — Opening a card.**
Activating the card's open region — the **title link** in a list row, the
**card body link** on a board card (SPC-027), by click or by Enter/Space on
the focused control — opens the issue overlay and writes `peekId` into the
address **keeping `board` and every filter parameter**. Clicking the list
row's key or a property chip, or the board card's vote/reaction footer, does
not open the overlay (SPC-027, SPC-048). The oracle pins the address contents (board + filters + peekId)
and that the board is behind the open overlay; it does not pin history
semantics. History follows the one model in SPC-022: opening pushes exactly
one entry; closing replaces it (SPC-042). The old app dropped the filter
keys from the address on a card click while the store kept filtering (so a
reload of an open issue lost the filter) — that is a defect and the new app
does not rebuild it; the position-preserving rule above is what the gate
asserts.
In list layout the open issue's row gains the active highlight. In board
layout the open card gains the same highlight.
The board card's footer (votes/reactions, SPC-047/SPC-048) is a **sibling**
of the card's link, not inside it: clicking a footer control toggles the
vote/reaction and does **not** open the overlay or write `peekId`, and the
same goes for keyboard activation of those controls. Only the link region
(the whole card body on a board card, the title on a list row) opens the
overlay. (The old app renders the footer outside the link for exactly this
reason; the list layout has no footer at all — SPC-029.)

**SPC-040 — Issue address opens the overlay.**
An address with `peekId` loads the board and, while the board data loads,
fetches that issue's detail and its comments (§5) and mounts the overlay
already open. The board behind it is the board the rest of the address
describes (layout and filters applied once they land). A `peekId` that does
not belong to the board's project, a malformed UUID, or a server 404/403
drops `peekId` from the address (replace) and leaves the board; it does not
crash, and it does not show the board-error illustration. The overlay fetches
detail and comments in parallel; either failing independently shows its own
error (§3.8/3.10) while the other renders.

**SPC-041 — Three presentations.**
One overlay component with three presentations:
- **side** (default): a half-width/fixed-width panel sliding in from the right
  (or left per the design) with a header, the detail column as its body and
  the activity section below; it keeps the board visible beside it;
- **modal**: centred over a dim, inert backdrop; closes on backdrop click and
  Escape;
- **full**: takes the content area (near-full-screen) with the detail content
  in the main column and the property panel in a side column.
A header control (segmented or menu) switches between them and keeps the same
`peekId` and the same board state; the default is side. The overlay is a
dialog in the accessibility tree with a name from the issue key/title, traps
focus while open in modal and full, restores focus to the opening card on
close, and Escape closes from every presentation. Comments/activity gating is
**uniform**: the section renders only while `is_comments_enabled` is on, in
all three presentations (old bug NEWFRONT-461: full-screen mounted it
unconditionally; inventory confirmation 3). Inside a frame, comment compose
and menus hide per SPC-015.

**SPC-042 — Closing keeps the board state.**
Close drops only `peekId` from the address and preserves `board` and every
filter key. The board underneath is exactly what it was (same rows, same
scroll, same filter state); the overlay remembers nothing between openings.
Close paths: the header close control in side view, the backdrop in modal,
Escape in every view, and the browser back gesture. Back returns to the
pre-open board because opening pushed one entry and closing replaces it
(the single history model in SPC-022); it is not a second, competing rule.
Closing by any path performs the same address rewrite. The highlight clears
(SPC-031).

**SPC-043 — Overlay content.**
Detail block order: the public key line (project identifier + sequence) and
any issue-type/eyebrow the design shows, the title (read as text, never an
editable field), the rendered description read-only (rich text through
`RichTextView` with the public asset resolver), then the property panel and
the activity section. An issue with no description (empty HTML, the API's
empty-content marker, or whitespace only) renders **no description block at
all** — no empty editor, no placeholder. Long content scrolls inside the
overlay; the header stays visible. No edit affordance exists anywhere.

**SPC-044 — Property panel.**
Shows exactly: **state** (marker + name), **priority** (marker + translated
label inside a marker chip), **due date** (formatted date; an explicit empty
marker when absent). Non-editable. A due date on or before today on an issue
whose state is not completed or cancelled is emphasised as overdue; future
dates and completed/cancelled states are never emphasised. The panel shows no
assignees, cycles, modules, estimates, links, attachments or parent (none are
in the row, none are public). In the full presentation the panel sits in a
side column; in side/modal it sits under the description or in a compact
strip, as the design decides, but the same three fields.

**SPC-045 — Copy link.**
A copy action in the overlay header, rendered only when the browser grants
clipboard write access (`navigator.clipboard.writeText` available and
permission not denied), copies the **current address** (board + filters +
peekId) and raises a success notice. Denied/unavailable → the action is not
rendered (not a disabled control). The full presentation's header shows no
copy control; the property column's second copy control of the old app was
never rendered (dead UI) and is not rebuilt. Copy failures raise an error
notice. The action is keyboard reachable and announced.

**SPC-046 — Overlay loading.**
While the detail resolves, the overlay mounts immediately with skeletons:
text-line skeletons in the detail column and property-row skeletons in the
full presentation's side column; the header shows the key and a title
skeleton, and copy is not offered until the address/page is known. No
half-rendered overlay: partial data replaces skeletons section by section,
never interleaves stale and new values for the same field. The comments
section shows its own skeleton independently of the detail (SPC-049). If the
list already had a cached row, it may seed the key/title instantly while the
detail fetch fills description, properties and engagement.

### 3.8 Slice I — issue votes and emoji reactions

**SPC-047 — Issue voting.**
Two controls (up and down), each with a live count, in the overlay and on
board-card footers (board layout only), rendered only while `is_votes_enabled`
is on.
- Hover/focus a control shows who voted that way, in order, or a short empty
  note; the visitor's own side is marked.
- Click toggles: same side again removes; the other side moves the single
  vote. One vote per visitor per issue.
- The screen updates immediately (optimistic), then writes; a failed write
  re-reads the issue's stored votes and shows them with an error notice.
- Anonymous clicks go to the spaces landing document with the subtree-relative
  current board position as its `next_path`, encoded once (§1.3, SPC-006,
  SPC-013; old bug NEWFRONT-465 truncated the position). No `/spaces` segment
  appears inside the value — the server adds the mount on redirect.
  After sign-in the visitor returns to the same issue open.
- Inside an embedding frame the controls stay visible with their counts but
  are inert: activating one does nothing (no write, no redirect), matching
  the old app's early return (SPC-015).
- Reads: the issue payload's embedded `vote_items` (the public votes **list**
  endpoint answers empty because it resolves the board by the wrong key; do
  not rely on it). Writes: `POST .../votes/` with `vote` 1|−1, `DELETE
  .../votes/`. The write path does not check the flag server-side (a direct
  post stores); the UI only offers controls when the flag is on, and the new
  app does not add client-side re-gating beyond that.
FIELD per vote: `actor`, `actor_details.{id, display_name, avatar,
first_name, last_name}`, `vote`.

**SPC-048 — Issue emoji reactions.**
Grouped pills below the issue title (overlay) and in the card footer (board):
one pill per emoji with its count, the visitor's own pills marked, plus an
add control opening a picker. Hovering a pill names the reactors in order
(truncated with an overflow note past a handful).
- Picker pick adds; clicking a pill the visitor already reacted to removes;
  clicking another pill adds theirs to that group. Optimistic then reconcile
  from a fresh issue read on failure, with a notice.
- Gated by `is_reactions_enabled`; hidden inside a frame (overlay and card).
- Anonymous attempts (pill or picker) go to sign-in with the encoded full
  position (SPC-013; NEWFRONT-465).
- Reads: the issue payload's embedded `reaction_items` (§5.1; there is no
  `reactions` key), not the list endpoint (same wrong-key lookup).
  Writes: `POST .../reactions/` and `DELETE .../reactions/:reactionCode/`
  where the code is the emoji's decimal code point(s), exactly as returned.
- Issue reaction writes are gated server-side too: while
  `is_reactions_enabled` is off both calls answer 400. The UI never offers
  them, and a refusal after a flag flip is handled as a failed write
  (notice + fresh read), not retried blindly.
FIELD per reaction: `reaction` (code), actor id/display name/avatar.

### 3.9 Slice J1 — public comments thread, gating and rows

**SPC-049 — Comments gating and invitation.**
The activity section (heading, thread, composer or invitation) renders only
while `is_comments_enabled` is on — in side, modal and full presentations
alike (confirmation 3; NEWFRONT-461). With it on:
- everyone sees the thread (anonymous included);
- signed-in visitors see the composer;
- anonymous visitors outside a frame see a sign-in invitation in place of
  the composer; inside a frame the invitation is hidden and the thread stays;
- a failed comments fetch shows an inline error with a retry that refetches
  the thread without reopening the overlay; it does not use the board-error
  illustration;
- an empty thread shows the heading and a short empty line (never a blank
  area).
The invitation's return address is the full current, subtree-relative
position (§1.3, SPC-006), encoded once (old bug NEWFRONT-465 kept only the
bare board). The server also refuses
comment writes and returns an empty list while the flag is off; the new app
does not depend on that gate (it hides the section) but must not break when it
happens (a disabled response renders as an empty thread).

**SPC-050 — Comment rows.**
Each row: avatar (uploaded image when present, else the author's initial with
a stable colour), display name, the age line — a **relative age** of the
comment's creation time ("3 hours ago" shape, from the shared `timeAgo`
helper, updating without a refetch on a coarse timer) (old bug
NEWFRONT-459 rendered no age; the target matches the workspace rows),
bot-marked authors, the formatted rich body (`RichTextView`, asset links
resolved through the public asset URL), and the reaction row (SPC-057).
Bot marking is part of the **author line itself**, not a second badge node:
when `actor_detail.is_bot` is true, the author name is rendered as the account
first name followed by " Bot" (the oracle asserts the author text ends in
" Bot"); the rest of the row shape is identical to a human row. Author and
body come from `actor_detail`, `comment_html` (fallback `comment_stripped`),
`created_at`/`updated_at`. The old space rows render **no edited marker**:
nothing under `apps/space/**` consumes `edited_at` and no space scenario
asserts one. If J1 reuses the workspace comment card, the age line's
"(edited)" suffix comes with that card — that is **reused workspace
presentation, not old space parity**, and no gate scenario may require it.
The space card, if written locally, likewise does not add one.

**SPC-051 — Folded comments.**
A comment whose `labels` include the fold marker renders collapsed: header
(name, age, the fold toggle) and nothing else, including no body and no
reaction row. Activating the toggle expands the body in place; toggling again
collapses. Expanded state is per row and local (not in the address, not
persisted). Folded state comes only from the server marker (never inferred
from the body or speaker). Selecting the toggle is keyboard operable.

**SPC-052 — Machine-authored speaker labels.**
The stored `speaker_type` is a four-value contract -- `human`, `agent`,
`system`, `integration` (the model's choices; `human` is the default). The
serializer emits whichever of those four the row holds. Only the exact value
`agent` produces a speaker label; `human`, `system`, `integration` and a
missing value render no speaker line at all. A human row whose
`speaker_label` is present is still a human row: no line. If the new app
tolerates an unknown value on read (forward compatibility), it treats it as
"not agent" (no line); that tolerance is an explicit new-app choice, not old
serializer behavior. For an agent comment whose `speaker_label` is non-empty
after trimming, the row shows that label as **its own line between the
header and the body**, ending with a colon, above the rendered comment body
(not folded into the author name and not on the header line). An agent
comment with an empty or whitespace-only label uses a generic agent label
instead, in the same own-line position. The author identity (avatar + display
name, SPC-050) is unchanged and is driven by `actor_detail`, so an agent row
still names the author it acted as.
Reuse seam: the current Issues `CommentCard` folds the label into the header
name via `commentAuthorName` and has no separate label line, and it requires
workspace scope plus workspace reaction hooks. This row is **not** satisfied
by that card as-is. The space slice either (a) gets an Issues-side change to
`CommentCard` — an optional own-line speaker slot and public-scope props for
reactions/edit/delete, with the workspace behavior unchanged — or (b) renders
its own public comment card from this spec. Both are new code; nothing is
copied from the old card. If (a), the Issues owner lands the prop change in
an Issues slice before J1 depends on it (see §6.2 and the gate note).

### 3.10 Slice J2 — composer, attachments, mentions, edit/delete, reactions

**SPC-053 — Posting rich comments with attachments.**
The composer is a rich editor (`shared/editor/RichTextEditor`, lazy) with a
toolbar (bold/italic/strike/lists/code/link), paste that keeps inline
formatting, and an image command. Rules:
- Enter submits while signed in (Shift+Enter inserts a line break); a send
  button submits too; an empty/whitespace body disables submit.
- Image command → file chooser → the three legs of §5.3 (metadata JSON post
  to `POST /api/public/assets/v2/anchor/:anchor/`, the signed S3 byte post,
  then the **required** confirm `PATCH .../:assetId/`), then insert the image
  node (src = the asset id, resolved at read time to the public anchor asset
  URL, §5.3) into the body. The comment **create payload carries no asset
  field**; after the create succeeds the app links
  the draft's asset ids to the new comment with one bulk call
  (`POST /api/public/assets/v2/anchor/:anchor/:newCommentId/bulk/`,
  `{asset_ids}`). The node shows an uploading state and flips to uploaded when
  the upload lands; submitting mid-upload is blocked until the upload settles
  (the old app allowed a post that stored a node without its file).
- Success appends the row (optimistic or refetch), clears the draft and
  keeps scroll; the new row's age and reactions are server-shaped.
- Failure raises an error notice, keeps the draft and leaves the existing
  thread untouched; nothing is stored.
- Upload failure raises a notice, removes the uploading node/draft image, and
  keeps the rest of the draft.
- Anonymous visitors never see the composer (SPC-049); inside a frame they see
  no composer either.
Create payload and fields: §5 (comments).

**SPC-054 — Member mentions.**
Typing the mention trigger (`@`) followed by at least one character opens a
member suggestion list from `GET .../members/` (names + avatars, keyboard
navigable, Enter inserts). Picking inserts a mention chip bound to the
member's id; the posted body carries the mention markup the sanitizer accepts;
the rendered comment shows the chip (name, not id). With no members or no
match the list shows an empty note (or closes) — it never offers people who
are not project members. Old bug NEWFRONT-460: no autocomplete existed at
all; the target is the working dropdown. Mentions render in posted rows and
in edits.

**SPC-055 — Owner menu.**
A comment row shows an overflow menu only when: the visitor is signed in, the
visitor's id equals the comment's author id, and the board is not embedded.
The menu has exactly Edit and Delete. Other visitors see no menu; embedded
frames show no menu at all. Delete removes the row immediately (optimistic)
and deletes server-side; a failure restores the row and raises a notice.
Delete is confirmed only if the design confirms destructive actions elsewhere;
the old app deleted immediately — keep immediate delete with a success notice
and an undo affordance only if the kit/product provides one (no new decision).

**SPC-056 — Comment edits.**
Edit switches the row's body to the rich editor with Save and Cancel; Save
swaps the content immediately (optimistic), persists with `PATCH
.../comments/:commentId/`, and on success shows a notice. No edited marker is
part of the parity rule (SPC-050: the old row does not render one); a reused
workspace card's "(edited)" suffix may appear but is not gate-provable. On
failure the row **reconciles to the stored text** and shows an error notice
(old bug NEWFRONT-467 left the phantom edit on screen) — the
save failure path refetches the thread (or rolls back the optimistic patch to
the last server value) so the view never disagrees with the server. Cancel
restores the previous content with no request. Only the owner can edit
(SPC-055).

**SPC-057 — Comment reactions.**
Each row carries a grouped-pill reaction control (counts, add control with
picker, toggle semantics, optimistic then reconcile, notices) with:
- reactor names on hover (comment payload rows are flat:
  `reaction.actor_detail` / lite `actor_detail`; read the flat shape — the
  old bug NEWFRONT-466 read the nested shape and showed nobody);
- the visitor's own pills highlighted on server-loaded rows (same fix);
- anonymous attempts → sign-in with the encoded full position (SPC-013;
  NEWFRONT-465);
- embedded frames show counts only, no add control;
- writes gated by the publisher's reactions flag (server enforces it for
  comment reactions).
Reads: `comment_reactions` on the comment row; writes: `POST`/`DELETE
.../comments/:commentId/reactions/` with the code.

### 3.11 Slice C — spaces auth card and flows (mounted by slice B)

The card lives only at `/spaces/`. It never appears on a board, and a board
never mounts it. It reads instance configuration (§5, instance row) for which
methods and providers exist, and it routes every step by the server's
`email-check` answer plus that configuration. Native submits are real HTML
forms (Enter works, browser password managers work) with the CSRF token in a
hidden field; JSON posts go through the space client.

**SPC-058 — Email-first step with inline validation.**
The first step is one labelled address field (with a clear control) and one
continue action. Rules:
- The field format-checks locally (standard address shape; trim; lower-case
  before sending).
- The inline invalid notice appears only **after** the field loses focus
  (blur) or after a submit attempt; while focused a bad value shows no
  notice. Fixing it clears the notice on the next blur.
- Continue is disabled while empty, while the local check fails, and while a
  check is in flight (button shows progress, not a duplicate request).
- Submit calls `POST /auth/spaces/email-check/` with `{email}` and creates
  nothing server-side. Server answers `{existing, status}` where `status` is
  `MAGIC_CODE` or `CREDENTIAL`; the card uses both (SPC-059).
- A landing address may carry `email` and pre-fill the field (still checked
  on submit).
- The email-check endpoint is anonymously rate limited (per minute, per
  client) and answers 429 with `{error_code: "RATE_LIMIT_EXCEEDED",
  error_message: "RATE_LIMIT_EXCEEDED"}` (server code 5900). **Parity
  behavior: no banner.** The old card's catalog has no 5900 entry, so the
  response lands in the unknown-code branch: the email step stays as it was,
  the address is preserved, no request is retried, and nothing explains the
  delay. The oracle pins this (`banner: null` for 5900), so the new app keeps
  it and SPC-063 and §5.4 list 5900 as **not modelled** (unknown branch). The
  no-explanation outcome is a known old defect, filed as **NEWFRONT-822**;
  when that issue is fixed, the banner becomes a new-app behavior with its
  own re-tagged scenario — this spec does not pre-empt it.

**SPC-059 — Known vs new routing and method selection.**
Known address → sign-in mode; new → sign-up mode. Method selection:
- *Code step* requires mail configured and magic-link login enabled
  (`is_smtp_configured && is_magic_login_enabled`); for a **known** address
  the account must also be code-created (`status === "MAGIC_CODE"` — password
  accounts always route to credentials). Taking the code step generates a
  fresh code immediately (`POST /auth/spaces/magic-generate/`).
- *Password step* requires email/password auth enabled
  (`is_email_password_enabled`).
- *Sign-up with signup disabled*: the check still routes the new address to a
  method step, and the server refuses the sign-up post with
  `SIGNUP_DISABLED`, which renders as a banner (this is an instance setting,
  not an edition; the card does not claim a pre-emptive gate it cannot see).
- *Neither method enabled*: the card must explain itself rather than mislead.
  Old bug NEWFRONT-462: sign-in showed a misleading invalid-address banner and
  sign-up dead-ended silently. The target: a dismissible banner on the email
  step stating that no sign-in method is available on this instance and to
  contact the administrator, in **both** modes, with the mode and address
  preserved.
- Clearing the locked address on a later step returns to the email step and
  keeps the current mode.
The mode is derived from the check; the visitor never picks sign-in vs
sign-up.

**SPC-060 — Code step.**
Shows the locked address with a clear control (back to email), a code field,
delivery guidance, a submit and a resend control. Rules:
- The step generates a code on entry (or the entry already generated it);
  resend re-generates and then disables itself behind a short client-side
  cooldown (a few seconds) before offering resend again. Past the server's
  per-address attempt cap the generate call reports exhaustion, which renders
  as a banner on the step (not a toast, not a silent failure).
- Submit is a native CSRF-protected form post to
  `/auth/spaces/magic-sign-in/` or `/auth/spaces/magic-sign-up/` (by mode)
  with `code`, `email`, `next_path` and `csrfmiddlewaretoken`. The token is
  fetched when the step renders; submit waits for it. A post without a token
  is refused by the server with the verification-failed page (no redirect,
  success status) — the card must never submit without the token.
- Overview of failures: wrong code → `INVALID_MAGIC_CODE_SIGN_*`; missing or
  consumed code → `EXPIRED_MAGIC_CODE_SIGN_*`; past cap →
  `EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_*`. The server redirects back to the
  spaces base with `error_code`, and the card maps it back to the code step in
  the matching mode with a banner (SPC-063).
- Codes are single-use and short-lived server-side; a valid code signs in,
  consumes the code and redirects to the validated `next_path`; replaying it
  reports expiry.
- A code-created account is marked password-unset, which is why it routes to
  the code step on the next visit.

**SPC-061 — Password step.**
Shows the locked address and one password field in sign-in mode, plus a
confirmation field in sign-up mode. Rules:
- Each field has its own visibility toggle that flips only that field.
- Sign-up: a non-empty password failing the strength rules shows a strength
  meter (a verdict plus the unmet criteria) and submit stays disabled until
  the password passes and both fields match. A filled confirmation that
  differs shows an inline mismatch notice once the confirmation is blurred or
  has grown at least as long as the password. Sign-in never shows the meter.
- Primary action: one translated label per mode, chosen by the design; the
  intent is the ordinary continue action in sign-in mode whether or not mail
  is configured (without mail the same control simply completes the
  password sign-in, and the old app's separate wording for that configuration
  is not a behavior to preserve), and an account-creating action in sign-up
  mode. The spec does not fix the label text.
- Sign-in with mail configured additionally offers a switch to the code step,
  which generates a code and moves the card there; without mail there is no
  switch.
- Submit is a native CSRF-protected form post to `/auth/spaces/sign-in/` or
  `/auth/spaces/sign-up/` with `email`, `password` (and `confirm_password` on
  sign-up), `next_path` and the token; the token is fetched at submit time.
  Wrong credentials → `AUTHENTICATION_FAILED_SIGN_IN`, no session. A fresh
  sign-up creates a password account and signs in. A token-less post is
  refused by the verification page as on the code step.
- A code-only instance (no password auth) never shows this step.

**SPC-062 — Provider buttons.**
Third-party buttons render exactly when the instance enables providers —
any of Google, GitHub, GitLab, Gitea (`is_*_enabled`). With none enabled the
card shows no buttons and no leftover divider or slot. Off the password step
each button carries a full, translated invitation naming the provider and
following the mode (a provider-continue phrase; the design owns the words);
beside the password step the same buttons render compact (icon-only, with an
accessible name). Activating a button starts the provider flow at the
**workspace provider path** `/auth/<provider>/` (`google`, `github`, `gitlab`,
`gitea`), carrying the encoded `next_path` — this is parity: the old card
navigates there (`apps/space/hooks/oauth/core.tsx`) and the green oracle
asserts `/auth/google/`. The server also exposes spaces-specific initiate
endpoints under `/auth/spaces/<provider>/`; the space card does **not** call
them, and no slice switches to them without a new decision and a re-tagged
scenario. Callback URLs are the server's own redirects. Completing a provider flow is unprovable on the
seeded stack (no credentials): the oracle proves presence, compact shape and
flow start, and the gate extends only if a provider is configured. Provider
errors that arrive as codes (`*_NOT_CONFIGURED`, `*_OAUTH_PROVIDER_ERROR`,
GitHub org membership) render through the same banner as SPC-063.

**SPC-063 — Failure banner and error links.**
Card-level failures render as a dismissible banner above the header;
dismissing removes it and keeps the current step. Banner types only — inline
step errors (malformed address, mismatch, weak password) never use the
banner. Opening `/spaces/?error_code=<code>&…` at the landing root lands on
the fixing step:
- credential failures (`AUTHENTICATION_FAILED_SIGN_IN`, `…_SIGN_UP`,
  `INVALID_PASSWORD`) → the password step in the matching mode;
- wrong/expired/exhausted code failures (`INVALID_/EXPIRED_/
  EMAIL_CODE_ATTEMPT_EXHAUSTED_MAGIC_CODE_SIGN_*` and the `MAGIC_SIGN_*_
  EMAIL_CODE_REQUIRED` variants) → the code step in the matching mode;
- account-existence failures (`USER_DOES_NOT_EXIST`, `USER_ALREADY_EXIST`) →
  the email step with the banner; the banner's wording points the visitor at
  the right mode (the other mode's step is not force-switched, matching the
  old card and the oracle);
- the six form-level validation codes the old banner set also knows --
  `REQUIRED_EMAIL_PASSWORD_SIGN_UP` (5040), `INVALID_EMAIL_SIGN_UP` (5045),
  `INVALID_EMAIL_MAGIC_SIGN_UP` (5050), `REQUIRED_EMAIL_PASSWORD_SIGN_IN`
  (5070), `INVALID_EMAIL_SIGN_IN` (5075) and `INVALID_EMAIL_MAGIC_SIGN_IN`
  (5080) -- render a banner on the step that made the call and do not switch
  step or mode (the old card only force-switches steps for the
  `AUTHENTICATION_FAILED_*`, wrong/expired/exhausted-code and
  account-existence families above). These arise from a submit (a field
  missing or malformed after the browser check), not from the code link;
  when they do arrive as `error_code`, the card keeps the current step;
- unknown numbers → no banner, email step untouched. The OAuth configuration
  codes the card knows render through the banner as described in SPC-062;
  `GITEA_NOT_CONFIGURED` (5112) is **not** among them, so a Gitea
  configuration failure renders no banner (old behavior, kept; no issue
  filed — the Gitea button only appears when the instance enables it).
- `RATE_LIMIT_EXCEEDED` (5900, the server's answer to any throttled auth
  call, including `email-check` and a throttled generate) is **not modelled**
  in parity: it falls into the unknown-code branch — no banner, the step that
  made the call stays as it was (email step with the address, or the code
  step), and no automatic retry. This is exactly what the green oracle
  asserts. The silent outcome is the old defect NEWFRONT-822; a fix adds a
  banner later, under that issue and a new scenario, and is not present
  behavior.
The banner text is written from the code's meaning and translated (one
catalog entry per known code); the code itself is never shown, and the
server's raw message/address fields are not echoed into the banner. Codes the
catalog does not know produce no banner at all (SPC-063's unknown branch).
`error_code` is consumed once: after rendering the banner, the card keeps the
step but does not re-apply the code on in-card navigation.

**SPC-064 — Headings and consent.**
The card header carries a mode-specific heading and subline (sign-in for
returning contributors, sign-up for new ones). Below the form, a consent line
names the terms of service and the privacy policy as links (targets from
configuration or build-time constants), with sign-up wording for account
creation and sign-in wording for signing in. Both links are present in both
modes.

## 4. Rows restated, confirmations resolved

### 4.1 Rows whose heading used to sit inside a range

The inventory headings of the six earlier notes sometimes covered several rows
under one number. These four are restated here as the authoritative text; the
numbered subsections above are the full behavior, this table is the one-line
contract the slices cite:

| Row | Restated capability |
| --- | --- |
| **SPC-036** | Filter state (priority, state, labels) is fully encoded in the address; a pasted address reproduces the same filtered board **on the first fetch**, and clearing filters strips the keys. |
| **SPC-041** | The issue overlay offers side, modal and full presentations through a switcher that preserves the open issue; the comments section is gated by the publisher flag in **every** presentation. |
| **SPC-042** | Closing the overlay removes `peekId` and nothing else; the filtered board underneath is exactly as it was (layout + filters survive). |
| **SPC-044** | The overlay property panel shows state, priority and due date, with overdue emphasis on incomplete issues whose due date is today or past. |

### 4.2 The five open confirmations

All five were resolved by the oracle runs (rows are `oracle green`); this is
the behavior to build.

1. **Cycles and modules visible surface.** None. The old board does **not**
   fetch cycles or modules: the page requests only states and labels, and the
   cycle/module stores exist with fetchers no public-board component calls.
   The issue payload does carry `cycle_id` and `module_ids`, but no card,
   group or overlay renders them, even when an issue belongs to both — the
   public board's display-property set is key, state, labels, priority and
   due only. The new app behaves the same: displays key, title, state,
   priority, labels and due date only (SPC-028), treats the ids in the
   payload as unused data, and issues **no** cycles or modules request.
2. **Label pill from a URL-supplied label.** The strip renders the labels
   facet as its name plus the facet-level clear control, with **zero value
   pills and no label names** (SPC-037). Labels still appear on cards from
   the issue payload.
3. **Activity section gating.** Uniform: the comments section renders only
   while `is_comments_enabled` is on, in side, modal and full (SPC-041,
   SPC-049). The old full-screen mounting it unconditionally is NEWFRONT-461
   and is not rebuilt.
4. **List-card footers.** List rows never render a vote/reaction footer in any
   flag combination; only board (kanban) cards do (SPC-029). This is
   intended and matches these rows.
5. **`next_path` validator.** The landing gate uses the same validator as the
   navbar sign-in link, the sign-out form, vote/reaction/comment sign-in
   redirects and the auth forms' hidden field: single leading slash, no
   backslashes, no active-content patterns, relative and same-origin, with the
   query preserved (SPC-006). No second, weaker checker for the landing.

## 5. API — endpoints, parameters, fields

Base: the space subtree's client talks to the app origin; paths below are
relative to it. `:anchor`, `:id` (issue), `:commentId` are UUIDs except the
anchor. Public endpoints are `AllowAny`; authenticated ones answer 401 for an
anonymous visitor. "Reads" lists the fields the UI displays or branches on;
the payloads carry more.

### 5.1 Board

| Call | Method + path | Parameters | Response fields read | Rows |
| --- | --- | --- | --- | --- |
| Board settings | `GET /api/public/anchor/:anchor/settings/` | — | `anchor`, `entity_name` (= `project`), `is_comments_enabled`, `is_reactions_enabled`, `is_votes_enabled`, `view_props.{list,kanban}`, `project_details.{id, identifier, name, logo_props, cover_image, description}`, `workspace_detail.{slug}`. `is_activity_enabled` rides in the payload but is **not read** by this area: it is an unread passthrough (§5.5), and every activity/comment gate is `is_comments_enabled` | 001, 003, 004, 010, 020, 029, 041, 047, 048, 049, 057 |
| Legacy resolve | `GET /api/public/workspaces/:slug/projects/:id/anchor/` | path `slug`, `project_id` | `anchor`, `entity_name` | 002 |
| Board metadata | `GET /api/public/anchor/:anchor/meta/` | — | `name`, `description` (only; the payload also carries `id`, `identifier`, `cover_image`, `icon_prop`, `emoji`, which the UI reads but deliberately does not display — `cover_image` never becomes an image tag) | 007 |
| Issues (grouped) | `GET /api/public/anchor/:anchor/issues/` | `group_by=state_id`, `sub_group_by` (unused), `per_page` (50), `cursor` (first request is the literal `50:0:0`; later pages send the group's opaque `next_cursor` from the response; the group fallback `50:1:0` is not a first request), `order_by=-created_at`, filters `state`, `priority`, `labels` (comma-joined), unknown filter keys ignored | envelope: `grouped_by`, `total_count`, `next_cursor`, `next_page_results`, `results` keyed by state UUID → `{results: [...], total_results}`; each issue is a flat dict with exactly `id`, `name`, `state_id`, `sort_order`, `estimate_point`, `priority`, `start_date`, `target_date`, `sequence_id`, `project_id`, `parent_id`, `cycle_id`, `created_by`, `state__group`, `label_ids`, `assignee_ids`, `module_ids`, `vote_items`, `reaction_items` — the card reads `id`, `name`, `state_id`, `priority`, `target_date`, `label_ids`, `sequence_id` and the embedded `vote_items`/`reaction_items`; there is **no** nested `state`, `project`, `votes` or `reactions` key | 023, 024, 026, 027, 028, 033, 036, 037 |
| Issue detail | `GET /api/public/anchor/:anchor/issues/:id/` | — | the same flat `.values()` dict as the list rows, plus `description_json`, `description_html`, `description_stripped`, `description_binary`: `id`, `name`, `state_id`, `sort_order`, `estimate_point`, `priority`, `start_date`, `target_date`, `sequence_id`, `project_id`, `parent_id`, `cycle_id`, `created_by`, `state__group`, `label_ids`, `assignee_ids`, `module_ids`, `vote_items`, `reaction_items`; each vote item is `{vote, actor_details.{id, display_name, first_name, last_name, avatar, avatar_url}}`, each reaction item is `{reaction, actor_details.{…same…}}`. There is no nested `state`, `project`, `votes` or `reactions` and no `votes` key at all — the old app reads `vote_items`/`reaction_items` and so does the new app | 030, 040, 043, 044, 047, 048 |
| States | `GET /api/public/anchor/:anchor/states/` | — | `id`, `name`, `color`, `group`, `sequence` | 023, 024, 034, 044 |
| Labels | `GET /api/public/anchor/:anchor/labels/` | — | `id`, `name`, `color` | 028, 034 (panel offers none), 037 |
| Members | `GET /api/public/anchor/:anchor/members/` | — | `id`, `member`, `member__display_name`, `member__avatar` | 054 |
| Cycles | `GET /api/public/anchor/:anchor/cycles/` | — | **not fetched** (no visible surface, confirmation 1) | — |
| Modules | `GET /api/public/anchor/:anchor/modules/` | — | **not fetched** | — |

Notes:
- `group_by=state_id` reports one entry per workflow state with
  `total_results` (the header/column count). The server's total query
  excludes declined-triage rows that the rows query includes
  (NEWFRONT-454), so a group's `total_results` can be lower than its rendered
  rows; the header renders the server number. Intended behavior is count =
  rendered rows; the new app renders the server number (it cannot compute the
  true total without a second query) and the bug stays with its issue.
- The public votes list (`GET .../votes/`) and reactions list
  (`GET .../reactions/`) resolve the board by the wrong key and answer empty;
  the app reads the embedded arrays instead (NEWFRONT-465 notes; §7).
- The detail endpoint's payload is a flat object with the keys above; it is
  the same `.values()` tail as a list row plus the four description fields.
  Neither payload nests serializer objects; the contracts describe the flat
  rows (the detail row extends the list row with the description fields).

### 5.2 Engagement

| Call | Method + path | Body/params | Response fields read | Rows |
| --- | --- | --- | --- | --- |
| Vote set | `POST /api/public/anchor/:anchor/issues/:id/votes/` | `{vote: 1 \| -1}` | stored vote (actor, vote) | 029, 047 |
| Vote remove | `DELETE /api/public/anchor/:anchor/issues/:id/votes/` | — | 204 | 047 |
| Issue reaction add | `POST /api/public/anchor/:anchor/issues/:id/reactions/` | `{reaction: "<code>"}` | created reaction | 029, 048 |
| Issue reaction remove | `DELETE /api/public/anchor/:anchor/issues/:id/reactions/:reactionCode/` | — | 204 | 048 |
| Comment reaction add | `POST /api/public/anchor/:anchor/comments/:commentId/reactions/` | `{reaction: "<code>"}` | created reaction | 057 |
| Comment reaction remove | `DELETE /api/public/anchor/:anchor/comments/:commentId/reactions/:reactionCode/` | — | 204 | 057 |
| Comment reactions list | `GET /api/public/anchor/:anchor/comments/:commentId/reactions/` | — | not needed (read from the comment row) | — |

Gating: **vote** writes do not check `is_votes_enabled` server-side (a direct
post stores); **issue reaction** add and remove *do* check
`is_reactions_enabled` and answer 400 while it is off; **comment reaction**
writes check it too. Comments writes refuse while `is_comments_enabled` is
off. The UI gates every control by the flag, so the server refusals only
appear if the flag flips mid-session; the new app treats such a refusal as a
failed write (notice + refetch), not as a silent success.

### 5.3 Comments

| Call | Method + path | Body/params | Response fields read | Rows |
| --- | --- | --- | --- | --- |
| Comments list | `GET /api/public/anchor/:anchor/issues/:id/comments/` | — (server filters `access=EXTERNAL`, order `created_at`) | `id`, `actor`, `actor_detail.{id, display_name, avatar, avatar_url, is_bot}`, `comment_html`, `comment_stripped`, `created_at`, `updated_at`, `labels` (fold marker), `speaker_type`, `speaker_label`, `comment_reactions[]` (`reaction`, `actor_detail`). `edited_at` is an unread passthrough (SPC-050: no space row renders an edited marker) | 049–052, 057 |
| Comment create | `POST /api/public/anchor/:anchor/issues/:id/comments/` | `{comment_html, comment_json?, access: "EXTERNAL"}` (no asset field: the body carries the image nodes) | created row (`id` needed for the link step) | 053, 054 |
| Comment edit | `PATCH /api/public/anchor/:anchor/issues/:id/comments/:commentId/` | `{comment_html, comment_json?}` (owner only) | updated row | 055, 056 |
| Comment delete | `DELETE /api/public/anchor/:anchor/issues/:id/comments/:commentId/` | — (owner only) | 204 | 055 |
| Asset metadata + presign | `POST /api/public/assets/v2/anchor/:anchor/` | **JSON**, not multipart: `{name, type (MIME), size (bytes), entity_type: "COMMENT_DESCRIPTION", entity_identifier}` (`entity_identifier` is the comment id for an edit and empty for a new comment; the server stores it as the asset's `comment_id`) | `asset_id`, `asset_url`, `upload_data` (the S3 form-post object: `url` + `fields`) | 053 |
| Asset read | `GET /api/public/assets/v2/anchor/:anchor/:assetId/` | — | 302 to a signed storage URL; 404 until confirmed | 043, 050, 053 |
| Asset bytes | direct form post to `upload_data.url` with `upload_data.fields` (S3, no app auth) | the form fields from the presign plus the `file` part | none (status only) | 053 |
| Asset confirm (single, **required**) | `PATCH /api/public/assets/v2/anchor/:anchor/:assetId/` | `{attributes?}` (optional; omit to keep server attributes) | 204. Sets `is_uploaded = true`; until this call the asset is invisible (`GET .../:assetId/` 404s and attachments do not resolve), so no image works without it | 053 |
| Asset bulk link | `POST /api/public/assets/v2/anchor/:anchor/:entityId/bulk/` | `{asset_ids: string[]}` where `:entityId` is the new comment's id | 204; the assets become comment attachments (`comment_id` set) | 053 |
| Asset restore | `POST /api/public/assets/v2/anchor/:anchor/restore/:assetId/` | — | editor lifecycle | 053 (editor lifecycle only) |

Upload validation, checked **before** any network call, per file:
- allowed MIME types are exactly `image/jpeg`, `image/jpg`, `image/png`,
  `image/webp`, `image/gif` (the editor's image extension ships this list);
  the file input's `accept` uses it too, but a dropped/renamed file is still
  checked by type;
- maximum size is **5 MB** (`MAX_FILE_SIZE`, the editor's file-handler
  `validation.maxFileSize`); a larger file is rejected client-side;
- a rejected file leaves the draft untouched, inserts no node, fires no
  request, and raises a notice naming the rule that failed (type or size) —
  the notice is written from this spec, translated, and never the old app's
  wording.
If a file passes the client check but the server refuses the metadata POST,
the response is a 400 with `{error: <message>, status: false}`; the composer
treats it exactly like a pre-check rejection (notice, no node, draft kept).
The server applies the same type list plus the entity-type allowlist
(`entity_type` must be a known `FileAsset.EntityTypeContext` value, so the
composer sends `COMMENT_DESCRIPTION`). There is no separate size error from
the server for a DMZ-upload flow; the storage service enforces its own limit
on the signed post, and a failed byte post is the upload-failure branch.

Attachment flow, per file, in this order: (1) `POST .../anchor/:anchor/` with
the file's metadata and `entity_type` for comment descriptions, receiving
`asset_id`/`asset_url`/`upload_data`; (2) form-post the bytes to the S3 URL
with the returned fields; (3) `PATCH .../:assetId/` to confirm — this step is
mandatory, not optional bookkeeping; (4) keep `asset_id` in the draft and
insert the image node in the editor; (5) post the comment body; (6) on
success, link the draft assets to the new comment with one bulk call carrying
the comment id and all draft asset ids. Submitting waits for every file to
reach step 3 (the old app did not, and stored a node whose asset stayed
unconfirmed and therefore unresolvable). A file that fails at any step is
reported as a notice, stays out of the body, and the post can proceed without
it. The bulk call only links assets of the comment-description entity type.
Upload cancel (`DELETE .../:assetId/`) is not exposed because the composer
has no cancel-with-upload path; if the design adds one, it uses that call.

Image source, as the old client actually works: the composer's upload handler
returns the **`asset_id`**, so the image node's `src` is the bare id; the read
resolver maps a non-absolute src to `API origin + /api/public/assets/v2/anchor/:anchor/:assetId/`
and passes an absolute URL through unchanged (bodies written by other clients
may embed workspace-shaped URLs such as
`/api/assets/v2/workspaces/:slug/projects/:projectId/:assetId/`, and those
must not be rewritten). The response's `asset_url` is not what the space
composer stores. `GET .../anchor/:anchor/:assetId/` is public for published
boards and redirects to a signed storage URL; it answers 404 until the
confirm PATCH has run. The new app gives `RichTextView` and the editor the
same resolver (a space-owned function, not the workspace one), and the
create/confirm/link calls above stay exactly as described.

### 5.4 Spaces auth and instance

| Call | Method + path | Body/params | Response fields read | Rows |
| --- | --- | --- | --- | --- |
| Instance info | `GET /api/instances/` | — | `config.{is_smtp_configured, is_magic_login_enabled, is_email_password_enabled, is_google_enabled, is_github_enabled, is_gitlab_enabled, is_gitea_enabled}`; `enable_signup` is carried in the payload but is **not read** by the card (the server enforces it, SPC-059) | 005, 016, 018, 059, 062 |
| Current user | `GET /api/users/me/` | — | `id`, `display_name`, `first_name`, `last_name`, `email`, `avatar_url` | 005, 013, 014, 049, 055 |
| CSRF token | `GET /auth/get-csrf-token/` | — | `csrf_token` | 014, 060, 061 |
| Email check | `POST /auth/spaces/email-check/` | JSON `{email}` (lower-case, trimmed) | `existing` (bool), `status` (`MAGIC_CODE` \| `CREDENTIAL`); errors `{error_code, error_message}` | 058, 059 |
| Magic generate | `POST /auth/spaces/magic-generate/` | JSON `{email}` | `key`; errors (exhausted, SMTP, instance) | 059, 060 |
| Code submit | native `POST /auth/spaces/magic-sign-in/` \| `.../magic-sign-up/` | form: `csrfmiddlewaretoken`, `code`, `email`, `next_path` | redirect to validated return path, or to the spaces base with `error_code` | 060 |
| Password submit | native `POST /auth/spaces/sign-in/` \| `.../sign-up/` | form: `csrfmiddlewaretoken`, `email`, `password` (+ confirmation), `next_path` | redirect as above | 061 |
| Session end (sign-out) | native `POST /auth/spaces/sign-out/` | form: `csrfmiddlewaretoken`, `next_path` | redirect to return path; session destroyed | 014 |
| Providers | `GET /auth/{google,github,gitlab,gitea}/` (the workspace flow start the old space card uses; server callbacks at `.../callback/`). The spaces-specific `GET /auth/spaces/{provider}/` endpoints exist but are **not called** by this area (parity: oracle asserts `/auth/google/`) | `next_path` | redirect to provider | 062 |

Auth error codes the card models (from the server's catalog; the card maps
the number to a step and a translated banner, never shows the number):
`INVALID_EMAIL`, `EMAIL_REQUIRED`, `SIGNUP_DISABLED`,
`AUTHENTICATION_FAILED_SIGN_IN`, `AUTHENTICATION_FAILED_SIGN_UP`,
`INVALID_PASSWORD`, `MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED`,
`MAGIC_SIGN_UP_EMAIL_CODE_REQUIRED`, `INVALID_MAGIC_CODE_SIGN_IN/UP`,
`EXPIRED_MAGIC_CODE_SIGN_IN/UP`, `EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_IN/UP`,
`REQUIRED_EMAIL_PASSWORD_SIGN_UP` (5040), `INVALID_EMAIL_SIGN_UP` (5045),
`INVALID_EMAIL_MAGIC_SIGN_UP` (5050),
`REQUIRED_EMAIL_PASSWORD_SIGN_IN` (5070), `INVALID_EMAIL_SIGN_IN` (5075),
`INVALID_EMAIL_MAGIC_SIGN_IN` (5080), `USER_DOES_NOT_EXIST`,
`USER_ALREADY_EXIST`, `INSTANCE_NOT_CONFIGURED`,
`SMTP_NOT_CONFIGURED`, `USER_ACCOUNT_DEACTIVATED` (5019),
`OAUTH_NOT_CONFIGURED`, and the `GOOGLE_`/`GITHUB_`/`GITLAB_` configuration
and provider-error codes. Deliberate exclusions, both proven by the old card
and the oracle:
- `RATE_LIMIT_EXCEEDED` (5900) is **not modelled**; it takes the unknown-code
  branch (no banner). The defect is NEWFRONT-822; the catalog gains it only
  when that issue is implemented with a new scenario.
- `GITEA_NOT_CONFIGURED` (5112) exists on the server but the old banner set
  lists only `OAUTH_NOT_CONFIGURED` plus the Google/GitHub/GitLab codes, so a
  Gitea configuration failure renders **no banner**. The new app keeps that;
  a slice must not add a Gitea banner.
Unknown numbers: no banner. The catalog is the one list: SPC-058 and SPC-063
read from it, with these exclusions stated there too.

### 5.5 Response shapes the contracts must declare

NEWFRONT-752 (contracts) owns the zod modules; these are the shapes the spec
requires them to model:

- `BoardSettings`: the settings object above, with `view_props` a loose
  boolean record and `project_details` nullable (the placeholder case).
  `is_activity_enabled` is declared as an optional passthrough only; no space
  component reads it, and no gate scenario derives behavior from it.
- `BoardMeta`: `{name?, description?, cover_image?}` — all optional.
- `BoardIssuePage`: the grouped envelope with `results` a record keyed by
  state id and `{results, total_results}` values, plus the top-level
  `next_cursor`, `next_page_results`, `total_count`, `grouped_by`.
- `BoardIssue` (list item) and `BoardIssueDetail` (detail payload): the detail
  row is `BoardIssue` plus the four description fields. Both are flat:
  `id`, `name`, `state_id`, `priority`, `target_date`, `label_ids`,
  `sequence_id`, `project_id`, `state__group`, `vote_items`, `reaction_items`
  (plus the other keys listed in §5.1). There is no `state`, `project`,
  `votes` or `reactions` key on either; `vote_items` and `reaction_items` are
  the engagement arrays.
- `PublicComment`: the comment row incl. `actor_detail`, `labels`,
  `speaker_type`, `speaker_label`, `comment_reactions`, plus `edited_at` as an
  optional passthrough that no space component reads (SPC-050).
- `PublicReaction` / `PublicVote`: the flat actor shapes (`actor_details` on
  `vote_items` and `reaction_items`, §5.1; `actor_detail` on a list comment's
  `comment_reactions`, and the flat `reaction.actor_detail` the comment-row
  reactions carry — NEWFRONT-466).
- `SpaceEmailCheck`: `{existing, status}`; `SpaceAuthError`:
  `{error_code, error_message}`.
- `Instance` config and `CurrentUser` may reuse the existing modules, but the
  space-specific fields (`is_magic_login_enabled`, `is_email_password_enabled`,
  provider flags) must be present. `enable_signup` is declared as an optional
  passthrough field of the payload; no space component reads it (§1.4).

Each contract gets a contract test against a real response from the Django
test server, per the Data layer page.

### 5.6 Not used by this area

The intake endpoints (`/api/public/anchor/:anchor/intakes/...`) and the
workspace project-boards endpoint exist under the same `apps/api` space
module but have no public-board surface. This area does not call them and adds
no rows for them; if a future row needs them, it is a new row, not a silent
addition.

## 6. F-06 reuse map

"Already there" means the component is merged on `web-new-dev` at the time
this spec was written (2026-10-19). Everything under "add" is **new code
written from this spec**, not adapted from `apps/space`.

### 6.1 What `apps/web_new` already has

- **Kit** (`@pidash/kit`): `Button`, `IconButton`, `Input`, `Textarea`,
  `Switch`, `Avatar`, `Badge`, `Menu`, `Popover`, `Tooltip`, `Dialog`,
  `Skeleton`, `Spinner`, `ToastHost` + `createKitToastManager`/`showToast`,
  `VirtualList`, `Markdown`/`MarkdownSpans`, `EmptyState` (+
  `emptyStateImagePath`), `Combobox`, `ContextMenu`, `Breadcrumbs`, theme
  helpers (`resolveTheme`, `applyTheme`, `watchSystemTheme`), tokens.
- **shared/shell**: `document.tsx` (`useDocumentHead`, overlay portal),
  `toaster.ts` (`notifySuccess/notifyError/notifyInfo`), `clipboard.ts`
  (page-URL copy), `NotFoundPage`, `PeekPanel` (overlay shell),
  `UserMenu` (workspace-shaped), `AppShell` (workspace-shaped),
  `CopyWorkItemLinkButton` (workspace-shaped), `externalLinks`, `narrowViewport`.
- **Issues feature** (public barrel `features/issues/index.ts`):
  `LayoutIcon`, layout kinds, `ListLayout`/`ListRow`/`GroupHeader` (workspace
  bound), `ReactionBar`/`ReactionPicker`/`EmojiPicker` + reaction model,
  `CommentCard`/`CommentEditForm` + comment model, comment/reaction **hooks
  and API** (workspace scoped), `RichTextView` (shared/editor too),
  `RichTextEditor` (lazy), mention extension, `Skeleton` loaders,
  `useNarrowViewport` consumers, toast notices constants.
- **core**: `api/client.ts` (`createClient`, singleton), `session` (workspace
  gate — not used), `theme`, `i18n`/`LanguageProvider`, `query`, `platform`,
  `edition`.
- **`@pidash/api-client`**: client, middleware, `auth.ts` (`InstanceConfig`,
  `getInstanceConfig`), `instance.ts`, `comments`/`reactions`/`issues`
  contracts for **workspace** endpoints, `ProjectDeployBoard` contract for the
  admin publish flow, and `contracts/comment-assets.ts` — a three-leg
  comment-image upload (`requestCommentImageUpload` → signed S3 byte post →
  `completeCommentImageUpload`, then `attachCommentImages`) against
  `/api/assets/v2/workspaces/:slug/projects/:projectId/...`. That flow is the
  same three legs §5.3 describes, under **workspace** paths. Space does not
  call it: the public visitor has no workspace slug it may trust. The space
  slice adds a parallel public-anchor contract (same three legs, paths under
  `/api/public/assets/v2/anchor/:anchor/`) inside its own contract module,
  and leaves the workspace module and its tests untouched.

### 6.2 Per slice — add or rework

| Slice | Borrows (read-only) | Must add / rework |
| --- | --- | --- |
| **A** shell, routing, failures | kit primitives; `useDocumentHead`; `NotFoundPage`; `notify*` | `features/spaces` feature skeleton; `/spaces/*` route tree with lazy boundaries; space query client + public API client instance; space `errorComponent`/`notFoundComponent`; settings/instance queries; the four loading stages of SPC-016 (full-viewport spinner for instance boot and legacy resolution, placeholder navbar with empty content while settings load, centered spinner under the navbar for the first issues fetch); SPC-003/004/018/019 illustrations (kit `EmptyState` art + kit illustrations, not old assets); board-error and anchor-not-found states; generic-not-found wiring |
| **B** landing + metadata | kit form primitives; `useDocumentHead`; `notify*` | `/spaces/` route; current-user + instance queries; auth gate; signed-in guidance view; metadata function (title/description/OG/Twitter, no cover); robots claim at the subtree root + host rule; return-path plumbing into the card |
| **C** auth card | kit `Input`/`Button`/`Avatar`/banner pattern; i18n | The whole card: email step, method routing, code step (native form + CSRF), password step (strength + toggles), provider buttons, error catalog → step mapping, dismissible banner, headings/consent; spaces-auth contract module in `@pidash/api-client` (**NEWFRONT-752** owns the contracts; C consumes them) |
| **D** navbar/theme/account/notices | `LayoutIcon`; kit `Menu`/`Popover`/`Tooltip`/`Avatar`; `notify*`; `ToastHost`; kit theme helpers | Space navbar (identity + fallbacks), layout switcher (own), filter trigger slot (slice F), theme toggle with space storage key and flip semantics, account popover, iframe hook, progress bar, space toast host |
| **E** layouts/paging | `LayoutIcon`, layout kinds; kit `Tooltip`/`Skeleton` | List layout (groups, headers with counts, collapse), kanban columns strip, empty board, per-group cursors and paging (manual list control, column tripwire + debounce), paging queries |
| **F** filters/pills/address | kit `Popover`/`Combobox`/`Tooltip`; Issues `LayoutPicker` helpers **not** reused | Filter panel (two facets, search, expand), applied strip with three removal levels, search-schema validation, fetch parameter builder, address writer |
| **G** cards/read-only | kit `Tooltip`; `timeAgo` (shared/lib); `LayoutIcon` | Public card/list row (key, title, markers), board card footer wiring (slice I), highlight, read-only guarantees, no-focus/no-reconnect query defaults |
| **H** overlay/copy | `PeekPanel` **read-only title variant** (requires the shell change noted in §2; until then H cannot mount the editable field), `RichTextView`, kit `Dialog`/`Skeleton`; Issues detail **not** reused | Three presentations wrapper, read-only detail body, property panel (state/priority/due + overdue), copy-link action, skeletons, close/address rules |
| **I** votes/reactions | Issues `ReactionBar`/`ReactionPicker`/`EmojiPicker` + model helpers; kit | Public vote control (overlay + card footer), public issue-reaction wiring (space queries + public endpoints), sign-in redirect with encoded position, frame gating |
| **J1** comments thread | fold helpers, bot mark, `RichTextView`, `timeAgo`; `CommentCard` **only after** the Issues own-line-speaker/public-scope change (§2), otherwise space's own card | Public comments query; gating/invitation; thread layout; fold; comments error/retry state (no edited marker: not old space behavior, SPC-050); the own-line agent speaker label (SPC-052) either through the changed card or in the space card |
| **J2** composer | `RichTextEditor` (lazy), mention extension, `RichTextView`; kit | Public composer (Enter submit, upload → asset ids, mid-upload guard), member mention source from the public members endpoint, owner menu, edit reconcile, comment reactions against public endpoints, notices |

Cross-cutting rule for the slices: when an Issues component needs a prop it
does not have (for example a viewer-id override or a "public" reaction write
callback), add the prop to the **Issues** component in the Issues owner's
slice or through a follow-up under the Issues epic — do not fork it into
`features/spaces` and do not shadow it with a near-copy. If the component
cannot be made public-safe without changing workspace behavior, write the
spaces equivalent from this spec and note it on the Issues issue.

## 7. Bug ledger — build the intent, keep the bug filed

Every row below was observed as a `bug:` scenario on the old app (the oracle
pins the old behavior). The new app implements the intent in this column.

| Bug | Row(s) | Intent to build |
| --- | --- | --- |
| NEWFRONT-453 | SPC-007 | Resolve the API base like every other call; serve board name/summary with a generic fallback and **no cover image**. |
| NEWFRONT-454 | SPC-023, SPC-024 | Render the server's group total (it can be short of the rows). Intended: count = rendered rows. The new app cannot compute the true total without a second query, so it renders the server number and the bug stays open for the backend. |
| NEWFRONT-455 | SPC-033 | No refetch on focus **or** reconnect; `refetchOnReconnect: false`. |
| NEWFRONT-456 | SPC-011 | The first toggle from the OS default flips to the opposite of what is showing and stores that. |
| NEWFRONT-459 | SPC-050 | A real relative age on every comment row, refreshed without a fetch. |
| NEWFRONT-460 | SPC-054 | A member autocomplete on the mention trigger, inserting chips. |
| NEWFRONT-461 | SPC-041, SPC-049 | The activity section is gated by `is_comments_enabled` in every presentation. |
| NEWFRONT-462 | SPC-059 | With no method enabled, both modes explain themselves in a dismissible banner and keep the address/mode; no misleading invalid-address error, no silent dead end. |
| NEWFRONT-463 | SPC-036, SPC-037 | The initial fetch carries the address filters (state, priority, labels); no seed-without-fetch. |
| NEWFRONT-464 | SPC-035 | Removing one value keeps survivors under the singular keys (`state`, `priority`, `labels`). |
| NEWFRONT-465 | SPC-013, SPC-014, SPC-047, SPC-048, SPC-049, SPC-057 | Every sign-in/sign-out round trip carries the **full** position (board, filters, open issue), encoded exactly once. The value stays subtree-relative (`/issues/<anchor>?…`, no `/spaces` segment) — one encoded value, not a mount-prefixed one (§1.3). |
| NEWFRONT-466 | SPC-057 | Comment reaction pills name their reactors and mark the visitor's own from server-loaded rows (read the flat actor shape). |
| NEWFRONT-467 | SPC-056 | A failed edit reconciles the row to the stored text. |
| NEWFRONT-469 | SPC-015, SPC-029 | The frame gate hides the card-footer reaction add control too; the old card kept a working add control while the overlay hid it. |
| NEWFRONT-822 | SPC-058, SPC-063 | **Parity: no banner.** The old card has no 5900 entry, so a throttled auth call lands in the unknown-code branch (step unchanged, address kept, no retry, no explanation) and the green oracle pins `banner: null`. The new app builds that parity behavior, so the row is flippable now; the filed issue tracks the *defect* (a visitor gets no explanation) and, if it is implemented, adds a translated try-again-later banner on the step that made the call, with a new intent scenario. Until then the catalog keeps 5900 unmodelled (§5.4). |
| NEWFRONT-823 | SPC-014 | **Intent: first + last name in the trigger fallback.** The old trigger template repeats `first_name` (`display_name \|\| first_name first_name \|\| email \|\| neutral`), so a user without a display name reads their first name twice and the email/neutral fallbacks are unreachable. The new app builds display name → first + last → email → neutral, from the same current-user fields; the oracle reads the display-name case only. |
| gate-only, no issue (addendum to NEWFRONT-463) | SPC-030/042 | Clicking a card from a filtered board keeps the filter keys in the address; the old app dropped them while the store kept filtering. Folded into SPC-030/042 and proven by the gate; NEWFRONT-463 covers the same address-loss family for the initial fetch. No separate issue: the gate scenario is the evidence and no user-visible divergence survives the intent. |
| gate-only, no issue | SPC-022/030/042 | The one history model (settle replaces, open pushes, close replaces) is a new-app choice; the old app pushed on both layout settle and overlay open. It is folded into SPC-022 and pinned by the gate's address/history scenario, so no issue is filed — a Back that lands on a self-correcting malformed address is not a user-visible divergence once the address is already settled. |
| gate-only, no issue (addendum to NEWFRONT-454) | SPC-020 | Settle only on layouts the board can render (list/kanban), instead of accepting any `view_props` flag the server says is enabled. The intent is folded into SPC-020; no issue because the deviation is only reachable in a configuration the publisher UI never produces. |
| gate-only, no issue (addendum to NEWFRONT-467/459) | SPC-017/053 | A post whose image is still uploading is blocked until the upload settles; the old app stored a node without its file. Intent lives in SPC-053; no separate issue — the gate pins it and the upload-guard unit test covers the branch. |

No row is left with an unowned old bug: NEWFRONT-469 carries the frame
footer, NEWFRONT-822 carries the silent rate limit (built as parity, i.e.
silent, until it is fixed), NEWFRONT-823 carries the duplicated first-name
fallback in the account trigger, and the four gate-only items are either addenda
to filed bugs (463, 454, 467/459) or explicit new-app choices (the history
model) proven by the gate's intent scenarios. If a slice finds a
user-visible divergence in a configuration the gate cannot produce, it files
an issue and links it here rather than widening a gate-only note.

## 8. Proof plan and gate

### 8.1 What each slice proves on its own

- Unit tests cover every changed line and branch (the binding run rule), using
  the space query modules with a stubbed transport: address parsing and
  rewriting (including the replace/push/replace history model of SPC-022),
  filter parameter building, paging cursors, gating flags, the frame hook,
  the path validator, the auth step reducer and error catalog (3900/5900
  both in the unknown-code branch), the upload pre-checks (type, 5 MB) and
  upload guard, the edit reconcile and the copy permission.
- Local checks: `pnpm --filter web_new typecheck|lint|test`, plus
  `check:boundaries`, `check:headers`, `check:deps`, `check:coexistence`, the
  similarity check and the paths gate. Slices append `built` to their rows.
- No slice brings up the parity stack.

### 8.2 What the area gate (NEWFRONT-763) proves

- Brings up the parity stack and runs the whole `apps/web_new/e2e/parity/space/`
  suite against `apps/web_new` for both editions (the same subtree served under
  OSS and cloud), with the configuration variants below.
- Turns rows whose scenarios pass into `new green`; files fix sub-issues for
  failures and waits on them.
- Appends `/spaces` to `apps/web_new/migrated-routes.json` only when every
  row is green, and regenerates the coexistence config.
- Every SPC row must be green: 001–065, no exemptions. A `bug:` scenario is
  oracle-only evidence (it pins the old app's defect); the new app passes
  through the intent described in §7. Four rows currently have **only** a
  `bug:` scenario (SPC-007, SPC-036, SPC-037, SPC-054) and SPC-059's
  no-method defect (NEWFRONT-462) is the same shape. Before those rows can be
  flipped the gate needs their intent scenarios — one per row, asserting the
  new app's behavior, tagged with the same row id and titled without `bug:`.
  Writing those scenarios is part of the gate's fix list (a small scenario
  issue), never a silently skipped row and never a re-tag of the old ones.
- Rows whose parity is the old *behavior* stay flippable without an intent
  scenario: SPC-058/063 keep the unknown-code branch for 5900 (no banner,
  oracle green) and the defect is tracked by NEWFRONT-822, which would add a
  new scenario when it is implemented; SPC-016 keeps the placeholder navbar
  in the settings stage (oracle green) rather than a full-viewport spinner.
- Configuration variants for the config-driven rows:
  - mail + magic login **on** and **off** (SPC-059/060/061);
  - email/password enabled and disabled (SPC-059/061);
  - each provider flag on/off as available (SPC-062);
  - locale: English only on OSS; cloud pack if present (SPC-065).
- Static checks on the gate's own run: the subtree imports nothing from the
  workspace shell, the space entry chunk does not include the shell, and
  `pnpm --filter web_new size` stays within the ceiling for the new chunk
  (lazy subtree).

### 8.3 Explicit non-goals for the gate

- No desktop rows (none exist for this area; `/spaces/*` is web-only).
- No new inventory rows: the 65 rows are complete. If a slice finds behavior
  no row covers, it adds a row (`SPC-066…`), an oracle scenario and covers it
  in its slice; the gate never invents rows.
- No weakening of the import ban, the similarity check or a scenario's
  assertions to make a row flippable.

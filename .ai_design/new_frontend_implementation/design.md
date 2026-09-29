# New Frontend (`apps/web_new`) — Design

> Directory: `.ai_design/new_frontend_implementation/`
>
> **Status:** draft, not started (2026-09-29). Revised the same day:
> full functional parity is required (§15), and the old code may be read
> as a reference but never imported or copied (§12).
>
> Replaces the Plane-derived `apps/web` (and, for the desktop build,
> `desktop-overlay/` + `desktop/scripts/merge-web-tree.sh`) with an
> independently written React app in `apps/web_new/` that has **the same
> functionality** (§15). Both apps run side by side;
> screens move over one route group at a time. The backend
> (`apps/api`, `apps/live`, runner WebSocket) is unchanged except where
> §9.1 asks for schema coverage.

---

## 1. Problem

Three problems, one root cause: the frontend is Plane's, and it was never
built for our product, our license, or our desktop app.

1. **It is slow to load.** Measured on the current desktop bundle
   (`desktop/src-tauri/dist`, 2026-09-29):
   - 29 MB on disk, ~700 JS chunks.
   - Largest chunks: `use-editor-flagging` 1.6 MB, `editor` 1.3 MB,
     `index` ×3 at 404–621 KB, `toolbar` 529 KB, recharts
     `generateCategoricalChart` 351 KB.
   - `CoreRootStore` (`apps/web/core/store/root.store.ts`) builds ~30 MobX
     sub-stores at startup, including cycle, module, sticky, analytics,
     estimate and page stores that most sessions never touch.
   - Four headless UI systems (Base UI, Headless UI, Radix, Blueprint.js),
     three positioning libraries (Popper, Floating UI, tippy), two emoji
     pickers, two component kits (`@pi-dash/ui`, `@pi-dash/propel`),
     lodash, three font families and an icon font.
2. **It is AGPL.** The product is moving off AGPL. The frontend is mostly
   Plane code, so it has to be rewritten, not relicensed. See §12.
3. **It is poor inside the Tauri desktop app.** It is a website in a
   window: no native title bar, no native menus, web-only routes, low
   information density. The desktop build is produced by copying
   `desktop-overlay/` files over `apps/web` at build time, which is fragile
   and makes desktop-specific UX hard to build.

### 1.1 How the current frontend is structured

About 300k lines of TS/TSX:

| Layer | Location | Size |
|---|---|---|
| Routes (React Router 7, framework mode, `ssr: false`) | `apps/web/app/` (75 `page.tsx`) | ~15k |
| Feature UI | `apps/web/core/components/` (~50 folders) | ~133k |
| Edition seams (`@/pi-dash-web/*` alias) | `apps/web/ce/` | ~10k |
| Store hooks | `apps/web/core/hooks/store/` (38 hooks) | ~6k |
| MobX stores | `apps/web/core/store/` + `packages/shared-state` | ~26k |
| Services (axios) | `apps/web/core/services/` **and** `packages/services/` | ~11k |
| Shared libs | `packages/{ui,propel,editor,utils,constants,types,i18n,…}` | ~90k |

The intended flow is route → component → store hook → MobX store →
service → axios. In practice:

- There are two `APIService` base classes with different behavior (the
  package one has the EE interceptor registry; the app one has the 401
  redirect).
- 95 component files create a `new XService()` directly, and 96 use `useSWR`,
  mostly as a trigger that calls a MobX action. Data fetching has no single
  owner.
- Every sub-store receives the root (`this as unknown as RootStore`) and
  reads its siblings through it, so no feature can be loaded on its own.
- Customization is file replacement at build time (`ce/`, `ee-overlay/`,
  `desktop-overlay/`), not interfaces.

## 2. Goals

1. **Fast.** Initial JS ≤ 150 KB gzipped. Desktop first meaningful paint
   ≤ 500 ms when opening onto cached data. Enforced in CI (§13).
2. **Independent code.** `web_new` does not use the old frontend's code:
   no imports of its components, stores, services, utils, types or
   styles, and no copied code. The old code may be read as a reference
   for behavior and API usage (§12). The new tree can carry our new
   license.
3. **Desktop-first UX.** Native-feeling shell in Tauri; the same codebase
   serves the web.
4. **One clear data path.** Components get server data only through
   feature query/mutation hooks; only one module touches the network.
5. **Full functional parity.** When the project is done, every feature of
   `apps/web` (OSS and cloud edition) works in `web_new`: nothing missing,
   nothing broken. Parity is defined by an inventory and proven by a parity
   test suite (§15).
6. **No breakage during the transition.** `apps/web` keeps working until
   each area is replaced and has passed parity.
7. **Extension by interface.** Desktop and EE behavior plug in through
   typed interfaces, not file overlays.

## 3. Non-goals

- Changing the backend API shape. The new app uses the existing endpoints.
  (Schema annotations in §9.1 are additive.)
- Server-side rendering. The app is a static SPA (Tauri loads local files;
  the web is behind auth).
- Rewriting `apps/live` (the collaboration server) in this project.
  `apps/admin` and `apps/space` **are** in scope (D13): they become route
  subtrees `/god-mode/*` and `/spaces/*` inside `apps/web_new`.
- Visual parity. The visual design and layout are new; **functional
  parity is required** (§15). A feature may look and flow differently, but
  every capability must exist and work.
- Mobile layout beyond "usable" at narrow widths.

## 4. Decisions already settled

| # | Decision |
|---|---|
| D1 | Stay on **React** (team familiarity, ecosystem; the weight is architectural, not React's). |
| D2 | New app lives at **`apps/web_new/`**, package name `web_new`. |
| D3 | Stack: **Vite + React 19 (+ React Compiler), TanStack Router, TanStack Query, Zustand, Tailwind 4, Base UI** (§7). |
| D4 | **Not** a 1:1 translation of Plane's React code. The old code is a reference (read to learn behavior, edge cases and API usage), never a source (no imports, no copying). §12. |
| D5 | Side-by-side migration: desktop switches first, web routes move via Caddy path routing, then `apps/web` is deleted. |
| D6 | `apps/web` is frozen to bug fixes once the `web_new` shell exists. New features are built only in `web_new`. |
| D7 | **Full functional parity** with `apps/web`, OSS and cloud edition. No area is dropped. Done = every item in the feature inventory passes the parity suite on `web_new` (§15). |

## 5. Architecture overview

### 5.1 Repository placement

```
pi-dash/
  apps/
    web/            old app — bug fixes only, deleted at the end
    web_new/        NEW — the rewrite; serves web and desktop
    api/ live/ proxy/ admin/ space/   unchanged
  packages/
    (old @pi-dash/* packages — never imported by new code)
    kit/            NEW @pidash/kit — design system
    api-client/     NEW @pidash/api-client — HTTP client + endpoint contracts
  desktop/          Tauri — builds apps/web_new; overlay merge removed
```

Only two new shared packages. Everything else stays inside
`apps/web_new` until a second app actually needs it.

### 5.2 Layers

```
┌──────────────────────────────────────────────────────────────┐
│ routes/     URL → screen. Thin: params, search schema,       │
│             loader (prefetch), layout, lazy component        │
├──────────────────────────────────────────────────────────────┤
│ features/   one folder per domain: api (queries/mutations),  │
│             components, filters, local UI store, index.ts    │
├──────────────────────────────────────────────────────────────┤
│ shared/     cross-feature UI: shell, pickers, editor, hooks  │
├──────────────────────────────────────────────────────────────┤
│ core/       api setup, query client, session, platform,      │
│             edition, i18n, theme, telemetry                  │
├──────────────────────────────────────────────────────────────┤
│ @pidash/kit              @pidash/api-client                  │
└──────────────────────────────────────────────────────────────┘
```

### 5.3 Dependency rules (enforced by lint, §13.2)

1. A layer imports only from layers below it.
2. A feature imports another feature only through its `index.ts`. Anything
   used by three or more features moves to `shared/`.
3. Only `core/api` performs network I/O. Components never call `fetch`.
4. Only `core/platform` touches Tauri (`window.__TAURI__`, `invoke`).
5. `@pidash/kit` has no Pi Dash domain knowledge: no API types, no queries.
6. Nothing under `apps/web_new`, `packages/kit` or `packages/api-client`
   imports `@pi-dash/*` or any path inside `apps/web` (§12).

## 6. What each old piece becomes

| Old | New |
|---|---|
| React 18, React Router 7 framework mode | React 19 + Compiler, TanStack Router |
| MobX root store (~30 stores) | TanStack Query cache (server data) + URL search params (filters, view) + small Zustand stores (UI only) |
| SWR as fetch trigger | Route loaders + `useQuery` |
| axios `APIService` ×2 | One `fetch`-based client in `@pidash/api-client` |
| `@pi-dash/ui` + `@pi-dash/propel` | `@pidash/kit` |
| Base UI + Headless UI + Radix + Blueprint | Base UI only |
| Popper + Floating UI + tippy | Floating UI only (via Base UI) |
| Tiptap loaded widely, with Yjs + markdown pipeline | Tiptap lazy-loaded for editing only; HTML viewer elsewhere; collaboration as a separate chunk |
| lodash-es | Native JS |
| Material Symbols font + lucide | lucide only, per-icon imports |
| `ce/` alias, `ee-overlay/`, `desktop-overlay/` | `core/edition` and `core/platform` interfaces |
| MobX-backed i18n | Lightweight i18n, active locale loaded on demand |

## 7. Stack

| Concern | Choice | Why |
|---|---|---|
| Build | Vite | Static output that drops into Tauri `dist/`; already used |
| UI runtime | React 19 + React Compiler | Automatic memoization; Actions and `useOptimistic` for forms |
| Routing | TanStack Router (file routes) | Typed paths **and typed, validated search params** (filters live in the URL); loaders; per-route code splitting; intent preloading |
| Server data | TanStack Query | Cache, dedupe, background refresh, optimistic mutations, invalidation, persistence |
| Client state | Zustand | ~1 KB, per-feature stores, usable outside React (Tauri events, shortcuts) |
| Styling | Tailwind 4 + CSS variable tokens | No runtime cost |
| Primitives | Base UI | Headless, accessible, tree-shakable; one system |
| Lists/boards | TanStack Virtual | Large issue lists and boards |
| Tables | TanStack Table (headless) | Only where a real table is needed |
| Drag and drop | Atlassian pragmatic-drag-and-drop | Small, framework-agnostic; lazy per board |
| Forms | react-hook-form + zod (`zod/mini` in runtime paths) | Small; schemas shared with API contracts |
| Dates | date-fns | Tree-shakable |
| Editor | Tiptap (lazy) | `description_html` compatibility |
| Charts | Chosen before analytics is built (§18); lazy | Not on any startup path |
| Tests | Vitest, Testing Library, Playwright | Vitest already in the repo |

## 8. Directory layout

```
apps/web_new/
  index.html
  vite.config.ts            PIDASH_TARGET=web|desktop, size budgets
  src/
    main.tsx                bootstrap: platform → edition → queryClient → router
    routes/                 TanStack Router file routes (generated routeTree)
      __root.tsx
      _public/sign-in.tsx  sign-up.tsx  invitations.$id.tsx
      $ws/
        route.tsx           workspace layout; loader: me, workspace, projects
        index.tsx           home
        projects/
          index.tsx
          $projectId/
            route.tsx
            issues/index.tsx          list/board; validateSearch = IssueSearch
            issues/$issueId.tsx       full-page issue
            runners/…  schedulers/…
        runs/…  runners/…  schedulers/…  prompts/…  assistant/…
        notifications.tsx
        settings/…
    features/
      issues/
        api/keys.ts  queries.ts  mutations.ts
        components/  IssueList  IssueBoard  IssueRow  IssuePeek  IssueDetail …
        filters/     search.ts (zod)  FilterBar
        store.ts     selection, peek stack (Zustand)
        index.ts
      projects/ members/ states/ labels/ comments/ activity/
      runs/ runners/ schedulers/ prompts/ assistant/
      notifications/ auth/ workspace/ settings/
    shared/
      shell/        AppShell  Sidebar  TitleBar  CommandPalette  Toaster
      editor/       RichTextView  RichTextEditor (lazy)  CommentInput
      pickers/      Member  State  Label  Priority  Date  Project
      commands/     command + shortcut registry
      hooks/  lib/
    core/
      api/          client.ts  errors.ts  stream.ts
      query/        client.ts  persist.ts  realtime.ts
      session/      me  workspace  permissions
      platform/     types.ts  web.ts  tauri.ts  index.ts
      edition/      types.ts  oss.ts  index.ts
      i18n/  theme/  telemetry/
    styles/app.css

packages/kit/
  src/  button  input  select  combobox  menu  dialog  popover  tooltip
        tabs  toast  avatar  badge  kbd  table  virtual-list  empty-state
        skeleton  tokens.css
packages/api-client/
  src/  client.ts  contracts/<resource>.ts  (generated/ if §9.1 option A)
```

## 9. Subsystems

### 9.1 API client and contracts

**Finding.** drf-spectacular is installed but only enabled with
`ENABLE_DRF_SPECTACULAR=1`, and `settings/openapi.py` limits the schema to
`SCHEMA_PATH_PREFIX: "/api/v1/"` (the public API). The web app uses the
internal endpoints (`/api/workspaces/…`, `/auth/…`) in
`pi_dash/app/views`, which are not in the schema. Of 177 internal view
classes, only 48 declare `serializer_class`, and none use
`@extend_schema`. A generated client over the internal API would be mostly
untyped today.

**Decision: hand-written contracts first, generated later where it pays.**

- `@pidash/api-client` provides one small `fetch` wrapper:
  - base URL, credentials, CSRF header (from `/auth/get-csrf-token/`),
    JSON encode/decode, `AbortSignal`, timeout;
  - normalized `ApiError { status, code, message, fields }`;
  - middleware hooks (401 → session expired, EE interceptors);
  - the transport is injected (`platform.fetch`, §9.7).
- Each resource has a contract module written by us, from the endpoint's
  observed behavior and the Django serializer:
  ```ts
  // packages/api-client/src/contracts/issues.ts
  export const Issue = z.object({ id: z.string(), name: z.string(), state_id: z.string(), … });
  export type Issue = z.infer<typeof Issue>;
  export const issues = {
    list: (c: Client, ws: string, pid: string, q: IssueQuery) =>
      c.get(`/api/workspaces/${ws}/projects/${pid}/issues/`, { query: q, schema: IssueListResponse }),
    update: (c: Client, ws: string, pid: string, id: string, body: IssuePatch) =>
      c.patch(`/api/workspaces/${ws}/projects/${pid}/issues/${id}/`, { body, schema: Issue }),
  };
  ```
- Schemas are validated in development and tests, and skipped in
  production builds (types still apply). Drift shows up in dev and CI,
  not as user-facing crashes.
- Contract tests (§14) run each contract against the Django test server.
- **Later (option A):** extend drf-spectacular to cover internal
  endpoints feature by feature (`@extend_schema` on the views we touch).
  Once a resource's schema is complete, its contract can be generated
  with `openapi-typescript`. This does not block anything.

### 9.2 Server data (TanStack Query)

- Each feature exports key factories and `queryOptions`:
  ```ts
  export const issueKeys = {
    all: (ws: string) => ["ws", ws, "issues"] as const,
    list: (ws: string, pid: string, s: IssueSearch) => [...issueKeys.all(ws), "list", pid, s] as const,
    detail: (ws: string, id: string) => [...issueKeys.all(ws), "detail", id] as const,
  };
  export const issueListQuery = (ws: string, pid: string, s: IssueSearch) =>
    queryOptions({ queryKey: issueKeys.list(ws, pid, s), queryFn: ({ signal }) => … });
  ```
- Every key starts with `["ws", ws]`, so switching workspace or signing
  out clears exactly one subtree.
- Mutations are optimistic by default for issue field edits, state moves,
  reorders and comments: `onMutate` patches the cache, `onError` rolls back,
  `onSettled` invalidates affected keys. Shared helpers live in
  `core/query`.
- Defaults: `staleTime` 30 s for lists, 5 min for reference data
  (states, labels, members, projects); `refetchOnWindowFocus` on (desktop
  window focus included via `platform`).
- **Persistence:** desktop persists the whole cache through
  `platform.storage` (file-backed) with a size cap and a schema-version
  buster; web persists reference data only (IndexedDB). On launch the app
  renders from the cache, then refreshes.
- **Real-time:** `core/query/realtime.ts` is the only place that turns
  server events into cache patches/invalidations. Initial sources are the
  existing SSE streams (runner chat, assistant). Issue-level push events
  are an open question (§18).

### 9.3 Routing and URL state (TanStack Router)

- File-based routes with the Vite plugin; each route's component is a lazy
  chunk.
- Loaders call `queryClient.ensureQueryData(...)`, so route code and data
  load in parallel. `defaultPreload: "intent"` preloads on hover/focus.
- **URL is the source of truth for view state:** filters, grouping,
  ordering, layout (list/board), and the open issue (`?peek=<id>`). Each
  route declares a zod `validateSearch`; invalid params fall back to
  defaults instead of throwing.
- Saved views store a serialized search object, so a saved view is just a
  link.
- `beforeLoad` on `$ws/route.tsx` enforces the session and workspace
  membership; `_public/*` routes redirect signed-in users.
- Legacy URLs from `apps/web` (e.g. `/:ws/projects/:pid/issues/`,
  `/:ws/browse/:workItem`) keep working: either same paths or a redirect
  table in `routes/_legacy.tsx`.

### 9.4 Client state (Zustand)

Only state that is (a) not server data, (b) not worth putting in the URL,
and (c) shared by distant components:

- issue multi-select and bulk-action bar
- peek-panel stack (beyond the top item, which is in the URL)
- command palette open/query
- sidebar collapsed/width (persisted per user via `platform.storage`)
- unsent drafts (issue create, comment)

Stores are created per feature and reset on sign-out via a single
`resetAll()` registry in `core/session`. No store references another
store.

### 9.5 Design system (`@pidash/kit`)

- Base UI primitives, styled with Tailwind 4 and CSS-variable tokens
  (`tokens.css`): color (light/dark), spacing, radius, type scale,
  elevation, and a `--density` scale (desktop defaults to compact).
- Components: Button, IconButton, Input, Textarea, Select, Combobox,
  Menu, ContextMenu, Dialog, Sheet, Popover, Tooltip, Tabs, Toast, Avatar,
  AvatarGroup, Badge, Kbd, Checkbox, Switch, Table, VirtualList,
  EmptyState, Skeleton, Spinner.
- Developed in isolation (Ladle or Storybook) with visual review before
  features use them.
- Icons: lucide-react, imported per icon. Fonts: one variable UI font +
  one mono font, `font-display: swap`, latin subset preloaded.
- A new visual design (§10) is made before the kit is built, not
  borrowed from Plane.

### 9.6 Editor

- `RichTextView`: renders stored `description_html` / `comment_html` via
  a sanitizer. No Tiptap. Used in lists, peek, comments, activity.
- `RichTextEditor`: `React.lazy` Tiptap editor, loaded when the user
  starts editing (and preloaded on hover of the description area). Minimal
  extension set: paragraphs, headings, lists, task lists, code, links,
  mentions, images (upload via the existing asset endpoints).
- **Compatibility requirement:** HTML written by the new editor must render
  correctly in `apps/web` and vice versa during coexistence. A fixture
  set of real stored descriptions is round-tripped in tests (§14).
- `CommentInput`: a lighter editor (same Tiptap chunk, fewer extensions)
  or markdown textarea; decided in Phase 1 by measured size.
- Real-time collaboration (Yjs + `apps/live` Hocuspocus) is its own lazy
  chunk, loaded only for documents that need it (pages, §15.1).

### 9.7 Platform (`core/platform`)

Replaces `desktop-overlay/` and the axios desktop adapter.

```ts
export interface Platform {
  kind: "web" | "desktop";
  fetch: typeof fetch;                         // desktop: Rust HTTP via invoke (desktop_http.rs)
  stream(url: string, init?: StreamInit): EventStream;  // SSE; desktop: desktop_api_stream
  openExternal(url: string): Promise<void>;
  storage: KeyValueStore;                      // cache persistence, UI prefs
  onFocusChange(cb: (focused: boolean) => void): Unsubscribe;
  window?: TitleBarApi;                        // drag region, traffic-light inset, fullscreen
  menu?: MenuApi;                              // native menu → command registry
  deepLinks?: DeepLinkApi;                     // pidash:// scheme
  agentRuntime?: AgentRuntimeApi;              // bundled runner / managed runner controls
  updates?: UpdaterApi;
}
```

- Selected at build time: `PIDASH_TARGET=web` imports `web.ts`,
  `PIDASH_TARGET=desktop` imports `tauri.ts`. The web bundle contains no
  Tauri code (verified by a bundle check).
- Features ask for optional capabilities (`platform.agentRuntime?`) and
  render nothing when absent. There are no `if (isDesktop)` branches
  outside `core/platform`.
- The Rust side (`desktop/src-tauri/src/*.rs`) is unchanged at first; the
  TS side re-implements the call shapes that `packages/services`
  `desktop-api-adapter.ts` / `desktop-event-source.ts` use today, written
  fresh against the Rust commands.

### 9.8 Editions (`core/edition`)

Replaces `ce/` + `ee-overlay/` file swapping.

```ts
export interface Edition {
  id: "oss" | string;
  routes?: RouteExtension[];          // extra route subtrees
  sidebar?: SidebarItem[];
  settingsSections?: SettingsSection[];
  auth?: AuthProviderExtension[];     // extra sign-in methods
  api?: ApiMiddleware[];              // e.g. token refresh
  flags: Record<string, boolean>;
  slots?: Partial<SlotComponents>;    // named UI slots, e.g. "issue.detail.sidebar.after"
}
```

- OSS ships `oss.ts`. The private repo's build provides its own module
  through a Vite alias for `@pidash/edition` only; that is the single
  swap point, and it has a typed contract.
- Slots are an explicit, short list. Adding a slot is a design decision,
  not a workaround.

### 9.9 Session and auth

- Web: existing cookie session + CSRF from `/auth/get-csrf-token/`; flows
  (email check, password, magic code, OAuth redirects, sign-out) re-built
  against `/auth/*`.
- Desktop: same endpoints through `platform.fetch`; external OAuth via
  `platform.openExternal` + `deepLinks`.
- `core/session` exposes `useMe()`, `useWorkspace()`, `usePermissions()`
  (role checks in one place). 401 from any request → session-expired
  state → sign-in, preserving the return URL.

### 9.10 i18n and theme

- i18n: ICU messages (`intl-messageformat` or a smaller equivalent),
  **new message keys and English copy written for this app**, one JSON
  per locale, only the active locale loaded. English ships first; other
  locales are added from new translations, not copied from
  `packages/i18n`.
- Theme: light/dark/system via tokens; follows the OS on desktop.

### 9.11 Errors and telemetry

- Route-level error boundaries with retry; query errors surface as inline
  states, mutation errors as toasts with the server message.
- `core/telemetry`: a small interface (page view, error, timing). The
  current Clarity snippet in `apps/web/app/root.tsx` is not carried over;
  whether to add analytics is a product decision (§18).
- Startup timings (`navigationStart → shell → first data`) are recorded
  and reported to the budget check (§13).

## 10. Desktop-first UX

- Custom title bar with drag region; macOS traffic-light inset; window
  title follows the route.
- Native menu bar (File/Edit/View/Go/Window/Help) wired to the same
  command registry as ⌘K. Every command has one ID, one handler, one
  shortcut, shown in the menu, palette and tooltips.
- Keyboard-first: list navigation (j/k), peek (space), open (enter),
  assign/state/label shortcuts, `g` + key navigation.
- Compact density by default on desktop; comfortable on web.
- Peek panel for issues instead of navigating away from lists.
- Opens instantly on cached data (§9.2) with a subtle "syncing" state.
- No web-only surfaces in the desktop build (marketing, sign-up upsell);
  handled by `platform.kind` at the route level, not by file overlays.
- Future (not in scope now): multiple windows, tray, offline edits.

A visual design pass (Figma or a kit prototype) happens in Phase 0 before
building `@pidash/kit` components.

## 11. Coexistence and migration

### 11.1 Web

- `apps/proxy` Caddyfiles route migrated path prefixes to `web_new` and
  everything else to `web`. Same origin, same session cookie, so crossing
  between apps is a full page load but otherwise seamless.
- The route list lives in one file per deployment and grows each phase.
- `web_new` links to not-yet-ported screens with plain `<a href>` (full
  load into `apps/web`), generated from the same route list so they flip
  automatically when a screen moves.

### 11.2 Desktop

- `desktop/scripts/dev-prep.sh` gains a switch to build `apps/web_new`
  (`PIDASH_TARGET=desktop`) instead of merging the overlay tree.
- The desktop switches once Phase 2 (§16) is complete. Screens not ported
  yet open in the system browser via `platform.openExternal`.
- After the switch, `desktop-overlay/` and `merge-web-tree.sh` are
  deleted.

### 11.3 Freeze policy

- From the end of Phase 0, `apps/web` takes bug fixes and security fixes
  only.
- New features are built only in `web_new`. If a feature must reach web
  users before its area is migrated, that area's migration is pulled
  forward rather than building the feature twice.

### 11.4 End state

`apps/web`, `desktop-overlay/`, `apps/web/ce`, and the old packages that
nothing else uses (`ui`, `propel`, `shared-state`, `services`, `hooks`,
`i18n`, `editor`, `utils`, `constants`, `types`, `codemods`,
`decorators`) are removed, once `admin`/`space` no longer depend on them
or are themselves rewritten.

## 12. Reference, not source

`web_new` is written independently. The old frontend is a **reference**:
agents and people may read it to learn what the product does. It is never
a **source**: nothing is imported from it and nothing is copied out of it.

### 12.1 Not allowed

1. Importing anything from the old frontend: `@pi-dash/*` packages,
   `apps/web/**`, `apps/web/ce/**`, `desktop-overlay/**`, or the private
   `ee-overlay/apps/web/**`. This includes components, hooks, stores,
   services, utils, constants, types and styles.
2. Copying code, even small snippets, and then editing it.
3. Mechanical translation: rewriting an old file into the new stack while
   keeping its structure, names and control flow.
4. Copying Tailwind class strings, CSS, i18n message keys or copy text,
   SVG icons, images or other assets.

### 12.2 Allowed

1. Reading old code to learn behavior: what a screen does, validation
   rules, edge cases, permission checks, empty/error states, keyboard
   shortcuts, and which API calls it makes with which parameters.
2. Running `apps/web` and observing it (this is also how the parity suite
   works, §15).
3. Writing what was learned **in prose** in the feature inventory and
   feature specs (§15). The spec, not the old file, is what the new code is
   written from.

### 12.3 Working rule for agents

Every implementation issue follows **read → spec → write**:

1. Read the relevant old code and run the old screen, if needed.
2. Update the area's spec with the behavior, in your own words, citing
   inventory IDs. No code in specs, except API paths and field names.
3. Write the new code from the spec, this design doc and the kit. Do not
   have old files open while writing new ones.

### 12.4 Enforcement

- Lint: import bans in §12.1.1 (part of §5.3 rule 6).
- CI **similarity check**: a token-based duplicate detector (e.g. jscpd)
  compares `apps/web_new`, `packages/kit` and `packages/api-client`
  against `apps/web` and the old `packages/*`. Any duplicated block of
  ≥ 50 tokens fails the build. The threshold is tuned in Phase 0 against
  false positives (JSX boilerplate, imports) and recorded here.
- CI header check: every new file carries the new license header; no file
  under the new trees carries the old SPDX header.
- Review checklist: the reviewer compares the diff against the old files
  the spec cites and rejects anything that looks translated rather than
  written.

### 12.5 Licensing note

Allowing implementers to read the old code is a weaker position than a
strict clean-room process, where the people who read the original never
write the new code. The spec step (§12.3) and the similarity check
(§12.4) are the mitigations. If legal review (§18) asks for more, the
cheap option with AI agents is to split roles for sensitive areas: a spec
run reads old code and writes the spec; a separate implementation run
sees only the spec and is not given access to the old code.

This project covers the frontend only. `apps/api`, `apps/live`,
`apps/admin`, `apps/space` and the remaining packages are also
Plane-derived and stay AGPL until handled separately.

## 13. Performance budgets

### 13.1 Budgets (CI-enforced)

| Metric | Budget | Check |
|---|---|---|
| Initial JS (shell + first route), gzipped | ≤ 150 KB | `size-limit` on build output |
| Any route chunk, gzipped | ≤ 50 KB (excl. named heavy chunks) | `size-limit` |
| Editor chunk, gzipped | tracked, alert on +10% | `size-limit` |
| Initial CSS, gzipped | ≤ 30 KB | `size-limit` |
| Desktop first meaningful paint, warm cache | ≤ 500 ms | Tauri smoke test (Playwright/WebDriver) |
| Issue list, 2 000 rows, scroll | no frame > 50 ms | Playwright trace |

A PR over budget fails; raising a budget needs a note in this doc.

### 13.2 Structural checks

- Import-boundary lint (§5.3), including the `@pi-dash/*` ban.
- Named heavy dependencies (editor, charts, drag-and-drop, PDF,
  emoji, collaboration) may only be reached through `import()`; a lint
  rule rejects static imports of them.
- Web bundle must contain no Tauri code; desktop bundle no web-only
  routes.
- Dependency allowlist for `web_new` and `kit`: adding a runtime
  dependency requires review.

## 14. Testing

| Level | Tool | What |
|---|---|---|
| Unit | Vitest | query key factories, optimistic updaters, search schemas, reducers, utils |
| Component | Vitest + Testing Library | kit components, feature components with a mocked client |
| Contract | Vitest against Django test server | every `@pidash/api-client` contract parses real responses |
| Editor compat | Vitest | fixture set of stored `description_html` round-trips without loss |
| **Parity** | Playwright, two drivers | every feature inventory item, run against `apps/web` (oracle) and `web_new` (§15.3) |
| E2E | Playwright (web) | smoke flows for `web_new`-only behavior (shell, peek, keyboard, cache) |
| Desktop smoke | Tauri + WebDriver | launch, sign-in, cached start, runner controls, deep link |
| Performance | size-limit, Playwright traces | §13 budgets |

## 15. Functional parity

**Everything `apps/web` does today, `web_new` must do when the project is
done.** No area is dropped. This covers the OSS build, the cloud edition
(`private-pi-dash/ee-overlay/apps/web`) and the desktop build
(`desktop-overlay/`). The UI may be redesigned; capabilities, rules and
outcomes may not be lost.

Parity is not judged by eye. It is defined by an inventory and proven by a
test suite.

### 15.1 Areas

| Area | Old routes / source | Phase |
|---|---|---|
| Auth, sign-up, invitations, onboarding, create workspace | `accounts`, `auth`, `sign-up`, `invitations`, `workspace-invitations`, `onboarding`, `create-workspace` | 1 |
| Workspace shell, home, projects list, command palette / Power K, search | `(projects)/page`, `projects/(list)`, `power-k`, `command-palette` | 1 |
| Issues: list, board, spreadsheet, calendar, gantt; detail, peek, filters, display options, bulk edit, sub-issues, relations, links, attachments, reactions, subscriptions | `projects/…/issues`, `browse/[workItem]` | 1 |
| Comments, activity, mentions | issue detail | 1 |
| Drafts | `drafts` | 1 |
| Runners, runs, approvals, runner chat, AI dev machines | `runners/*`, `ai-dev-machines`, project `runners` | 2 |
| Schedulers, prompts, assistant | `schedulers`, `prompts`, `assistant/*`, project `schedulers` | 2 |
| Notifications | `notifications` | 2 |
| Desktop: agent runtime, bare sign-in, updater, deep links | `desktop-overlay/`, `ce/components/desktop` | 2 |
| Views (project and workspace) | `views`, `workspace-views` | 3 |
| Archives | `archives/*` | 3 |
| Intake | `intake` | 3 |
| Cycles, active cycles | `cycles`, `active-cycles` | 3 |
| Modules | `modules` | 3 |
| Pages (collaborative documents) | `pages` | 3 |
| Estimates | project settings `estimates` | 3 |
| Analytics | `analytics/[tabId]` | 3 |
| Stickies | `stickies` | 3 |
| Exports (CSV, PDF) | `exporter`, settings `exports` | 3 |
| Project settings: general, members, states, labels, features, GitHub, automations, schedulers | `settings/projects/[projectId]/*` | 3 |
| Workspace settings: general, members, integrations, webhooks, API tokens, billing | `settings/(workspace)/*` | 3 |
| Profile, account settings, appearance, notifications preferences | `profile/[userId]`, `settings/account`, `settings/profile/*` | 3 |
| Cloud edition: marketing/home, docs, downloads, pricing, login, apps, profile tabs | `ee-overlay/apps/web/app/**` | 3 (via `core/edition`) |

In scope as well (NEWFRONT-4): `apps/admin` (instance admin, route subtree
`/god-mode/*`) and `apps/space` (public boards, route subtree `/spaces/*`),
both web build only, removed together with `apps/web` in Phase 4.

### 15.2 Feature inventory

`.ai_design/new_frontend_implementation/parity/inventory/<area>.md`
lists every capability as a row with a stable ID:

| Field | Example |
|---|---|
| ID | `ISS-042` |
| Capability | Bulk-change state of selected issues in list layout |
| Who | member+, not guest |
| Edition | oss / cloud / desktop / all |
| Old entry point | `projects/…/issues` list, selection bar |
| API | `POST /api/workspaces/{ws}/projects/{pid}/bulk-operation-issues/` |
| Acceptance | all selected issues move; activity entry per issue; list regroups |
| Parity test | `parity/issues/bulk-state.spec.ts` |
| Status | not started / oracle green / new green |

- The inventory is built in Phase 0 by agents reading the old code and
  running the old app, one area per issue, and **reviewed by a human**
  before that area's implementation starts. It is the definition of
  "done".
- Things that are easy to miss get their own rows: permissions per role,
  keyboard shortcuts, empty and error states, URL/deep-link behavior,
  exports, drag-and-drop, real-time updates, settings that change other
  screens, and edition- or desktop-only behavior.
- New rows found later (a behavior nobody listed) are added with the next
  ID and a note of where they were found.

### 15.3 Parity suite

Like the Rust port's contract tests, the old app is the **oracle**.

- Scenarios live in `apps/web_new/e2e/parity/<area>/`, one or more per
  inventory ID, tagged with the ID.
- Scenarios are written against a **driver interface** of user-level
  actions and reads (`createIssue`, `setState`, `applyFilter`,
  `openSettings`, `listVisibleIssues`, …). There are two drivers:
  `drivers/web` (selectors for `apps/web`) and `drivers/web_new`. The UI
  can differ; the scenario is the same.
- Assertions check both what the user sees (through the driver) and the
  resulting server state (through the API), so a redesigned screen
  cannot pass by looking right while saving the wrong thing.
- Order: a scenario is first made green against `apps/web`. That proves
  the scenario is correct. Then the `web_new` implementation must make it
  green too.
- Runs against a seeded local stack (Django + Postgres + `apps/live`),
  both editions, and the desktop build for desktop-tagged rows.
- CI publishes a parity report: per area, inventory rows × {oracle green,
  new green}.

### 15.4 Gates

| Gate | Condition |
|---|---|
| Area implementation may start | the area's inventory is reviewed; its oracle scenarios are green on `apps/web` |
| Area route moves to `web_new` in Caddy / desktop | 100% of the area's inventory rows green on `web_new` |
| Desktop switches to `web_new` (end of Phase 2) | all Phase 1–2 areas and all desktop rows green |
| `apps/web` is removed (Phase 4) | 100% of all inventory rows green on `web_new` for both editions and desktop; a human acceptance pass signed off |

## 16. Phases

### Phase 0 — Foundation (no user-visible change)

1. Visual design direction: tokens, type, density, shell layout, issue
   list/board/peek mockups.
2. Scaffold `apps/web_new` (Vite, React 19 + Compiler, TanStack Router +
   Query, Tailwind 4), `packages/kit`, `packages/api-client`.
3. Lint rules (§5.3, §13.2), `size-limit`, license-header check, CI job.
4. `core/api`, `core/query` (incl. persistence), `core/platform`
   (`web.ts`, `tauri.ts`), `core/edition` (`oss.ts`), `core/session`.
5. Kit v0: Button, Input, Menu, Dialog, Popover, Tooltip, Toast,
   Avatar, Badge, Kbd, VirtualList, Skeleton.
6. Shell: title bar, sidebar, command palette, workspace switcher.
7. Sign-in (email + password / magic code), read-only issue list for one
   project.
8. Measure baseline vs. `apps/web` (bundle, web load, desktop startup)
   and record in Appendix B.
9. Freeze `apps/web` (§11.3).
10. Parity foundation (in parallel with 2–7): seeded test stack,
    driver interface, `drivers/web`, parity report in CI, similarity
    check (§12.4).
11. Feature inventory for **every** area in §15.1, one issue per area,
    human-reviewed.
12. Oracle scenarios for Phase 1 areas, green on `apps/web`.

**Exit:** a signed-in user can open a project's issue list in both web and
desktop builds; budgets and the similarity check are green; the full
inventory is reviewed; Phase 1 oracle scenarios are green; measured
numbers are in this doc.

### Phase 1 — Daily issue workflow

Issue list + board (virtualized), filters in URL, peek + full detail,
create/edit/move with optimistic updates, rich-text view + lazy editor,
comments, activity, pickers, keyboard navigation, home, projects list,
full auth + onboarding flows. Contract and editor-compat tests.

Oracle scenarios for Phase 2 areas are written during this phase.

**Exit:** all Phase 1 inventory rows green on `web_new`.

### Phase 2 — Pi Dash core + desktop switch

Runners, runs, approvals, runner chat (SSE through `platform.stream`),
schedulers, prompts, assistant, notifications, agent-runtime controls,
deep links, native menus. Switch the desktop build to `web_new`; delete
`desktop-overlay/` and `merge-web-tree.sh`.

Oracle scenarios for Phase 3 areas are written during this phase.

**Exit:** the §15.4 desktop gate passes; desktop ships on `web_new`;
screens not yet moved open in the browser.

### Phase 3 — Web cutover

Every remaining area in §15.1, including cycles, modules, pages
(collaborative editing via `apps/live`), analytics, intake, estimates,
stickies, exports, settings, profile, and the cloud edition module in the
private repo. Each area moves in Caddy as soon as it passes its gate.

**Exit:** every area served by `web_new` on the web; every inventory row
green.

### Phase 4 — Removal

After the final §15.4 gate and a human acceptance pass: remove
`apps/web`, legacy route handling that is no longer needed, and old
packages no longer used by `admin`/`space` (§11.4). The parity suite
keeps running on `web_new` only, as its regression suite.

### 16.1 PR sequence (Phase 0)

1. `web_new` scaffold + CI (build, typecheck, lint, size-limit, header check).
2. `@pidash/api-client` core + `auth`, `me`, `workspace`, `project`,
   `issue` (read) contracts + contract-test harness.
3. `core/*` (platform, edition, query, session) + unit tests.
4. `@pidash/kit` v0 + Ladle.
5. Shell + sign-in + read-only issue list.
6. Desktop build switch behind a flag (`PIDASH_DESKTOP_WEB=web_new`) +
   smoke test.
7. Baseline measurements (Appendix B).
8. Parity harness: seeded stack, driver interface, `drivers/web`,
   report; similarity check.
9. Inventory issues, one per area (can run in parallel with 1–8).

## 17. Risks

| Risk | Mitigation |
|---|---|
| The rewrite takes longer than planned while the old app keeps moving | Freeze policy (§11.3); desktop-first ordering delivers value mid-way; progress is measurable from the parity report |
| Full parity is a large scope (cycles, modules, pages with collaboration, analytics, gantt, exports, cloud edition) | Everything heavy is lazy (§13); areas are independent epics that run in parallel once Phase 1 patterns exist |
| A feature is missed because nobody listed it | Inventory built from code *and* running app, human review, "easy to miss" row categories (§15.2), acceptance pass before removal |
| Parity scenarios encode old bugs as required behavior | A scenario that asserts a bug is marked `bug:` with a linked issue; `web_new` may fix it, and the row records the intended behavior |
| Two UIs drift, users confused during transition | Same URLs where possible, one route list driving proxy + links, short Phase 3 |
| Undocumented internal API behavior | Contracts + contract tests (§9.1); dev-mode schema validation surfaces drift early |
| Editor output incompatible with stored HTML or `apps/web` | Fixture round-trip tests (§9.6); minimal extension set |
| Performance regresses feature by feature | CI budgets and import rules (§13) from the first PR |
| Reading old code turns into copying it (people or agents) | Read → spec → write (§12.3), similarity check in CI, import bans, review checklist (§12.4) |
| Real-time expectations (others' edits appearing live) not met without MobX-style sockets | §18 decision on issue push events; focus/interval refetch as baseline |

## 18. Open questions

1. **License** for the new tree (proprietary, source-available, or a
   permissive license) and the exact header text.
2. **Legal review** of the reference-not-source process (§12), whether
   any areas need split spec/implementation roles (§12.5), and a plan for
   the rest of the Plane-derived code (API, live, admin, space).
3. **Out of scope or not:** `apps/admin` and `apps/space`.
4. **Real-time issue updates:** is there a server push channel for issue
   changes today (beyond runner/assistant SSE)? If not, do we add one
   (SSE endpoint fed by existing signals) or rely on focus/interval
   refetch? Parity requires whatever live behavior `apps/web` has today.
5. **OpenAPI coverage:** do we invest in `@extend_schema` on internal
   views (option A in §9.1), and when?
6. **Charts library** for analytics.
7. **Analytics/telemetry** on the new app (replace Clarity or not).
8. **Visual design owner** and tool (Figma vs. kit-first prototyping).
9. **Similarity threshold** for §12.4, set after the Phase 0 trial.

---

## Appendix A — One interaction, end to end

Opening an issue in the peek panel from the list and editing its
description:

```
click row
  → navigate({ search: (s) => ({ ...s, peek: issueId }) })      typed search param
  → route loader: queryClient.ensureQueryData(issueDetailQuery)  cache hit → instant
       → @pidash/api-client issues.get
       → core/api client (CSRF, errors)
       → platform.fetch                                          web fetch | Rust invoke
  → <IssuePeek> useSuspenseQuery(issueDetailQuery)
       → <RichTextView html={issue.description_html}/>           no Tiptap loaded
user clicks description
  → import("shared/editor/RichTextEditor")                       editor chunk (preloaded on hover)
save
  → useUpdateIssue():
       onMutate   patch detail + list caches                     UI updates immediately
       mutationFn PATCH /api/workspaces/:ws/projects/:pid/issues/:id/
       onError    roll back, toast server message
       onSettled  invalidate issueKeys.detail + affected lists
```

## Appendix B — Baseline measurements

`apps/web` figures come from the local desktop build in
`desktop/src-tauri/dist` (2026-09-20), counting the entry plus every
module and import the React Router manifest lists for the route chain.
That build's `index.html` references a different manifest hash than the
one shipped next to it, so CSS is not counted. Re-measure on a fresh
build in Phase 0.

| Metric | `apps/web` | `apps/web_new` |
|---|---|---|
| Desktop `dist/` size | 29 MB | |
| JS chunks | ~700 | |
| JS to open a project's issue list | 245 files, 7.6 MB raw / 2.3 MB gzipped (7 routes in chain) | target ≤ ~200 KB gzipped |
| JS to open an issue's full page | 210 files, 5.9 MB raw / 1.8 MB gzipped | |
| Loaded on issue list but not needed there | editor chunks (1.6 MB + 1.3 MB raw), recharts (351 KB raw) | none |
| Largest chunk | 1.6 MB (`use-editor-flagging`) | |
| Web: time to issue list (cold / warm) | | |
| Desktop: launch → issue list (cold / warm) | | |

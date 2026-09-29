# Architecture

**Read this when:** you create files, decide where code goes, or add an import.

## Repository placement

```
pi-dash/
  apps/
    web/            old app — bug fixes only, removed at the end
    web_new/        the new app; serves web and desktop
  packages/
    kit/            @pidash/kit — design system
    api-client/     @pidash/api-client — HTTP client + endpoint contracts
    (old @pi-dash/* packages — never imported by new code)
  desktop/          Tauri — will build apps/web_new; overlay merge removed
```

Only two new shared packages. Everything else stays inside `apps/web_new` until a second app needs it.

## Layers

```
routes/     URL → screen. Thin: params, search schema, loader (prefetch), layout, lazy component
features/   one folder per domain: api (queries/mutations), components, filters, local UI store, index.ts
shared/     cross-feature UI: shell, pickers, editor, commands, hooks
core/       api setup, query client, session, platform, edition, i18n, theme, telemetry
@pidash/kit          @pidash/api-client
```

## Import rules (lint-enforced)

1. A layer imports only from layers below it.
2. A feature imports another feature only through its `index.ts`. Anything used by three or more features moves to `shared/`.
3. Only `core/api` performs network I/O. Components never call `fetch`.
4. Only `core/platform` touches Tauri (`window.__TAURI__`, `invoke`).
5. `@pidash/kit` has no Pi Dash domain knowledge: no API types, no queries.
6. Nothing in the new trees imports the old frontend (see *Reference, not source*).

## Directory layout

```
apps/web_new/src/
  main.tsx                bootstrap: platform → edition → queryClient → router
  routes/                 TanStack Router file routes
    __root.tsx
    _public/              sign-in, sign-up, invitations
    $ws/
      route.tsx           workspace layout; loader: me, workspace, projects
      index.tsx           home
      projects/$projectId/
        issues/index.tsx  list/board; validateSearch = IssueSearch
        issues/$issueId.tsx
        …
      runs/ runners/ schedulers/ prompts/ assistant/ notifications.tsx settings/
  features/
    issues/
      api/                keys.ts  queries.ts  mutations.ts
      components/
      filters/            search.ts (zod) + FilterBar
      store.ts            Zustand, UI state only
      index.ts            public API of the feature
    projects/ members/ states/ labels/ comments/ activity/ runs/ runners/
    schedulers/ prompts/ assistant/ notifications/ auth/ workspace/ settings/ …
  shared/
    shell/                AppShell, Sidebar, TitleBar, CommandPalette, Toaster
    editor/               RichTextView, RichTextEditor (lazy), CommentInput
    pickers/              Member, State, Label, Priority, Date, Project
    commands/             command + shortcut registry
    hooks/ lib/
  core/
    api/  query/  session/  platform/  edition/  i18n/  theme/  telemetry/
  styles/app.css
apps/web_new/e2e/parity/  parity scenarios and drivers
```

## What each old piece becomes

| Old | New |
|---|---|
| React 18, React Router 7 framework mode | React 19 + Compiler, TanStack Router |
| MobX root store (~30 stores built at startup) | TanStack Query cache + URL search params + small Zustand stores |
| SWR as a fetch trigger | route loaders + `useQuery` |
| two axios `APIService` classes | one `fetch` client in `@pidash/api-client` |
| `@pi-dash/ui` + `@pi-dash/propel` | `@pidash/kit` |
| Base UI + Headless UI + Radix + Blueprint | Base UI only |
| Popper + Floating UI + tippy | Floating UI only (via Base UI) |
| Tiptap loaded widely | Tiptap lazy-loaded for editing only |
| lodash-es | native JS |
| `ce/` alias, `ee-overlay/`, `desktop-overlay/` file swapping | `core/edition` and `core/platform` interfaces |

## One interaction, end to end

Opening an issue in the peek panel and editing its description:

```
click row → navigate({ search: s => ({ ...s, peek: issueId }) })
  → route loader: queryClient.ensureQueryData(issueDetailQuery)   (cache hit → instant)
      → @pidash/api-client → core/api client → platform.fetch
  → <IssuePeek> useSuspenseQuery(issueDetailQuery) → <RichTextView html=…/>   (no Tiptap)
user clicks description → import("shared/editor/RichTextEditor")
save → useUpdateIssue(): onMutate patch caches → PATCH → onError roll back → onSettled invalidate
```

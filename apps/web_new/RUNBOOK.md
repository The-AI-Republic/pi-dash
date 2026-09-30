<!-- Copyright (c) Pi Dash contributors. License pending H-license decision (NEWFRONT-2). -->

# web_new runbook — feature patterns (NEWFRONT-17)

The first vertical slice (shell, sign-in, read-only issue list) sets the
patterns every epic copies. This file is the canonical description; the
area spec lives at
`.ai_design/new_frontend_implementation/parity/specs/f06-vertical-slice.md`.

## Where a feature's code goes

```
src/features/<name>/
  api/
    keys.ts        key factory; every key starts with ["ws", ws] (Data layer)
    queries.ts     queryOptions(client, ws, …) — loaders prefetch, hooks read
    mutations.ts   optimistic mutations via core/query helpers (when needed)
  components/      one file per component, presentational where possible
  filters/
    search.ts      zod-mini schema for the route's search params + parse fallback
  store.ts         Zustand, UI state only, registered in core/session reset
  index.ts         the feature's public API — other features import only this
```

`src/shared/` holds cross-feature UI: `shell/` (AppShell, Sidebar,
TitleBar, CommandPalette, toaster) and `commands/` (registry + palette
store). `src/core/` holds `api` (the singleton HTTP client), `query`,
`session`, `platform`, `edition`. `src/routes/` holds file routes only.

## Layer rules (enforced by `pnpm check:boundaries`)

- routes → features → shared → core, strictly downwards.
- A feature imports another feature only through its `index.ts`.
- Only `core/api` performs network I/O (transport is `platform.fetch`).
- Components never call `fetch`; they use feature hooks or suspense queries.
- Shell components are presentational: the route loads data and passes
  props, because `shared/` cannot import `features/`.

## Routes

- Thin: params, search schema, gates, prefetch, layout, component.
- `validateSearch` is a zod-mini schema (or a function using one); invalid
  params fall back to defaults, never throw.
- `beforeLoad` enforces session and membership (redirects, `notFound()`).
- `loader` prefetches with `queryClient.ensureQueryData(…)` so code and
  data load in parallel; the component reads back with `useSuspenseQuery`.
- `errorComponent` (retry) and `notFoundComponent` (access notice) per area.
- Unknown paths render the root `notFoundComponent`: not-yet-migrated areas.

## Coexistence (F-09)

- `apps/web_new/migrated-routes.json` is the one list of path prefixes the
  proxy serves from web_new; everything else goes to the old app on the
  same origin (session cookie shared, no origin/cookie change). Empty until
  area gates append their prefixes.
- After editing the list, run `pnpm --filter web_new coexistence:generate`
  to rewrite the marked blocks in `apps/proxy/Caddyfile.ce` and
  `Caddyfile.aio.ce`; never edit those blocks by hand. CI enforces
  freshness with `pnpm --filter web_new check:coexistence`.
- Links to not-yet-migrated screens use
  `src/shared/coexistence/CrossAppLink.tsx`: unmigrated paths render as
  plain `<a href>` (full-page load into the old app), migrated paths as
  router links. Both read the same list, so a link flips the moment its
  prefix lands — no call site changes.

## Queries and session

- Key shape: `["ws", workspaceSlug, "<feature>", …]`.
- Lists: `staleTime` 30s; reference data (workspaces, projects): 5min.
- `core/session` owns `useMe`, `useWorkspace(slug)`, `usePermissions(slug)`,
  the sign-out reset registry, and the 401 → expired rule.
- Sign-in completion navigates through the server landing URL
  (`window.location.assign(result.location)`): the server already computed
  onboarding / last-workspace / invitation targets the slice does not own.

## Auth card

- Email first; `POST /auth/email-check/` decides password vs code step.
- Code step generates via `POST /auth/magic-generate/` with a 30s resend
  cooldown; both form POSTs follow the redirect convention
  (`error_code` / `error_message` query params on failure).
- Every server code maps to our own banner copy plus the step that can fix
  it (`features/auth/authErrors.ts`); unknown codes get a generic banner.
- Sign-out: `POST /auth/sign-out/` best-effort, then clear every store and
  the whole query cache, then go to sign-in.

## Shell and commands

- `AppShell` is layout + three global wirings: command registration, the
  `mod+k` listener, and the session-expired toast + redirect.
- Screens register their commands while mounted (`registerCommands` in an
  effect; the cleanup unregisters). Global entries (sign-out, go home) are
  registered by the workspace layout.
- Toasts: one kit manager (`shared/shell/toaster.ts`); features call
  `notifySuccess` / `notifyError`.

## Testing

- Unit (`vitest run`): pure logic and components with stub transports.
  Node env by default; DOM suites add `// @vitest-environment jsdom`.
- Contract (`PIDASH_CONTRACT_BASE_URL=…`): api-client suites against a
  scratch Django seeded with `packages/api-client/contract/seed-contracts.py`.
- E2E smoke (`pnpm --filter web_new test:e2e`, env-gated): Playwright,
  sign-in → issue list against a scratch Django + `vite dev`. Skips without
  `PIDASH_E2E_BASE_URL`. The parity harness (NEWFRONT-19) owns CI wiring.

## Dev against local Django

```sh
export PIDASH_API_ORIGIN="http://127.0.0.1:8123"   # on its own line
pnpm --filter web_new dev                            # :3010, /auth + /api proxied
```

The scratch Django must serve the dev origin as its app host, otherwise the
native auth redirects land outside the SPA:

```sh
export WEB_URL="http://localhost:3010" APP_BASE_URL="http://localhost:3010"
```

Two proxy rules earned the hard way: `changeOrigin` stays `false` so the
Host Django sees matches the page Origin (native form views reject
otherwise), and the client drops its cached CSRF token after every
session-changing POST (Django rotates the secret on login, and the stale
token answers a bespoke 200 failure page instead of a 403).

Never point a run at a shared database; the contracts runbook shows how to
create and seed a scratch one.

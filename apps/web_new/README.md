<!-- Copyright (c) Pi Dash contributors | SPDX-License-Identifier: AGPL-3.0-only | See the LICENSE file for details. -->

# web_new — Pi Dash new frontend

Scaffold from NEWFRONT-12. Serves both web and the desktop app (Tauri).
Replaces `apps/web`, `apps/admin` (`/god-mode`) and `apps/space` (`/spaces`)
area by area; the old apps stay running until each area has moved over.

The old frontend is a **reference, never a source**: nothing in these trees
imports `@pi-dash/*`, `apps/web`, `apps/admin`, `apps/space`,
`desktop-overlay` or the edition overlays. The import-ban lint and the
boundary check enforce this — see `checks/check-boundaries.mjs`.

## Install

```sh
pnpm install
```

## Dev

```sh
pnpm --filter web_new dev
```

Dev server runs on port **3010** (`strictPort`: it fails instead of moving).

## Test

```sh
pnpm --filter web_new test
pnpm --filter @pidash/kit test
pnpm --filter @pidash/api-client test
```

Unit tests: Vitest. Contract tests (against local Django) land in NEWFRONT-14:

```sh
# once they exist:
pnpm --filter web_new test:contracts
```

Parity scenarios (Playwright, `e2e/parity`, NEWFRONT-19) run against the
seeded stack from `e2e/parity/stack/README.md`:

```sh
export PARITY_SEED_FILE="$PWD/apps/web_new/e2e/parity/.seed.json"
export PARITY_API_URL=http://localhost:18019
pnpm --filter web_new test:parity:oracle   # apps/web only
pnpm --filter web_new test:parity:new      # apps/web_new only
pnpm --filter web_new test:parity          # both
pnpm --filter web_new parity:report        # inventory x oracle/new report
```

## Build

`pnpm --filter web_new build` produces **both** static bundles:

- `dist/web` — browser build (`PIDASH_TARGET=web`, the default)
- `dist/desktop` — desktop build for the Tauri shell (`PIDASH_TARGET=desktop`)

Single-target builds:

```sh
pnpm --filter web_new build:web
pnpm --filter web_new build:desktop
```

Never set variables inline (`PIDASH_TARGET=... cmd` is dropped by some agent
shells); the build scripts above already encode the target via `--mode`.

## Checks

```sh
pnpm --filter web_new check   # oxlint + boundaries + headers + dep allowlist
pnpm --filter web_new size    # size-limit budgets (Quality gates)
```

Budgets: initial JS ≤ 150 KB gzip, initial CSS ≤ 30 KB gzip.
Raising a budget needs a `process:` comment on NEWFRONT-1 and a human
decision — never raise one in the same PR that exceeds it.

## Layout

```
src/
  main.tsx            bootstrap: target -> router
  target.ts           PIDASH_TARGET parsing (tested)
  routes/             TanStack Router file routes (thin: params, search, loader)
  features/<domain>/  api/ components/ filters/ store.ts index.ts (public API)
  shared/             shell/ editor/ pickers/ commands/ hooks/ lib/
  core/               api/ query/ session/ platform/ edition/ i18n/ theme/ telemetry/
  styles/app.css      Tailwind 4 entry
e2e/parity/           parity scenarios and drivers (NEWFRONT-19)
checks/               check-boundaries.mjs check-headers.mjs check-deps.mjs
LICENSE_HEADER.txt    the ONE file defining the license header (interim
                      AGPL-3.0-only text per F-11/NEWFRONT-22).
                      JSON manifests (package.json, tsconfig, oxlintrc,
                      allowlist) cannot carry it — JSON has no comment syntax —
                      so the header check covers code extensions (ts/tsx/mjs/cjs/css)
```

## Rules for new code

1. A layer imports only from layers below it (`routes > features > shared > core`).
2. A feature imports another feature only through its `index.ts`.
3. Only `core/api` performs network I/O. Components never call `fetch`.
4. Only `core/platform` touches Tauri. No `if (isDesktop)` outside it.
5. `@pidash/kit` has no Pi Dash domain knowledge.
6. Heavy dependencies (editor, charts, drag-and-drop, PDF, emoji, collaboration)
   are only reached through `import()` — never a static import.
7. Adding a runtime dependency needs review: update `allowlist.json`, explain
   in the PR why an existing choice does not work, and note its gzipped size.

# Old frontend at a glance

**Read this when:** you need the size and shape of what is being replaced: to scope an inventory, estimate a split, find where something lives in the old code, or judge whether a number (lines, bundle size) is plausible.

Measured on 2026-09-29 (`pi-dash` at `web-new-dev`, `private-pi-dash` at `9811268`). The old code is frozen to bug fixes, so these numbers change little; re-measure with the command at the bottom.

## Size

About **328,000 lines** (300,000 non-blank) in about **3,700 files**: TS, TSX, JS and CSS source, excluding build output and `.d.ts` files.

| Part | What it is | Files | Lines |
|---|---|---|---|
| `apps/web` | the main app (75 route pages) | 2,234 | 205,314 |
| `apps/space` | public boards (3 route pages) | 168 | 11,578 |
| `apps/admin` | instance admin, god-mode (12 route pages) | 105 | 7,608 |
| `desktop-overlay` | desktop-only overrides for the Tauri build | 24 | 3,769 |
| `packages/propel` | component kit (newer) | 394 | 31,847 |
| `packages/editor` | Tiptap rich-text editor and collaboration | 234 | 22,653 |
| `packages/utils` | helpers | 97 | 10,342 |
| `packages/ui` | component kit (older) | 130 | 8,718 |
| `packages/types` | shared TypeScript types | 121 | 7,402 |
| `packages/services` | axios API services | 74 | 5,748 |
| `packages/constants` | constants | 57 | 4,805 |
| other packages | `i18n`, `shared-state`, `tailwind-config`, `codemods`, `hooks`, `decorators`, `logger` | 59 | 8,258 |
| **Total (public repo)** | | **~3,700** | **328,042** |
| cloud overlay | `private-pi-dash/ee-overlay/apps/{web,admin}` | 41 | 4,878 |

Inside `apps/web`, about 133,000 lines are feature components (`core/components`, ~50 folders), 24,000 are MobX stores (`core/store`), 15,000 are routes (`app/`), 10,000 are edition seams (`ce/`) and 5,500 are services (`core/services`).

## Whose code it is

| | Lines | Share | Rule |
|---|---|---|---|
| Plane-derived (everything present in the initial import of 2026-04-17) | ~314,000 | 96% | reference only: read for behavior, never import or copy |
| AI Republic's own code (runners, runner chat, assistant, schedulers, prompts, agent runtime, AI dev machines, their services and stores, `desktop-overlay`) | ~14,300 | 4% | may be ported and adapted; exact paths on *Reference, not source* |
| Cloud overlay (AI Republic, private repo) | ~4,900 | | may be ported and adapted |

## Tests

Only about **8,400 lines** of tests exist in the old frontend (under 3%). There is little existing coverage to lean on, which is why the parity suite is written first against the old app (the oracle) and then required of the new one.

## Why the new app should be much smaller

A large part of the old code is duplication or hand-built infrastructure:

- **Two component kits** (`ui` and `propel`, ~40,000 lines together), plus four headless UI systems and three positioning libraries underneath. The new app has one kit on Base UI.
- **Two API service layers** (`apps/web/core/services` and `packages/services`, with two different `APIService` base classes). The new app has one `fetch` client with contracts.
- **~26,000 lines of MobX stores** that hand-build caching, loading state and cross-store syncing. In the new app, TanStack Query provides most of that.
- **File-overlay editions** (`ce/`, `ee-overlay/`, `desktop-overlay/`) instead of interfaces.

There is no line-count target for the new app. Parity is the requirement, and the bundle budgets on *Quality gates* are what keep it small.

## Runtime weight (the reason for the rewrite)

From the old desktop build (`desktop/src-tauri/dist`, local build of 2026-09-20):

| | Old app |
|---|---|
| Bundle on disk | 29 MB, ~700 JS chunks |
| JS loaded to open a project's issue list | 245 files, 7.6 MB raw / 2.3 MB gzipped |
| Loaded there but not needed | both editor chunks (1.6 MB + 1.3 MB raw), the charts library (351 KB raw) |
| MobX stores built at startup | ~30 |

Target for the new app: ≤ 150 KB gzipped initial JS (*Quality gates*).

## Where to find things in the old code

| Looking for | Old location |
|---|---|
| A screen | `apps/web/app/(all)/[workspaceSlug]/…/page.tsx` (admin: `apps/admin/app/`, space: `apps/space/app/`) |
| Its UI | `apps/web/core/components/<area>/`, edition variant in `apps/web/ce/components/<area>/` |
| Its data and actions | `apps/web/core/store/<area>*.store.ts` (MobX) |
| Its API calls | `apps/web/core/services/` and `packages/services/src/` |
| Desktop behavior | `desktop-overlay/apps/web/**`, `apps/web/ce/components/desktop/` |
| Cloud behavior | `private-pi-dash/ee-overlay/apps/{web,admin}/**` (read-only sparse clone) |

## Re-measuring

From the `pi-dash` root:

```sh
git ls-files -- apps/web apps/admin apps/space desktop-overlay \
  packages/{ui,propel,editor,utils,constants,types,services,shared-state,hooks,i18n,codemods,decorators,logger,tailwind-config} \
  | grep -E '\.(ts|tsx|js|jsx|mjs|cjs|css|scss)$' | grep -vE '(^|/)(node_modules|dist|build)/|\.d\.ts$|\.min\.' \
  | tr '\n' '\0' | xargs -0 cat | awk 'NF{n++} END{print NR " lines, " n " non-blank"}'
```

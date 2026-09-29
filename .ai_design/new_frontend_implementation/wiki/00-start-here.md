# Start here

**Read this when:** you are new to NEWFRONT, or you need to find which page answers your question. You do not need to read every page; open only the ones your issue touches.

## What we are building

`apps/web_new` is a new React frontend that replaces the Plane-derived frontends: `apps/web`, `apps/admin` (instance admin, becomes `/god-mode/*`) and `apps/space` (public boards, becomes `/spaces/*`). It serves both the web and the Tauri desktop app from one codebase. Old and new run side by side until each area has moved over and passed parity.

Two goals override everything else:

1. **Independent code.** The old frontend is a reference, never a source. Read it to learn behavior; never import it or copy it. See *Reference, not source*.
2. **Full functional parity.** When the project is done, everything the old apps do (OSS, cloud edition, desktop) works in `apps/web_new`. See *Parity*.

Why: the old app loads slowly (about 7.6 MB of JS to open an issue list), it is AGPL and the product is moving off AGPL, and it works poorly inside the desktop app.

## Page map

| Page | Open it when you… |
|---|---|
| Reference, not source | read old code, or write any new code |
| Parity | write inventory rows, parity scenarios, or decide whether a feature is done |
| Architecture | create files, decide where code goes, or add an import |
| Stack and dependencies | add a library, or wonder which library to use |
| Data layer | call the API, fetch or mutate data, add routes, filters or UI state |
| UI kit and editor | build UI, add a kit component, or touch rich text |
| Platform and editions | touch desktop/Tauri behavior or cloud-edition behavior |
| Quality gates | finish an issue: tests, budgets, CI checks, definition of done |
| Phases and migration | need to know what is in scope now, or how routes move from `apps/web` |
| Decisions and open questions | hit a question that looks undecided |

## Where things live

| What | Where |
|---|---|
| New app | `apps/web_new/` (package `web_new`) |
| Design system | `packages/kit/` (`@pidash/kit`) |
| API client and contracts | `packages/api-client/` (`@pidash/api-client`) |
| Feature inventory | `.ai_design/new_frontend_implementation/parity/inventory/<area>.md` |
| Area specs | `.ai_design/new_frontend_implementation/parity/specs/<area>.md` |
| Parity scenarios | `apps/web_new/e2e/parity/<area>/` |
| Full design record | `.ai_design/new_frontend_implementation/design.md` (long; these pages are the working reference) |
| Rulebook and process comments | NEWFRONT-1 |

## Reading pages

`pidash page list --project NEWFRONT` lists pages; `pidash page get <id> --project NEWFRONT` reads one. Record the pages you read, with `updated_at`, in your workpad.

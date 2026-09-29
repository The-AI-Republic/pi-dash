# Phases and migration

**Read this when:** you need to know what is in scope now, or how screens move from `apps/web` to `apps/web_new`.

## Phases

### Phase 0 — Foundation (no user-visible change)

1. Visual design direction: tokens, type, density, shell layout, issue list/board/peek mockups (human-owned).
2. Scaffold `apps/web_new`, `packages/kit`, `packages/api-client` + CI (build, typecheck, lint, size-limit, header check).
3. `@pidash/api-client` core + auth, me, workspace, project, issue (read) contracts + contract-test harness.
4. `core/*`: platform (web, tauri), edition (oss), query (incl. persistence), session.
5. Kit v0: Button, Input, Menu, Dialog, Popover, Tooltip, Toast, Avatar, Badge, Kbd, VirtualList, Skeleton.
6. Shell (title bar, sidebar, command palette, workspace switcher), sign-in, read-only issue list.
7. Desktop build switch behind a flag (`PIDASH_DESKTOP_WEB=web_new`) + smoke test.
8. Parity harness: seeded stack, driver interface, `drivers/web`, parity report; similarity check.
9. Feature inventory for every area, one issue per area, human-reviewed.
10. Oracle scenarios for Phase 1 areas, green on `apps/web`.
11. Baseline measurements; freeze `apps/web`.

**Exit:** a signed-in user opens a project's issue list in web and desktop builds; budgets and similarity check green; full inventory reviewed; Phase 1 oracle scenarios green.

### Phase 1 — Daily issue workflow

Auth and onboarding, shell, home, projects list, command palette, search, issues (all layouts, detail, peek, filters, display options, bulk edit, sub-issues, relations, links, attachments, reactions, subscriptions), comments, activity, mentions, drafts. Oracle scenarios for Phase 2 areas are written during this phase.

**Exit:** all Phase 1 rows green on `apps/web_new`.

### Phase 2 — Pi Dash core + desktop switch

Runners, runs, approvals, runner chat (SSE via `platform.stream`), AI dev machines, schedulers, prompts, assistant, notifications, desktop features (agent runtime, deep links, native menus, updater). Switch the desktop build to `apps/web_new`; delete `desktop-overlay/` and `merge-web-tree.sh`. Oracle scenarios for Phase 3 areas are written during this phase.

**Exit:** desktop gate passes (all Phase 1–2 rows and desktop rows green); desktop ships on `apps/web_new`.

### Phase 3 — Everything else + web cutover

Views, archives, intake, cycles, modules, pages (with collaboration), estimates, analytics, stickies, exports, all settings, profile, and the cloud edition module in `private-pi-dash`. Each area moves on the web as soon as it passes its gate.

**Exit:** every area served by `apps/web_new`; every inventory row green.

### Phase 4 — Removal

After the final gate and a human acceptance pass: remove `apps/web`, legacy route handling no longer needed, and old packages no longer used by `apps/admin` or `apps/space`. The parity suite keeps running on `apps/web_new` as its regression suite.

## Coexistence

- **Web:** `apps/proxy` Caddyfiles route migrated path prefixes to `apps/web_new` and everything else to `apps/web`. Same origin and session cookie; crossing between apps is a full page load. The migrated-route list is one file per deployment and grows each phase.
- **Links to unmigrated screens** from `apps/web_new` are plain `<a href>` generated from the same route list, so they flip automatically when a screen moves.
- **Desktop:** switches at the end of Phase 2; screens not yet moved open in the system browser via `platform.openExternal`.
- **Backend:** unchanged. If a feature needs a backend change, file it in PDASHOSS01 (OSS) or PRIVATEPI1 (cloud) and relate it as a blocker.

## Freeze policy

From the end of Phase 0, `apps/web` takes bug fixes and security fixes only. New features are built only in `apps/web_new`. A bug fixed in `apps/web` during the transition gets an inventory row update (or a new row) so `apps/web_new` does not reintroduce it.

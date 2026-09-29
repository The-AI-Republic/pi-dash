# Parity

**Read this when:** you write inventory rows or parity scenarios, implement an area, or need to decide whether something is done.

**Everything `apps/web` does today, `apps/web_new` must do when the project is done.** No area is dropped. This covers the OSS build, the cloud edition (`private-pi-dash/ee-overlay/apps/web`) and the desktop build (`desktop-overlay/`). The UI may be redesigned; capabilities, rules and outcomes may not be lost.

Parity is not judged by eye. It is defined by an **inventory** and proven by a **parity suite**.

## Areas

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
| Desktop: agent runtime, bare sign-in, updater, deep links | `desktop-overlay/`, `apps/web/ce/components/desktop` | 2 |
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
| Project settings | `settings/projects/[projectId]/*` | 3 |
| Workspace settings (incl. billing, API tokens, webhooks, integrations) | `settings/(workspace)/*` | 3 |
| Profile, account, appearance, notification preferences | `profile/[userId]`, `settings/account`, `settings/profile/*` | 3 |
| Cloud edition: home, docs, downloads, pricing, login, apps, profile tabs | `ee-overlay/apps/web/app/**` | 3 |

`apps/admin` and `apps/space` are out of scope unless decided otherwise.

## Feature inventory

File: `.ai_design/new_frontend_implementation/parity/inventory/<area>.md`. One row per capability:

| Field | Example |
|---|---|
| ID | `ISS-042` (area prefix + number; never reused) |
| Capability | Bulk-change state of selected issues in list layout |
| Who | member+, not guest |
| Edition | oss / cloud / desktop / all |
| Old entry point | `projects/…/issues` list, selection bar |
| API | `POST /api/workspaces/{ws}/projects/{pid}/bulk-operation-issues/` |
| Acceptance | all selected issues move; activity entry per issue; list regroups |
| Parity test | `parity/issues/bulk-state.spec.ts` |
| Status | not started / oracle green / new green |

Build rows from the code **and** the running app. Give these their own rows; they are easy to miss:

- permissions per role (admin, member, guest) and what each role cannot do
- keyboard shortcuts
- empty, loading and error states that carry behavior (e.g. a retry, a CTA)
- URL and deep-link behavior (shareable filters, direct links to an issue)
- exports, imports, drag-and-drop
- real-time updates (what refreshes without reload)
- settings that change other screens (e.g. project features toggling cycles/modules)
- edition-only and desktop-only behavior

If you find a behavior no row covers, add a row with the next ID and note where you found it. A human reviews each area's inventory before implementation starts.

## Parity suite

- Scenarios live in `apps/web_new/e2e/parity/<area>/`, one or more per inventory ID, tagged with the ID.
- Scenarios are written against a **driver interface** of user-level actions and reads (`createIssue`, `setState`, `applyFilter`, `openSettings`, `listVisibleIssues`, …). Two drivers implement it: `drivers/web` (targets `apps/web`) and `drivers/web_new`. The UI can differ; the scenario is the same.
- Assertions check both what the user sees (through the driver) and the resulting server state (through the API). A redesigned screen cannot pass by looking right while saving the wrong thing.
- **Order:** a scenario is made green on `apps/web` first (the oracle; this proves the scenario is correct). Then the `apps/web_new` implementation must make it green.
- Runs against a seeded local stack (Django + Postgres + `apps/live`), both editions, and the desktop build for desktop rows.
- CI publishes a parity report: per area, inventory rows × {oracle green, new green}.

**Old bugs:** if the oracle behavior is a bug, do not encode it silently. Mark the scenario `bug:` with a linked issue, and record the intended behavior in the row. `apps/web_new` may implement the fix.

## Gates

| Gate | Condition |
|---|---|
| An area's implementation may start | its inventory is human-reviewed and its oracle scenarios are green on `apps/web` |
| An area's routes move to `apps/web_new` (Caddy / desktop) | 100% of the area's rows green on `apps/web_new` |
| Desktop switches to `apps/web_new` | all Phase 1–2 areas and all desktop rows green |
| `apps/web` is removed | 100% of all rows green for both editions and desktop, plus a human acceptance pass |

An issue is Done only when every inventory row it names is green on `apps/web_new`.

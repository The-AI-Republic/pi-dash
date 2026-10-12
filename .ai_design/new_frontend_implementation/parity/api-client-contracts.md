# `@pidash/api-client` contracts — runbook (NEWFRONT-14)

One test per contract, each parsing a real response from a live local Django
(`apps/api`) seeded with `packages/api-client/contract/seed-contracts.py`.
Unit tests (`src/*.test.ts`) run everywhere with stub transports; the
contract suites (`src/contract/*.contract.test.ts`) skip unless
`PIDASH_CONTRACT_BASE_URL` is set, so CI stays green.

## Contracts and suites

| Contract                  | Function                                                                                  | Suite                                                                                                                                                                          |
| ------------------------- | ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| CSRF token                | `getCsrfToken`                                                                            | `auth.contract.test.ts` — token parses and is non-empty                                                                                                                        |
| Sign-in                   | `signIn`                                                                                  | `auth.contract.test.ts` — wrong password returns `ok:false` with a code; seeded credentials return `ok:true` and the session reads back `me`                                   |
| Email check               | `checkEmail`                                                                              | `auth.contract.test.ts` — seeded address reports `existing:true` with its login mode; unknown reports `existing:false`                                                         |
| Magic code                | `generateMagicCode`, `signInWithMagicCode`                                                | unit stubs (`contracts.test.ts`) prove the JSON shape and the shared redirect convention; live positive flow needs SMTP + redis, covered by the sign-in card in the E2E smoke  |
| Sign-out                  | `signOut`                                                                                 | `auth.contract.test.ts` — sign in, sign out, the session no longer reads back `me`                                                                                             |
| Current user              | `getMe`, `getMeSettings`                                                                  | `users.contract.test.ts` — email matches the seeded user; settings point at the seeded workspace                                                                               |
| Workspaces                | `listWorkspaces`                                                                          | `users.contract.test.ts` — seeded slug present with role and member count                                                                                                      |
| Projects                  | `listProjects`                                                                            | `projects.contract.test.ts` — seeded identifier present                                                                                                                        |
| States                    | `listStates`                                                                              | `projects.contract.test.ts` — non-empty, one default                                                                                                                           |
| Labels                    | `listLabels`                                                                              | `projects.contract.test.ts` — `contract-bug` present                                                                                                                           |
| Members                   | `listMembers`                                                                             | `projects.contract.test.ts` — admin membership present                                                                                                                         |
| Issue list                | `listIssues`                                                                              | `issues.contract.test.ts` — ungrouped envelope, seeded rows parse                                                                                                              |
| Issue detail              | `getIssue`                                                                                | `issues.contract.test.ts` — list-then-read round trip                                                                                                                          |
| Home dashboard            | `getWorkspaceDashboard`                                                                   | `home.contract.test.ts` — counts parse, buckets are arrays                                                                                                                     |
| Home preferences          | `listHomePreferences`, `updateHomePreference`                                             | `home.contract.test.ts` — seeded keys present; flag flips and restores                                                                                                         |
| Quick links               | `listQuickLinks`, `createQuickLink`, `getQuickLink`, `updateQuickLink`, `deleteQuickLink` | `home.contract.test.ts` — full CRUD round trip, delete resolves 204                                                                                                            |
| Recent visits             | `listRecentVisits`                                                                        | `home.contract.test.ts` — unfiltered and `entity_name=issue` narrowing                                                                                                         |
| Tour flag                 | `completeTour`                                                                            | `home.contract.test.ts` — PATCH true then false, server message reads back                                                                                                     |
| Project details           | `listProjectDetails`                                                                      | `projects.contract.test.ts` — seeded row carries annotations, `archived_at` null                                                                                               |
| Project create/delete     | `createProject`, `deleteProject`                                                          | `projects.contract.test.ts` — throwaway project round trip, delete resolves 204 (unique name/identifier per run: delete is soft and the backend reserves both)                 |
| Project archive           | `archiveProject`, `unarchiveProject`                                                      | `projects.contract.test.ts` — stamp parses, unarchive resolves 204                                                                                                             |
| Project leave             | `leaveProject`                                                                            | `projects.contract.test.ts` — second admin leaves (204) and rejoins; only admin refuses with 400 (seeded probe user `contract.probe@example.com`)                              |
| Project invitations       | `listProjectInvitations`, `joinProjects`                                                  | `projects.contract.test.ts` — invite list parses; join by ids answers the success message                                                                                      |
| Favorite projects         | `listFavoriteProjects`, `addFavoriteProject`, `removeFavoriteProject`                     | `projects.contract.test.ts` — add/list/remove round trip; GET list is red live on backend bug NEWFRONT-384 (viewset lacks `serializer_class`, GET 500s; POST/DELETE 204 green) |
| Workspace user-properties | `getWorkspaceUserProperties`, `updateWorkspaceUserProperties`                             | `projects.contract.test.ts` — limit flips and restores                                                                                                                         |
| Project user-properties   | `getProjectUserProperties`, `updateProjectUserProperties`                                 | `projects.contract.test.ts` — sort order bumps and restores                                                                                                                    |
| Issue update      | `updateIssue`                                                                              | `updates.contract.test.ts` — PATCH answers 204; rename proven by re-read, then restored (thin shell slice; full ownership in issues area)                                     |
| Subscription      | `getSubscriptionStatus`, `subscribeIssue`, `unsubscribeIssue`                              | `updates.contract.test.ts` — status false → subscribe → status true → unsubscribe → status false                                                                              |
| Sidebar prefs     | `getSidebarPreferences`, `updateSidebarPreferences`                                        | `sidebar.contract.test.ts` — every known row present with pinned/order; patch-and-restore round trip                                                                          |
| Favorites         | `listFavorites`, `createFavorite`, `updateFavorite`, `deleteFavorite`, `listFavoriteGroup` | `sidebar.contract.test.ts` — folder + entry create, group children, move/reorder PATCH, delete                                                                                |
| Workspace search  | `searchWorkspace`                                                                          | `search.contract.test.ts` — seeded issue found across entities; `entities` param scopes the answer                                                                            |
| Profile           | `getProfile`, `updateProfile`                                                              | `profile.contract.test.ts` — theme + language read; patch-and-restore round trip                                                                                              |
| User timezone     | `updateMyTimezone`                                                                         | `profile.contract.test.ts` — PATCH timezone and restore; `Me` read proves the starting value                                                                                  |
| Timezones         | `listTimezones`                                                                            | `profile.contract.test.ts` — non-empty; Europe/Paris present                                                                                                                  |
| Assistant threads | `listAssistantThreads`, `createAssistantThread`                                            | `assistant.contract.test.ts` — create then list-contains                                                                                                                      |
| Model config      | `getLlmConfig`                                                                             | `assistant.contract.test.ts` — blank-until-configured shape (SHELL-007 check)                                                                                                 |
| Unread count      | `getUnreadNotifications`                                                                   | `notifications.contract.test.ts` — total + mention counts (thin shell slice; full ownership in notifications area)                                                            |
| Cycle update      | `updateCycle`                                                                              | `updates.contract.test.ts` — rename applies, echo parses, then restored (thin shell slice; full ownership in cycles area)                                                     |
| Module update     | `updateModule`                                                                             | `updates.contract.test.ts` — rename applies, echo parses, then restored (thin shell slice; full ownership in modules area)                                                    |
| Page update       | `updatePage`                                                                               | `updates.contract.test.ts` — rename applies, echo parses, then restored (thin shell slice; full ownership in pages area)                                                      |

## From a clean checkout

Prerequisites: the JS toolchain (`pnpm install` at the repo root) and a
Python environment with the `apps/api` requirements installed.

```sh
# 1. Create a scratch database (never reuse a shared one).
createdb pidash_newfront14

# 2. Migrate and seed it. The script prints its target first, refuses
#    prod-like names, and writes nothing without --apply.
cd apps/api
export DATABASE_URL="postgres://<user>@localhost:5432/pidash_newfront14"
export REDIS_URL="redis://localhost:6379/0"
export AMQP_URL="redis://localhost:6379/1"
export WEB_URL="http://localhost:3000" APP_BASE_URL="http://localhost:3000"
export DJANGO_SETTINGS_MODULE="pi_dash.settings.local"
python manage.py migrate
python ../../packages/api-client/contract/seed-contracts.py
python ../../packages/api-client/contract/seed-contracts.py --apply

# 3. Serve it on a port of your own (do not reuse another run's port).
python manage.py runserver 127.0.0.1:8123 --noreload

# 4. Run the contract suites against it (from the repo root).
export PIDASH_CONTRACT_BASE_URL="http://127.0.0.1:8123"
pnpm --filter @pidash/api-client test
```

Expected: unit tests pass and all contract tests run (not skip) and pass — except the favorite-list GET, which stays red until backend bug NEWFRONT-384 is fixed (the table above is the count).
Unset `PIDASH_CONTRACT_BASE_URL` returns the package to unit-only mode.

## Notes

- The suites sign in through the real form flow, so they prove the session
  cookie jar in `src/contract/setup.ts` end to end. The jar walks
  redirects itself because fetch hides the intermediate `Set-Cookie`
  headers (the sign-in 302 carries the session) — a browser keeps those.
- `AMQP_URL` must point at a reachable broker (local redis works; no
  worker needed): the issue list queues a visit-tracking task and 500s
  when the broker refuses connections.
- Seeded rows use a `contract-` prefix and `get_or_create`, so re-seeding is
  safe. The seed user password defaults to `Contract123!` and can be
  overridden with `PIDASH_CONTRACT_PASSWORD` (mirrored to the test run).
- Validation is always on in tests (`validate: true` in the harness);
  production builds skip it (`NODE_ENV=production`).

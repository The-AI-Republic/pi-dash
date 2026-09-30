# `@pidash/api-client` contracts — runbook (NEWFRONT-14)

One test per contract, each parsing a real response from a live local Django
(`apps/api`) seeded with `packages/api-client/contract/seed-contracts.py`.
Unit tests (`src/*.test.ts`) run everywhere with stub transports; the
contract suites (`src/contract/*.contract.test.ts`) skip unless
`PIDASH_CONTRACT_BASE_URL` is set, so CI stays green.

## Contracts and suites

| Contract     | Function                 | Suite                                                                                                                                        |
| ------------ | ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------- |
| CSRF token   | `getCsrfToken`           | `auth.contract.test.ts` — token parses and is non-empty                                                                                      |
| Sign-in      | `signIn`                 | `auth.contract.test.ts` — wrong password returns `ok:false` with a code; seeded credentials return `ok:true` and the session reads back `me` |
| Current user | `getMe`, `getMeSettings` | `users.contract.test.ts` — email matches the seeded user; settings point at the seeded workspace                                             |
| Workspaces   | `listWorkspaces`         | `users.contract.test.ts` — seeded slug present with role and member count                                                                    |
| Projects     | `listProjects`           | `projects.contract.test.ts` — seeded identifier present                                                                                      |
| States       | `listStates`             | `projects.contract.test.ts` — non-empty, one default                                                                                         |
| Labels       | `listLabels`             | `projects.contract.test.ts` — `contract-bug` present                                                                                         |
| Members      | `listMembers`            | `projects.contract.test.ts` — admin membership present                                                                                       |
| Issue list   | `listIssues`             | `issues.contract.test.ts` — ungrouped envelope, seeded rows parse                                                                            |
| Issue detail | `getIssue`               | `issues.contract.test.ts` — list-then-read round trip                                                                                        |

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

Expected: unit tests pass and the 12 contract tests run (not skip) and pass.
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

# Parity seeded stack (runbook)

Scratch backend for parity runs. One command brings it up, one resets it.
Nothing here is shared: dedicated `parity19-*` containers, ports, and the
`parity19_pgdata` volume, so a reset can never touch dev data or another
run's database.

## Services

| Service  | Image                                                                     | Port           | Notes                                                                                       |
| -------- | ------------------------------------------------------------------------- | -------------- | ------------------------------------------------------------------------------------------- |
| `pg`     | postgres:15.7-alpine                                                      | 15419          | Scratch database `pidash`, user `parity19`                                                  |
| `redis`  | redis:7-alpine                                                            | 16319          | Django cache backend                                                                        |
| `mq`     | rabbitmq:3-management-alpine                                              | none published | Broker for background tasks                                                                 |
| `api`    | built from `apps/api` (`Dockerfile.dev`, lean migrate-plus-serve command) | 18019          | Django + uvicorn, migrates on boot                                                          |
| `worker` | same image as `api` (celery worker, default queue)                        | none published | Consumes background tasks: mention links, notifications                                     |
| `oracle` | built from the repo (`apps/web/Dockerfile.dev`)                           | internal :3000 | Old app with same-origin API calls                                                          |
| `proxy`  | caddy:2-alpine (`stack/Caddyfile`)                                        | 13000          | One origin: frontend plus `/auth`, `/api`, `/static` to Django, `/parity19-assets` to minio |
| `minio`  | minio/minio (bucket `parity19-assets`, created at boot)                   | 19019          | Object storage backing presigned asset uploads                                              |
| `live`   | built from `apps/live` (opt-in profile `full`)                            | 13001          | Realtime server for realtime rows                                                           |

All ports move through `PARITY_PG_PORT`, `PARITY_REDIS_PORT`,
`PARITY_API_PORT`, `PARITY_ORACLE_PORT`, `PARITY_MINIO_PORT`, `PARITY_LIVE_PORT`. The defaults
are the contract the Playwright config assumes (`PARITY_ORACLE_URL`
defaults to the proxy on `:13000`, `PARITY_NEW_URL` to web_new on `:3010`,
`PARITY_API_URL` to `:18019`).

The api service runs on `parity_scratch_settings.py` (mounted read-only),
which widens only the default anonymous throttle to 600/minute: every
scenario shares one IP bucket with the frontend's own loader calls, so the
production 30/minute budget saturates minutes into an oracle run. The
authentication throttle (30/minute on email-check / magic / forgot) is
untouched, so rate-limit oracles still trip it.

## Bring up

From the repo root:

```sh
apps/web_new/e2e/parity/stack/parity-up.sh
```

This builds `parity19-api:local`, starts pg/redis/mq/api, waits for the
API, and seeds the deterministic parity workspace (one onboarded owner,
one project, three named issues). Seed facts land in
`apps/web_new/e2e/parity/.seed.json` (generated, never committed);
scenarios read it through `PARITY_SEED_FILE`.

## Reset

```sh
apps/web_new/e2e/parity/stack/parity-reset.sh
```

Drops the scratch volume and rebuilds through `parity-up.sh`. The script
prints its target first; if the names ever stop matching this stack, stop
and ask a human instead of proceeding.

## Oracle build: dev or production

By default the oracle is the old app's dev server (`oracle`). It compiles
pages on demand, so it is the heaviest service in the stack: several hundred
MB of memory and most of the stack's CPU while a suite runs.

```sh
PARITY_ORACLE_MODE=prod apps/web_new/e2e/parity/stack/parity-up.sh
```

serves the same code as a production build instead (`oracle-prod`: built by
`Dockerfile.oracle-prod`, static files behind nginx). It uses a few MB and
almost no CPU, and Docker reuses its layers until the old app's own sources
change, so only the first bring-up pays for the build.

The two builds are not identical to drive. The dev server runs React in
StrictMode and without minification; a few recorded old-app bugs exist only
there. Scenarios were made green against the dev oracle, so treat `prod` as
opt-in until the suite has been confirmed against it.

## Teardown

```sh
apps/web_new/e2e/parity/stack/parity-down.sh
```

Removes the stack's containers, network and scratch volumes (images stay,
so the next bring-up is fast). Run it when you are done.

Inside a Pi Dash agent run you do not have to remember: `parity-up.sh` arms
`parity-reaper.sh`, a detached watcher that runs the teardown when the
run's agent process exits, however it exits. The stack therefore lives for
one run; the next run brings it up again. Set `PARITY_KEEP_STACK=1` before
`parity-up.sh` to keep a stack across runs. Outside an agent run (your own
shell, CI) nothing is armed.

Every service has a `mem_limit` in the compose file, about 6 GB for the
whole stack at the limits and well under 1.5 GB in normal use. If a
container is killed at its limit, raise that one limit rather than removing
it.

## Realtime

Most scenarios need only the API. When a scenario covers realtime rows,
start the live service too:

```sh
docker compose -f apps/web_new/e2e/parity/stack/docker-compose.yml --profile full up -d live
```

The document editor needs the live endpoint to become editable. The
oracle reads it from `VITE_LIVE_BASE_URL`, which the compose file
derives from `PARITY_LIVE_PORT` when set (empty otherwise, keeping the
window-origin fallback): export the slot's live port — e.g.
`PARITY_LIVE_PORT=13089` — and (re)create the oracle so the dev server
picks it up. Known wrinkle (NEWFRONT-188): the compose `live` image
currently fails to build (pre-existing propel DTS error); until that is
fixed, run live natively — build the workspace packages with
`tsdown --no-dts`, write an `apps/live/.env` for the slot
(`PORT`, `API_BASE_URL`, `LIVE_BASE_PATH=""`,
`LIVE_SERVER_SECRET_KEY`, slot `REDIS_URL`, slot
`CORS_ALLOWED_ORIGINS`), build `apps/live`, and start
`node --env-file=.env dist/start.mjs` on the slot's live port.

## Frontends

The stack serves the oracle behind one origin (port 13000): an edge proxy
mirrors the production topology, routing `/auth`, `/api` and `/static` to
Django and everything else to the old app, so the native credential POST
and the session cookie behave exactly like deployed. That address is the
Playwright `oracle` base URL.

The new app is not served by this stack; run its dev server yourself:

```sh
pnpm --filter web_new dev   # :3010, the Playwright `new` base URL
```

## Editions

This checkout runs the OSS backend. Where the cloud overlay exists, the
same scripts serve it: check out the edition tree and rerun `parity-up.sh`
(the image build picks up the overlay). Desktop rows run against the
desktop build of web_new with the same seeded stack.

## Seed contents

`seed/seed_parity.py` runs inside the api container through
`manage.py shell`, so the backend tree stays untouched. It rebuilds:

- user `parity-oracle@example.com` (password `Parity-Seed-1`),
  verified and onboarded, so no gate funnels scenarios away;
- workspace `Parity Workspace` (`parity-ws`), owned by that user;
- project `Parity Project` (`PAR`), flat list layout preference;
- one `Todo` state plus three issues: `Parity first/second/third issue`.
- second member `parity-mention@example.com` (password `Parity-Seed-2`,
  display name `Parity Mention`), workspace + project member, so mention
  scenarios have someone to @-mention besides the author.

To change the seed, edit that file and rerun `parity-up.sh`.

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

## Realtime

Most scenarios need only the API. When a scenario covers realtime rows,
start the live service too:

```sh
docker compose -f apps/web_new/e2e/parity/stack/docker-compose.yml --profile full up -d live
```

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

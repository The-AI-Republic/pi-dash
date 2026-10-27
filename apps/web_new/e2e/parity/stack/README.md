# Parity seeded stack (runbook)

Scratch backend for parity runs. One command brings it up, one resets it.
Nothing here is shared: dedicated containers, ports, and volumes, so a
reset can never touch dev data or another run's database. Parallel runs
on one host each take a namespace — see "Concurrent runs" below.

## Services

| Service        | Image                                                                              | Port           | Notes                                                                                       |
| -------------- | ---------------------------------------------------------------------------------- | -------------- | ------------------------------------------------------------------------------------------- |
| `pg`           | postgres:15.7-alpine                                                               | 15419          | Scratch database `pidash`, user `parity19`                                                  |
| `redis`        | redis:7-alpine                                                                     | 16319          | Django cache backend                                                                        |
| `mq`           | rabbitmq:3-management-alpine                                                       | none published | Broker for background tasks                                                                 |
| `api`          | built from `apps/api` (`Dockerfile.dev`, lean migrate-plus-serve command)          | 18019          | Django + uvicorn, migrates on boot                                                          |
| `worker`       | same image as `api` (celery worker, default queue)                                 | none published | Consumes background tasks: mention links, notifications                                     |
| `oracle`       | built from the repo root (`stack/Dockerfile.oracle`, old-app inputs only)          | internal :3000 | Old app with same-origin API calls                                                          |
| `oracle-admin` | built from the repo root (`stack/Dockerfile.oracle-admin`, admin inputs only)      | internal :3000 | Old admin console, served under `/god-mode` (prod variant `oracle-admin-prod`)              |
| `proxy`        | caddy:2-alpine (`stack/Caddyfile`)                                                 | 13000          | One origin: frontend plus `/auth`, `/api`, `/static` to Django, `/parity19-assets` to minio |
| `minio`        | bitnamilegacy/minio, pinned tag+digest (bucket `parity19-assets`, created at boot) | 19019          | Object storage backing presigned asset uploads                                              |
| `live`         | built from `apps/live` (opt-in profile `full`)                                     | 13001          | Realtime server for realtime rows                                                           |

All ports move through `PARITY_PG_PORT`, `PARITY_REDIS_PORT`,
`PARITY_API_PORT`, `PARITY_ORACLE_PORT`, `PARITY_MINIO_PORT`, `PARITY_LIVE_PORT`. A namespace
derives them (per-service base plus its last two digits: namespace 257
gets `:15457`, `:16357`, `:18057`, `:13057`, `:19057`, `:13157`); unset,
the legacy defaults below apply, which is the contract the Playwright
config assumes (`PARITY_ORACLE_URL` defaults to the proxy on `:13000`,
`PARITY_NEW_URL` to web_new on `:3010`, `PARITY_API_URL` to `:18019`).

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

This builds the scratch images, starts pg/redis/mq/api (plus worker,
oracle and proxy), waits for the API, seeds the deterministic parity
workspace (one onboarded owner, one project, three named issues), and
warms the oracle's entry routes. Seed facts land in
`apps/web_new/e2e/parity/.seed.json` under the default stack, or
`.seed-<ns>.json` under a namespace (generated, never committed);
scenarios read the file through `PARITY_SEED_FILE`, which `parity-up.sh`
prints, along with the other suite exports, when it finishes. A preflight
check refuses to start when another stack (or any foreign process)
already holds one of this stack's ports — pick a free namespace instead
of forcing it.

Never pipe bring-up (or reset) to `tail`: the pipe's exit status is
`tail`'s, so a failed bring-up looks green. Run the script bare, or with
`set -o pipefail`, or log to a file.

Image builds reuse cached layers: a rerun rebuilds an image only when its
Dockerfile or its copied inputs changed. The oracle Dockerfile copies just
the old app's inputs (`apps/web`, the workspace packages it resolves, the
root workspace files), so edits elsewhere — `apps/web_new`, the new
packages, `apps/api`, e2e specs — never rebuild or recreate the oracle
container. Edits to the old app itself, its packages, or the shared
`pnpm-lock.yaml` still rebuild it, as they should; the install step is
frozen, so even a cache miss recomputes the identical image instead of
drifting with the registry (if it ever fails with an outdated-lockfile
error, run `pnpm install` on the host first). For a clean no-cache
rebuild of both images:

```sh
apps/web_new/e2e/parity/stack/parity-up.sh --rebuild
```

The warm-up step loads the entry route, one seeded project route, and
the admin console entry (`/god-mode/`) through the proxy and waits until
all are served. The dev-server oracle
pays its dependency-optimization spike on those first requests; warming
here means the suite's first specs never meet a cold boot. If the oracle
cannot serve within ten minutes the script fails with a pointer to its
logs instead of letting the suite flake — on a loaded host that usually
means the dev server was OOM-killed (see NEWFRONT-133). On the production
oracle the step passes instantly.

## Reset

```sh
apps/web_new/e2e/parity/stack/parity-reset.sh [project]
```

Drops this stack's scratch volumes and rebuilds through `parity-up.sh`.
It honors `PARITY_NS` like the other scripts, and prints its target
(project, containers, volumes) first; a pre-down assertion also refuses
when the target project owns foreign-named containers. If the names ever
stop matching this stack, stop and ask a human instead of proceeding.
Pass the project explicitly whenever sudo is involved (see "Slots and
sudo" below).

## Concurrent runs

Every oracle run on a shared host exports one setting before anything
else:

```sh
export PARITY_NS=257   # your issue number (or slot number); see the rules below
```

That derives the compose project (`parity257`), the container names, the
six published ports, the volumes (compose prefixes them with the
project, so each namespace gets its own postgres datadir), the seed file
(`.seed-257.json`) and the suite URLs — for the shell scripts and the
suite alike. Bring the stack up, export the block `parity-up.sh` prints
at the end, run the specs; `parity-reset.sh` and `parity-down.sh` target
the same namespace. Anything set explicitly (`PARITY_PROJECT`,
`PARITY_CONTAINER_PREFIX`, any `PARITY_*_PORT`, `PARITY_SEED_FILE`,
`PARITY_API_URL`, `PARITY_ORACLE_URL`, `PARITY_API_CONTAINER`,
`PARITY_REDIS_CONTAINER`) wins over the derivation.

Locally-built image tags are namespaced the same way (NEWFRONT-435):
`api`, `worker`, `oracle`, `oracle-prod`, `live`, `api-cloud` and
`oracle-cloud-prod` build as `<prefix>-<service>:local` (`worker` reuses
the `api` image), where the prefix is the resolved
`PARITY_CONTAINER_PREFIX` (default `parity19`), so a build in one
namespace never retags the image a sibling namespace runs. The first
bring-up in a fresh namespace builds its own set of images; after that
Docker reuses their layers exactly as before.

Rules:

- Numeric namespaces only for the one-setting flow. A non-numeric
  `PARITY_NS` still namespaces the project and seed file, but its ports
  must all be set explicitly. Slot `sN` maps to `PARITY_NS=N`.
- Namespaces sharing last-two-digits share ports: 157 and 257 collide.
  The preflight check fails fast with the busy ports; pick a free one
  (`docker ps` shows who owns what). Suffixes `00` and `19` are refused
  outright — they reproduce the default stack's own ports.
- Bring-up and reset refuse when the namespace's project already has
  containers from another checkout (docker records which checkout
  created each container, and the guard compares it). Reruns from the
  owning checkout stay idempotent. Namespaces are disjoint, so no
  locking is needed across them; two concurrent `parity-up.sh` runs for
  the SAME namespace fail loudly on container names instead.
  `parity-down.sh` only warns on foreign ownership and proceeds — it is
  the deliberate override for orphaned stacks.
- Adopting a namespace for a checkout that used hand-set `PARITY_PROJECT`
  - ports recreates its containers (same project, same volumes, so the
    seeded data survives) and may move its ports to the derived ones —
    re-export the printed block and carry on.
- Bare `docker compose -f stack/docker-compose.yml ...` commands do not
  resolve namespaces: prefix them with the same environment (at least
  `PARITY_PROJECT`, or `-p`) the scripts would use.

## Oracle build: dev or production

By default the oracle is the old app as a production build (`oracle-prod`:
built by `Dockerfile.oracle-prod`, static files behind nginx). It is also
the memory-safe choice: it uses a few MB and almost no CPU, and Docker
reuses its layers until the old app's own sources change, so only the
first bring-up pays for the build. The suite was confirmed green against
it (NEWFRONT-246).

```sh
PARITY_ORACLE_MODE=dev apps/web_new/e2e/parity/stack/parity-up.sh
```

brings up the dev server instead (`oracle`), which compiles pages on demand:
several hundred MB of memory and most of the stack's CPU while a suite runs.
Use it when triaging a failure that only reproduces there — the dev server
runs React in StrictMode and without minification, and a few recorded
old-app bugs exist only in that mode.

Keep the production oracle as the default. On a small host (the shared
runner's container runtime is capped at roughly 3.8 GB) the dev server's
one-time dependency-bundling burst can exhaust memory and be killed before
it serves a single request, which then looks like a scenario failure but is
not (NEWFRONT-133). `check-oracle-build.mjs` fails the build if a bare
bring-up stops selecting `oracle-prod`, so a silent flip back to the dev
server cannot land unnoticed.

One switch selects both frontends: the admin oracle (`oracle-admin-prod`
by default, `oracle-admin` under `PARITY_ORACLE_MODE=dev`) always matches
the web oracle's mode, so runs never mix a prod web oracle with a dev
admin oracle (NEWFRONT-441). The cloud edition selects its own admin
oracle instead (`oracle-admin-cloud-prod` — see "Cloud oracle" below),
so the admin entry gate always matches the edition.

## Admin console oracle

The old admin console (`apps/admin`) serves through the same proxy origin
under `/god-mode`, mirroring the production topology
(`apps/proxy/Caddyfile.ce`: bare `/god-mode` redirects to `/god-mode/`,
and `/god-mode/*` proxies to the admin service with the full path, which
the app's `/god-mode` basename routes on). Same-origin serving is what
makes the native credential POST and the session cookie work for admin
scenarios, exactly like the web oracle.

- Suite address: `PARITY_ORACLE_ADMIN_URL` (printed by `parity-up.sh`,
  derived as `<oracle>/god-mode`, overridable explicitly). The suite
  helper `parityOracleAdminUrl` resolves the same value.
- No new published ports: the admin mount rides the proxy, so the
  namespace derivation, the guard, and the six-port contract are
  unchanged.
- Upstream selection: both admin oracles share the network alias
  `oracle-admin-upstream`, which is what the proxy dials — so no
  per-mode variable has to cross into compose (new variables do not
  survive hosts where docker runs behind an env-scrubbing sudo shim).
  `parity-up.sh` stops the non-selected admin oracle so the alias never
  splits traffic after a mode switch.
- Native override: point `ORACLE_ADMIN_UPSTREAM` at
  `host.docker.internal:<port>` to serve a natively run admin frontend
  through the proxy, mirroring `ORACLE_UPSTREAM` (needs a docker that
  forwards the variable to compose).
- Seed identity: the seed owner (`PARITY_SEED_JSON` email/password) is
  also an instance admin (verified), so admin scenarios sign in with the
  seed credentials — no separate admin identity is seeded. The instance
  ships set-up (`is_setup_done=True`); setup-form scenarios flip it off
  per-scenario through the API helpers and restore it after.

## Teardown

```sh
apps/web_new/e2e/parity/stack/parity-down.sh [project]
```

Removes the stack's containers, network and scratch volumes (images stay,
so the next bring-up is fast). It honors `PARITY_NS`: it tears down your
namespace's stack, never a sibling's. As with reset, the script prints
its target and asserts container names before destroying; pass the
project explicitly whenever sudo is involved (see "Slots and sudo"
below). Run it when you are done.

## Slots and sudo

Parallel runs on one daemon isolate by namespace: export `PARITY_NS`
and every script resolves the same compose project, container names,
ports, volumes and seed file for it (see "Concurrent runs" above). An
explicit `PARITY_PROJECT` always wins over the derivation; the default
when nothing is set is `parity19`.

Never combine exported `PARITY_*` variables with `sudo` plus a stack
script (or `sudo docker compose`): sudo's `env_reset` drops the exported
variables, the resolver silently falls back to the default project, and
a `down -v` meant for your private slot deletes a sibling slot's live
stack including its volumes. This happened once (NEWFRONT-174); the
destructive scripts now refuse the ambiguous shape instead of guessing,
but raw `docker compose` commands have no such guard.

Safe forms, in order of preference:

```sh
# 1. No sudo at all (preferred; needs docker-group membership).
export PARITY_NS=257
apps/web_new/e2e/parity/stack/parity-down.sh

# 2. Project as an argument: argv survives sudo, exported env does not.
sudo apps/web_new/e2e/parity/stack/parity-down.sh parity257

# 3. Env passed explicitly through sudo.
sudo env PARITY_NS=257 apps/web_new/e2e/parity/stack/parity-down.sh
```

For `parity-reset.sh` under sudo, prefer form 3 on a namespaced stack:
reset's bring-up half derives the stack's ports from the namespace,
which a bare project argument cannot carry. `parity-down.sh` needs only
the project, so form 2 is complete there.

Rule of thumb: every project-scoped compose command spells its project
with `-p` (argv), never through environment interpolation alone.
Operators may additionally set `Defaults env_keep += "PARITY_*"` in
sudoers on runner hosts; the scripts do not rely on it.

## Sibling stack deleted? Recover in minutes

If your stack's containers or volumes vanish because another slot's `down`
hit your project (symptom: sudden DB/API failures, `docker ps` no longer
shows your containers), only scratch rows are lost — images and checkouts
survive. From your own checkout (never rebuild images from someone else's:
they bake the wrong code), with your namespace exported:

```sh
export PARITY_NS=257   # or yours; omit for the default stack
apps/web_new/e2e/parity/stack/parity-up.sh
```

That one command re-does the whole path: bring up, wait for migrations
(`migrate --check`), bootstrap the instance, reseed the parity workspace,
regenerate the seed file. Then rerun your scenarios; they regenerate the
rest of the scratch data. The whole recovery is about ten minutes when
images are cached. Leave a `process:` note on NEWFRONT-1 saying which
slot hit you, so the pattern stays visible.

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
export PARITY_NS=257   # or yours; the scripts resolve the rest from it
eval "$(apps/web_new/e2e/parity/stack/parity-env.sh)"
docker compose -p "$PARITY_PROJECT" -f apps/web_new/e2e/parity/stack/docker-compose.yml --profile full up -d live
```

The document editor needs the live endpoint to become editable. The
oracle reads it from `VITE_LIVE_BASE_URL`, which the compose file
derives from `PARITY_LIVE_PORT` when set (empty otherwise, keeping the
window-origin fallback): export the slot's live port — e.g.
`PARITY_LIVE_PORT=13089` — and (re)create the oracle so it picks it up (the
dev server reads it at startup; the production build bakes it in at build
time, so that one keeps the fallback). Known wrinkle (NEWFRONT-188): the
compose `live` image
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
Django, `/god-mode/*` to the old admin console, and everything else to the
old app, so the native credential POST and the session cookie behave
exactly like deployed. That address is the Playwright `oracle` base URL;
the admin mount is the `PARITY_ORACLE_ADMIN_URL` suite address (see
"Admin console oracle" above).

The new app is not served by this stack; run its dev server yourself:

```sh
pnpm --filter web_new dev   # :3010, the Playwright `new` base URL
```

## Editions

This checkout runs the OSS backend. Where the cloud overlay exists, the
same scripts serve the cloud edition (NEWFRONT-436); desktop rows run
against the desktop build of web_new with the same seeded stack.

## Cloud oracle

The cloud edition of the old app — the OSS API plus the private overlay,
behind the overlaid production frontend — as a selectable oracle for the
CLOUD-\* rows. The default stays OSS: nothing below changes unless
`PARITY_ORACLE_EDITION=cloud` is exported.

Prerequisite: a runner-local checkout of the private overlay repo,
OUTSIDE the pi-dash tree (read-only input; never commit it into pi-dash,
never import it from web_new code):

```sh
git clone --depth 1 git@github.com:The-AI-Republic/private-pi-dash.git ~/private-pi-dash
```

Bring-up resolves it from `CLOUD_OVERLAY_DIR`, else from the
`<repo-root>/../private-pi-dash` sibling. A missing or incomplete
checkout fails before any Docker call, with these instructions.

Bring up (one stack serves one edition; use a second `PARITY_NS`
namespace to run OSS and cloud side by side — ports, volumes, seed file
and suite URLs all derive from the namespace exactly like the OSS
stack):

```sh
export PARITY_NS=436   # or your slot; omit for the default stack
export PARITY_ORACLE_EDITION=cloud
apps/web_new/e2e/parity/stack/parity-up.sh
```

That stages the overlay inputs into the gitignored
`stack/.cloud-staging/` directory (refreshed every bring-up, so overlay
edits apply on the next run), builds `api-cloud` (the OSS api image plus
one overlay layer), `oracle-cloud-prod` (the production oracle plus one
overlay layer) and `oracle-admin-cloud-prod` (the production admin
oracle plus the overlay's single admin file — the cloud home-page
override — swapped on top; NEWFRONT-446), and starts `pg redis mq
minio api-cloud oracle-cloud-prod oracle-admin-cloud-prod proxy`. The
cloud `/god-mode/` mount serves the overlaid admin entry gate (OIDC
authorization resolver) instead of the OSS setup/sign-in gate; the
dashboard pages past the gate are the same build with the same data. `api-cloud` runs on
`parity_cloud_scratch_settings.py` (mounted read-only), which layers the
same 600/minute anonymous widening onto the cloud settings — the cloud
settings inherit the 30/minute production budget, which would otherwise
saturate minutes into an oracle run. There is no cloud worker (nothing
in the cloud area needs background fan-out) and no dev-mode cloud oracle
(`PARITY_ORACLE_MODE=dev` with the cloud edition refuses; unset it).
Teardown is the existing path — `parity-down.sh`, `parity-reset.sh` and
the run-end reaper all work unchanged — plus removal of the staging
directory, so no overlay file lingers in the tree past its stack:

```sh
export PARITY_NS=436
export PARITY_ORACLE_EDITION=cloud
apps/web_new/e2e/parity/stack/parity-down.sh   # containers, volumes, staging; images stay
```

Seed contents (`seed/seed_cloud.py`, same `PARITY_SEED_JSON:` line and
key names as the OSS seed, so shared helpers keep working):

- user `cloud-oracle@example.com` (dev-login subject `parity|cloud-seed`,
  display name `Cloud Oracle`), verified and onboarded, no password —
  cloud identities come from the provider;
- workspace `Cloud Workspace` (`cloud-ws`), owned by that user;
- project `Cloud Project` (`CLD`), flat list layout preference;
- one `Todo` state plus three issues: `Cloud first/second/third issue`.

Scenarios sign this user in through the dev-login endpoint, which bypasses
real airepublic SSO (no `AIREPUBLIC_*` secrets needed for the core
flows). Seeded values are overridable through `PARITY_CLOUD_SEED_EMAIL`,
`PARITY_CLOUD_SEED_SUB` and `PARITY_CLOUD_SEED_DISPLAY_NAME`.

Smoke verification (from the bring-up shell, with the printed suite env
exported; `PARITY_API_URL` is the api port, `PARITY_ORACLE_URL` the
proxy):

```sh
# 1. The overlay pages render through the proxy.
curl -s -o /dev/null -w 'home:%{http_code}\n' "$PARITY_ORACLE_URL/home"
curl -s -o /dev/null -w 'login:%{http_code}\n' "$PARITY_ORACLE_URL/login"
# 1b. The cloud admin entry serves (the overlaid gate is
# client-rendered, so a 200 shell proves the mount; the ADM-074..079
# scenarios prove the gate branches).
curl -s -o /dev/null -w 'godmode:%{http_code}\n' "$PARITY_ORACLE_URL/god-mode/"
# 2. The seeded identity signs in (dev-login bypasses real SSO). Carry the
# session cookies like the browser does: the session-backed views below
# read those, not the Bearer token.
SEED="$(cat "$PARITY_SEED_FILE")"
EMAIL="$(echo "$SEED" | python3 -c 'import json,sys; print(json.load(sys.stdin)["email"])')"
JAR="$(mktemp)"
curl -s -c "$JAR" -o /dev/null -w 'devlogin:%{http_code}\n' -X POST "$PARITY_API_URL/api/auth/dev-login/" \
  -H 'Content-Type: application/json' \
  -d "{\"email\":\"$EMAIL\",\"display_name\":\"smoke\"}"
curl -s -b "$JAR" -o /dev/null -w 'me:%{http_code}\n' "$PARITY_API_URL/api/auth/me/"
# 3. The cloud APIs respond.
curl -s -b "$JAR" -o /dev/null -w 'openhub:%{http_code}\n' "$PARITY_API_URL/api/openhub/status/"
curl -s -b "$JAR" -o /dev/null -w 'billing:%{http_code}\n' "$PARITY_API_URL/api/billing/me/plan/"
curl -s -b "$JAR" -o /dev/null -w 'connections:%{http_code}\n' "$PARITY_API_URL/api/users/me/ai-assistant/connections/"
```

All eight answer 200. The openhub status may report `connected: false`
(no gateway behind a scratch stack) — the smoke proves the endpoint
responds, not that a gateway answers.

Notes for scenario authors: the OpenHub app lane is on in both halves
(`OPENHUB_APPS_ENABLED=1` on the API, `VITE_OPENHUB_APPS_ENABLED=1`
baked into the oracle); the flag-off shape is a documented follow-up
(see `parity/cloud-oracle.md`). The `helpers/stack.ts` instance control
targets the `api` service — extend it if a cloud scenario needs it.
The overlay's pinned OSS SHA and its skew against this branch are
reconciled in `parity/cloud-oracle.md`.

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

## Disk-full guard

Stale artifacts accumulate on the shared validation host across slots
(`/tmp` build/probe dirs from finished runs, docker build cache and image
layers), and nothing reclaims them on its own. At 100% any slot's Postgres
PANICs with `No space left on device` mid-sweep; the API starts returning
500s and the sweep burns (NEWFRONT-175).

`parity-up.sh` therefore runs `parity-disk-check.sh` before the docker
build. The check reads the filesystems holding `/` and Docker's data-root
and judges the one with the least free space. The limits are absolute free
space, not a percentage, so a large disk at 90% used with hundreds of GB
left is not refused: it warns below `PARITY_DISK_WARN_FREE_GB` (default 15)
and refuses to start below `PARITY_DISK_MIN_FREE_GB` (default 6, the old 90%
limit on a 60 GB host). The retired `PARITY_DISK_WARN_PCT` and
`PARITY_DISK_MAX_USE_PCT` overrides now exit 2. Run it by hand before a long
sweep against a kept stack, where no fresh bring-up runs the gate:

On Docker Desktop (macOS and Windows) the data-root only exists inside the
daemon's Linux VM, so the host cannot `df` it. The guard then measures the
VM disk from a throwaway container (`docker run --rm --network none
alpine:3.20 df -P /`) and applies the same limits. If that container
cannot run, it prints a warning that the VM disk was not checked and
continues with the host `/` check; a data-root that exists on the host is
always checked as before, and an unreadable data-root on a non-Desktop
Linux host is still a configuration error (exit 2).

```sh
apps/web_new/e2e/parity/stack/parity-disk-check.sh [--path <dir>]...
```

If a sweep dies with Postgres `No space left on device`, sudden API 500s,
or mass sign-in/load failures, check host disk first — before debugging
the spec (read-only):

```sh
df -h /
df -h /tmp
docker system df
```

Recovery is: free space, restart the slot's Postgres, verify the seed
(three issues, grouped reads), then re-sweep from pristine state
(`parity-reset.sh`). Freeing host disk is the operator's janitoring
(NEWFRONT-248): agents must not prune sibling stacks, images, volumes,
build caches, or `/tmp` files owned by other runs. `PARITY_IGNORE_DISK_CHECK=1`
bypasses the refusal for emergencies only — never on a shared host
without the operator's acknowledgement.

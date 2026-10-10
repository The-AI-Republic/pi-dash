# Parity suite

The same scenarios run against both frontends: first `apps/web` (the
oracle, proving the scenario is correct), then `apps/web_new` (proving
parity). The UI may differ; the scenario text does not.

## Layout

```text
e2e/parity/
  playwright.config.ts   two projects: oracle (old app) and new (web_new)
  fixtures.ts            `test`/`expect` with driver plus seed facts
  drivers/
    parity-driver.ts     the interface: user-level actions and reads
    web.ts               oracle driver against apps/web
    web-new.ts           skeleton for apps/web_new (throws until an area lands)
    index.ts             picks the driver from the project or PARITY_TARGET
  helpers/
    api.ts               server-state assertions through the REST API
    tags.ts              inventory-ID tagging plus title parsing
  issues/                example scenario (AUTH-001, ISS-007)
  stack/                 seeded backend plus runbook
  report/                parity report generator (CI)
```

## Running

Bring up the stack (runbook in `stack/README.md`), start the frontend you
want, then from `apps/web_new`:

```sh
export PARITY_SEED_FILE="$PWD/e2e/parity/.seed.json"
export PARITY_API_URL=http://localhost:18019
export PARITY_ORACLE_URL=http://localhost:3000
export PARITY_NEW_URL=http://localhost:3010
pnpm test:parity:oracle   # old app only
pnpm test:parity:new      # new app only (until areas land, these throw by design)
pnpm test:parity           # both projects
```

Narrow a run by adding Playwright arguments after the script name:

```sh
pnpm test:parity:oracle --grep @iss-007                 # one inventory row
pnpm test:parity:oracle runners/chat.spec.ts            # one spec file
pnpm test:parity:oracle runners/ --list                 # list, do not run
```

Always narrow while you iterate; the full oracle suite is several hundred
scenarios on one worker. A `--` before the arguments is accepted and
ignored.

## Writing a scenario

1. Find the inventory rows it proves; every scenario names at least one.
2. Title it with `specTitle(ROWS, "...")` and tag it with
   `specTags(ROWS)` so `--grep` and the report can join results back.
3. Drive only through `driver` (what the user does/sees) and `helpers/api`
   (what the server stored). Never reach into page internals from the spec.
4. Make it green on `apps/web` first, then flip the matching rows to
   `oracle green`.

Row status, in the inventory's Status cell: `not started`, then
`oracle green` once the scenario passes on the old app. A build slice that
implements the row in `apps/web_new` appends `built` (`oracle green,
built`); slices are unit-tested and do not run the parity suite. The
area gate runs the area's scenarios against `apps/web_new` once and turns
passing rows into `new green`. The report counts all three.

## Extending the driver

New areas need new actions: add the method to `parity-driver.ts`, implement
it in `drivers/web.ts`, and add a throwing stub in `drivers/web-new.ts`
until the area lands. Extend, never fork: one interface, two drivers.

Prefer user-visible selectors (`getByRole`, `getByText`, `getByPlaceholder`):
the example scenario reads issue titles as paragraph text in `main`
landmarks with no app-side hook at all. If the old app genuinely has no
stable selector for something a scenario needs, a `data-testid` attribute
in `apps/web` is the only change allowed there — but note the CI paths
gate only passes `apps/web` diffs whose every changed line carries
`data-testid`, which a first-time attribute addition cannot satisfy, so
treat hooks as a last resort needing a human-granted path exception.

## Old bugs

If the oracle behavior is a bug, do not encode it silently: prefix the
scenario title with `bug:`, link the issue in a comment, and record the
intended behavior in the inventory row. web_new may implement the fix.

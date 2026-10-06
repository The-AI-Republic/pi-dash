<!-- Copyright (c) Pi Dash contributors. License pending H-license decision (NEWFRONT-2). -->

# Perf harness (NEWFRONT-21)

One method measures both apps; two specs guard the budgets in CI.

## CI specs (budgets from the Quality gates page)

- `perf.desktop-warm.spec.ts` — desktop warm start renders cached rows in
  ≤ 500 ms. A cold load populates the persisted query cache; the measured
  reload blocks the issues API, so rows can only come from the warm cache.
- `perf.scroll-2000.spec.ts` — an issue list with 2,000 rows scrolls with
  no frame over 50 ms. The issues API is mocked to 2,000 rows; a recorder
  samples real rAF pacing while the list scrolls top to bottom.

Both run against the production bundles with every API call mocked, so CI
needs browsers but no backend:

```sh
pnpm --filter web_new build
pnpm --filter web_new test:perf
```

`playwright.perf.config.ts` serves `dist/web` (:3021) and `dist/desktop`
(:3022) through `serve.mjs` and runs the specs serially — frame and load
timings must not compete with each other for CPU. `mocks.ts` builds the
payloads; the app parses every one with zod at runtime, so a malformed mock
fails loudly, and any endpoint outside the four the slice calls is aborted.

## Baseline measurement (design.md Appendix B)

`measure.mjs` signs in through the served origin's own `/auth` proxy, then
loads the issue list cold (fresh profile, ×3) and warm (primed HTTP cache,
×3) and records the JS fetched plus the time to painted rows. It runs the
same steps against either app:

```sh
# Serve each production bundle with /api proxied to a seeded Django:
node serve.mjs --dir apps/web_new/dist/web --port 3041 --api http://localhost:18021
node serve.mjs --dir apps/web/build/client --port 3043 --api http://localhost:18021

node measure.mjs --app-url http://localhost:3041 --kind web-new \
  --email user@example.com --password secret --workspace ws \
  --project <uuid> --issue-name "Seeded issue" --out new-web.json
node measure.mjs --app-url http://localhost:3043 --kind web-old ... # same flags
```

Desktop launches measure the desktop bundle the same way (the Tauri shell
loads these files from disk; native window overhead is outside frontend
code). The old desktop serves the same client bundle: the overlay swaps
desktop chrome outside the issue-list route chain, so the measured route
loads identical files.

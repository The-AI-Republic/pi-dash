# 13 — Development Workflow

Day-to-day commands, what runs them, and the quality gates you'll bump into.

## TS workspace (root)

```bash
pnpm dev                         # start all dev servers concurrently
pnpm build                       # turbo run build across workspace
pnpm check                       # format + lint + types
pnpm check:lint                  # OxLint only
pnpm check:format                # oxfmt check
pnpm check:types                 # tsc --noEmit across workspace
pnpm fix                         # auto-fix format + lint
pnpm fix:format                  # oxfmt
pnpm fix:lint                    # oxlint --fix

# Target a single package or app:
pnpm turbo run <task> --filter=<pkg>
pnpm --filter web_new dev
pnpm --filter live dev
```

> i18n: the OSS build is English-only — wrap UI copy in `t("Source English text")`; no key catalogue to sync. Adding a language is documented in `packages/i18n/README.md`. (Multi-language locales and their tooling are part of Pi Dash Cloud.)

## Rust API (`rust-api/`)

```bash
cd rust-api

# Serve + worker (DATABASE_URL must be a TCP URL; serve fails fast on a stale schema):
export DATABASE_URL="postgresql://pi_dash:pi_dash@127.0.0.1:5432/pi_dash"
cargo run -p pidash-api-bin -- serve --bind 127.0.0.1:8000
cargo run -p pidash-api-bin -- worker --concurrency 4

# Narrowest checks for the crate you touched:
cargo fmt --all --check
cargo clippy -p <affected-crate> --all-targets -- -D warnings
cargo test -p <affected-crate>   # add a module/test filter when you can
python3 scripts/write_guard.py --check
```

Contract suites covering your change run against the Rust binary (`BASE_URL` + `DATABASE_URL` + `CONTRACT_SECRET_KEY`, see `contract-tests/README.md`); run the same case against live Django to prove it encodes the old system's real behaviour.

The Django backend in `apps/api/` is frozen — no behaviour change, no new tests, no Ruff runs needed for active-system work.

See [14 — Testing](./14-testing.md) for the full test layout.

## Rust runner (`runner/`) + desktop/daemon wire types (`pidash-ipc/`)

```bash
# From the repo root (runner/ + pidash-ipc/ are in the root Cargo workspace):
cargo build -p pidash             # debug build
cargo test -p pidash              # unit + integration tests (add a filter when you can)
cargo clippy -p pidash --all-targets -- -D warnings   # lint — treat warnings as errors

# From a debug build, run via:
./target/debug/pidash <subcommand>
```

`rust-toolchain.toml` pins the Rust version. `cargo-dist` packages release artifacts — see [15 — Releasing](./15-releasing.md).

## OxLint + oxfmt (TS workspace)

The entire TS workspace shares **one** root config:

- `.oxlintrc.json`
- `.oxfmtrc.json`

Both are Rust-based. OxLint is 50-100x faster than ESLint, with zero Node.js dependencies at runtime. `eslint-disable` comments still work for back-compat.

### `web_new` quality gates

`web_new` lints with `--deny-warnings` and enforces `check:boundaries`, `check:headers`, `check:deps` and `check:similarity` — see [05 — Frontend architecture](./05-frontend-architecture.md). The frozen legacy apps keep their old `--max-warnings` ceilings; never raise one.

### What gets linted

Root `pnpm check:lint` fans out via Turbo — every workspace app and package lints itself, including the frozen legacy apps (with their pinned ceilings). Active-system work is gated on the `web_new` checks above plus `live`'s `--max-warnings` ceiling.

Ignored:

```
node_modules/, dist/, build/, .next/, .turbo/, **/coverage/, **/storybook-static/
*.config.{js,mjs,cjs,ts}
```

## Husky + lint-staged (pre-commit hook)

`.husky/` registers a pre-commit hook that runs **lint-staged** on every commit:

1. `oxfmt --no-error-on-unmatched-pattern` formats staged files (`.{js,jsx,ts,tsx,cjs,mjs,cts,mts,json,css,md}`).
2. `oxlint --fix --deny-warnings` lints staged code files.

A warning that trips `--deny-warnings` blocks the commit. **Fix the warning** — do **not** `--no-verify`. Per repo policy on destructive actions: investigate and fix, don't bypass.

If a commit fails because of a hook, the commit didn't happen — fix the issue, re-stage, and create a **new** commit (don't `--amend`, you'd modify the wrong commit).

## Turborepo

- Config: `turbo.json` at repo root.
- Build outputs cached under `dist/**`, `build/**`, `.react-router/**`.
- `dev` and `clean` tasks bypass the cache (you want fresh dev servers).
- Remote cache is **disabled** in `turbo.json`.

Useful Turbo commands:

```bash
pnpm turbo run build --filter=web_new
pnpm turbo run build --filter=...web_new     # build web_new + everything web_new depends on
pnpm turbo run build --filter=web_new^...    # build everything that depends on web_new
pnpm turbo run build --dry                   # show the task graph without executing
```

## Dependency conventions

From `AGENTS.md`:

- **Internal packages** — `"workspace:*"`.
- **External deps** — `"catalog:"`, pinned in the `catalog:` block of `pnpm-workspace.yaml`.

Don't add a direct version for anything in the catalog. Upgrading a catalog entry upgrades it for every package at once — that's the point.

## Where to read next

- [14 — Testing](./14-testing.md) — test runners + conventions per stack
- [15 — Releasing](./15-releasing.md) — runner cargo-dist + version envs
- [docs/linting.md](../linting.md) — deeper OxLint docs (rules, suppressions, IDE integration)

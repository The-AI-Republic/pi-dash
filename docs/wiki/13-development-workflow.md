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
pnpm --filter web dev
pnpm --filter @pi-dash/ui storybook    # Storybook on :6006

# i18n:
pnpm i18n:sync                   # sync source English messages across all locales
pnpm i18n:translate              # auto-translate missing keys
```

## Django backend (`apps/api/`)

```bash
cd apps/api

# Server commands (DJANGO_SETTINGS_MODULE defaults to ...production):
python manage.py runserver
python manage.py migrate
python manage.py createsuperuser
# For local work, point at local settings:
DJANGO_SETTINGS_MODULE=apple_pi_dash.settings.local python manage.py runserver

# Tests:
./run_tests.sh                   # wrapper around tests/run_tests.sh
python run_tests.py -u           # unit tests only (markers: unit | contract | smoke)
python run_tests.py -u -o -p     # unit + coverage + parallel (-n auto)
pytest -m smoke                  # direct pytest with markers

# Lint / format (Ruff, NOT wired into root pnpm fix):
ruff format .
ruff check . --fix
```

`pytest.ini` configures `--reuse-db --nomigrations`. Tests reuse the DB across runs and skip migrations — fast, but you must run `pytest --create-db` to validate schema-altering work.

See [14 — Testing](./14-testing.md) for the full test layout.

## Rust runner (`runner/`)

```bash
cd runner
cargo build                      # debug build
cargo test                       # unit + integration tests
cargo check                      # quick type-check
cargo clippy -- -D warnings      # lint — treat warnings as errors

# From a debug build, run via:
./target/debug/pidash <subcommand>
```

`rust-toolchain.toml` pins the Rust version. `cargo-dist` packages release artifacts — see [15 — Releasing](./15-releasing.md).

## OxLint + oxfmt (TS workspace)

The entire TS workspace shares **one** root config:

- `.oxlintrc.json`
- `.oxfmtrc.json`

Both are Rust-based. OxLint is 50-100x faster than ESLint, with zero Node.js dependencies at runtime. `eslint-disable` comments still work for back-compat.

### Per-app `--max-warnings` ceilings

Each app pins its own ceiling in its `package.json`:

| App           | Ceiling |
| ------------- | ------- |
| `web`         | 11957   |
| `space`       | 676     |
| `admin`       | 759     |
| `live`        | 119     |
| `@pi-dash/ui` | 66      |

Crossing the ceiling fails `pnpm check:lint`. **After a cleanup, lower the ceiling** — leaving headroom defeats the gate.

### What gets linted

```
apps/{web,admin,space,live}/**
packages/**
```

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
pnpm turbo run build --filter=web
pnpm turbo run build --filter=...web         # build web + everything web depends on
pnpm turbo run build --filter=web^...        # build everything that depends on web
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

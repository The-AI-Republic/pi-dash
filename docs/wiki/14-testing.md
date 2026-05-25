# 14 — Testing

Each stack has its own test runner. There is **no** unified "pnpm test" — root `pnpm check` covers JS/TS lint+format+types, but not Python tests and not the Rust runner.

## TS workspace (Vitest)

Vitest runs per-app and per-package where tests exist.

```bash
# Per app / package:
pnpm --filter web test
pnpm --filter live test
pnpm --filter @pi-dash/ui test

# Across workspace via Turbo:
pnpm turbo run test
```

Test files live next to source (`*.test.ts` / `*.test.tsx`) or in app-local `tests/` dirs (e.g. `apps/web/tests/`, `apps/live/tests/`).

### Storybook

`@pi-dash/ui` ships Storybook for component-in-isolation development:

```bash
pnpm --filter @pi-dash/ui storybook   # http://localhost:6006
```

Build new components in Storybook first, wire them into an app second.

## Django backend (pytest)

`apps/api/pytest.ini` configures:

- `--reuse-db --nomigrations` — DB persists between runs; migrations are skipped.
- Markers: `unit | contract | smoke`.

```bash
cd apps/api

./run_tests.sh                   # wraps tests/run_tests.sh
python run_tests.py -u           # unit only
python run_tests.py -u -o -p     # unit + coverage + parallel (-n auto)
python run_tests.py              # full suite

pytest -m smoke                  # direct pytest, smoke marker only
pytest --create-db               # rebuild DB — required for schema-altering changes
pytest apps/api/pi_dash/runner/tests/test_session.py::test_welcome_frame  # one test
```

### Markers

| Marker     | Meaning                                                       |
| ---------- | ------------------------------------------------------------- |
| `unit`     | Fast, isolated. Default for new tests.                        |
| `contract` | Cross-module contract tests — wire-format, serializer parity. |
| `smoke`    | Coarse-grained end-to-end happy-path.                         |

Tests live under `apps/api/pi_dash/tests/` and per-module `tests/` (e.g. `pi_dash/runner/tests/`).

### Test settings

`apple_pi_dash.settings.test` is the test settings module — it tweaks middleware and disables some prod-only services so tests don't need RabbitMQ etc.

## Rust runner (cargo test)

```bash
cd runner
cargo test                       # unit + integration
cargo test --release             # release mode (slower compile, runtime closer to shipped binary)
cargo test test_name             # filter
cargo clippy -- -D warnings      # lint as a quality gate
```

Test layout:

- **Unit tests** — inline `#[cfg(test)] mod tests { ... }` in each module. Deterministic, table-driven where possible (protocol serde, approval policy, reconnect backoff, workspace resolve, config roundtrip).
- **Integration tests** — `runner/tests/protocol_roundtrip.rs` and friends. Every client/server protocol variant round-trips; router state machine invariants get exercised.

### Manual QA matrix (per release)

Per `runner/README.md`, every runner release is manually QA'd:

> macOS arm64 / x64 + Linux x64 + Windows x64 → first-run `configure` → `install` → `start` → TUI shows connected → synthetic run via `/api/runners/runs/` → approval prompt → decision.

## Quality gates summary

| Gate          | Command                        | When it runs                                                       |
| ------------- | ------------------------------ | ------------------------------------------------------------------ |
| TS format     | `oxfmt`                        | Pre-commit (lint-staged) + `pnpm check:format`                     |
| TS lint       | `oxlint --fix --deny-warnings` | Pre-commit + `pnpm check:lint` (per-app `--max-warnings` ceilings) |
| TS types      | `tsc --noEmit`                 | `pnpm check:types` (per-package)                                   |
| TS unit       | `vitest`                       | `pnpm test` / per-package                                          |
| Python format | `ruff format`                  | Manual (not wired to root)                                         |
| Python lint   | `ruff check`                   | Manual                                                             |
| Python tests  | `pytest`                       | `./run_tests.sh`                                                   |
| Rust lint     | `cargo clippy -- -D warnings`  | Manual                                                             |
| Rust tests    | `cargo test`                   | Manual                                                             |

Everything is run in CI on PRs — the `--deny-warnings` and `-D warnings` settings make sure no warning slips through unreviewed.

## Where to read next

- [13 — Development workflow](./13-development-workflow.md) — the full command reference
- `apps/api/tests/` and `apps/api/run_tests.py` — Python test harness
- `runner/tests/` — Rust integration tests

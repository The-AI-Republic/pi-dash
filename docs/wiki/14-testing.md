# 14 — Testing

Each stack has its own test runner. There is **no** unified "pnpm test" — root `pnpm check` covers JS/TS lint+format+types, and the Rust stacks use cargo.

## TS workspace (Vitest)

Vitest runs per-app and per-package where tests exist.

```bash
# Per app / package:
pnpm --filter web_new test
pnpm --filter live test
pnpm --filter @pidash/kit test

# Across workspace via Turbo:
pnpm turbo run test
```

Test files live next to source (`*.test.ts` / `*.test.tsx`) or in app-local `tests/` dirs (e.g. `apps/live/tests/`). `web_new` additionally gates on `test:changed --coverage`, the parity suites (`test:parity:oracle` / `test:parity:new`), and the `check:*` gates — never run the whole `web_new` suite yourself.

## Rust backend (contract tests + cargo)

Backend behaviour is pinned by the pytest contract suites in `rust-api/contract-tests/<domain>/`:

```bash
cd rust-api/contract-tests
python3 -m venv .venv && source .venv/bin/activate && pip install -e .
BASE_URL=http://127.0.0.1:8000 DATABASE_URL="postgresql://..." pytest app_intake
```

Suites run unmodified against live Django (the oracle: the old system's real behaviour) and against the Rust binary with every `PIDASH_RUST_*` flag on. Always run a new or changed case against **both** — against Django to prove it encodes the old system's real behaviour (or, for a deliberate-difference case, that the old system really differs), and against Rust to prove the fix. See `rust-api/contract-tests/README.md` for the full env contract.

Rust unit tests live inline (`#[cfg(test)]`) in each crate:

```bash
cd rust-api
cargo test -p <affected-crate>   # add a module/test filter when you can
```

The Django test suite in `apps/api/` is frozen with the backend — no new tests go there.

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

| Gate              | Command                                             | When it runs                                              |
| ----------------- | --------------------------------------------------- | --------------------------------------------------------- |
| TS format         | `oxfmt`                                             | Pre-commit (lint-staged) + `pnpm check:format`            |
| TS lint           | `oxlint --fix --deny-warnings`                      | Pre-commit + `pnpm check:lint` (`web_new` gates in [05](./05-frontend-architecture.md)) |
| TS types          | `tsc --noEmit`                                      | `pnpm check:types` (per-package)                          |
| TS unit           | `vitest`                                            | `pnpm test` / per-package                                 |
| Rust fmt/clippy   | `cargo fmt --all --check`, `cargo clippy -- -D warnings` | Manual, per affected crate                             |
| Rust unit         | `cargo test -p <crate>`                             | Manual, narrowest filter                                  |
| Contract (Rust)   | `pytest <domain>` (`BASE_URL` → Rust binary)        | Manual, cases covering the change                         |
| Contract (oracle) | `pytest <domain>` (`BASE_URL` → Django)             | Manual, same cases — proves the pinned behaviour is real  |

Everything is run in CI on PRs — the `--deny-warnings` and `-D warnings` settings make sure no warning slips through unreviewed.

## Where to read next

- [13 — Development workflow](./13-development-workflow.md) — the full command reference
- `rust-api/contract-tests/README.md` — the contract harness env contract
- `runner/tests/` — Rust integration tests

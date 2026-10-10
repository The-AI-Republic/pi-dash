# Known Rust failures (PIDASHCONV-821)

Suites in the Rust HTTP matrix
(`.github/workflows/rust-api-contract-rust.yml`) that currently fail
against the Rust server for a tracked root cause. Each entry names the
fix issue; the workflow's gate fails the job when a suite fails that is
**not** on this list, and fails it when a suite **passes** while still
on this list (so the fix PR must drop its entry — a green fix PR proves
the entry is stale).

Entry format (parsed by `_harness/rust_matrix.py`, one line per suite):

```md
- `app_issues`: PIDASHCONV-822 — one-line symptom or root cause
```

Rules:

- One fix issue per root cause, filed under PIDASHCONV-1 with title
  `Rust <domain> fix: …`, the failing test, and the suspected file.
- Never edit the test to match Rust; fix Rust (or prove Django wrong in
  the fix issue and port the corrected behaviour).
- This list must be empty for PIDASHCONV-821 to be Done.

## Suites

- `auth_magic`: PIDASHCONV-854 — every magic-link endpoint 500s with an empty body (Django 39/39)
- `auth_session`: PIDASHCONV-855 — all 11 password set/change/reset tests time out vs Rust (Django 79/79 in 33s)
- `orchestration`: PIDASHCONV-856 — signal-dispatch/reentrancy divergence: rooms don't retime, ticker doesn't arm/disarm, moves not counted queued
- `prompting`: PIDASHCONV-842 — Rust anon throttle 429s GET /auth/get-csrf-token/ after ~30 hits (Django unlimited)

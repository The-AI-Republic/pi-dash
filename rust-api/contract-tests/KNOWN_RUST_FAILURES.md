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

- One fix issue per root cause, with the failing test and the suspected
  file. Under PIDASHCONV the title was `Rust <domain> fix: …`; since the
  NEWPIDASH project it is a NEWPIDASH `Bug:`/`Drift:` issue (the parser
  accepts any `<PROJECT>-<n>` id).
- Never edit the test to match Rust; fix Rust (or prove Django wrong in
  the fix issue and port the corrected behaviour).
- This list must be empty for PIDASHCONV-821 to be Done.

## Suites

- `v1_openapi`: NEWPIDASH-34 — test_routes_golden.py: Rust OpenAPI document omits the v1 scheduler write routes (main 730ea8d28, 7d64d46ad)

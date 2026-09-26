# Contract tests: api-v1 projects, members, states, estimates (D-19 oracle)

Stage-1 oracle for domain D-19 (`project` 4, `member` 5, `invite` 1,
`user` 1, `state` 2, `estimate` 3 URL entries). The same suite runs against
the live Django server and, later, against the Rust server through the proxy.

## Layout

- `_harness/` — shared helpers (created first-use by PIDASHCONV-77; extend,
  never fork): direct-Postgres seeding (`db.py`) and the API-key HTTP client
  (`http.py`). No Django imports anywhere.
- `v1_projects/` — this domain's suite, one module per URL module.

## Run

```sh
cd rust-api/contract-tests
BASE_URL=http://127.0.0.1:18077 DATABASE_URL=postgresql://user:pass@host:5432/db \
  pytest v1_projects -q
```

`BASE_URL` is the live backend under test (no trailing slash). `DATABASE_URL`
is the Postgres the backend reads/writes; the suite seeds rows straight into
it with `psycopg` and never touches Django. Both variables are required —
the suite errors out when either is missing.

The server under test should run with a generous `API_KEY_RATE_LIMIT`
(e.g. `100000/minute`): the default `60/minute` per-key throttle is real
backend behavior, but the suite is a contract check, not a throttle check.
Each test module authenticates with its own API key so per-key budgets stay
independent either way.

## Coverage contract (per domain)

- Every registered endpoint gets a response-shape assertion.
- One denied-permission case (a caller without the role gets 403).
- One tenant-isolation case (a caller from another workspace sees nothing).
- The suite must fail when a permission class is deliberately removed
  (demonstrated per PR with a one-line local patch, reverted afterwards).
- Project identifier routing: project-scoped routes accept the UUID and the
  workspace-scoped `identifier` slug interchangeably.

## Known ported bugs pinned here

- `estimate.py` URL patterns are defined but **not registered** in
  `pi_dash/api/urls/__init__.py`, so every estimate route answers 404.
  `test_estimates.py` pins the 404; the Rust port must reproduce it until a
  follow-up decides otherwise.

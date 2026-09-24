# Contract tests (stage-1 oracles)

Python `pytest` + `httpx` suites, one directory per domain
(`rust-api/contract-tests/<domain>/`), with shared helpers in `_harness/`
(created on first use; extend it, never fork it).

## Running

```sh
cd rust-api/contract-tests
pip install -r requirements.txt
BASE_URL=http://127.0.0.1:8000 \
DATABASE_URL=postgresql://postgres:postgres@localhost:5432/pidash \
SECRET_KEY=<the backend's Django SECRET_KEY> \
  pytest app_scheduler
```

The suite runs against a **live** backend and seeds rows straight into
Postgres over `DATABASE_URL`. It never imports Django and never uses its
test client. The same suite runs against the Rust server through the proxy.

`SECRET_KEY` must match the backend under test: suites mint `session-id`
cookies with the backend's session-signing format instead of calling login
endpoints, so auth behaves identically on both backends.

## Conventions

- One test module per URL group plus `test_permissions.py` (denied cases)
  and `test_tenant_isolation.py`.
- Every endpoint gets a response-shape assertion (exact key sets).
- One denied-permission case per restricted endpoint; the suite must fail
  when a permission class is deliberately removed.
- Tests seed uniquely-named rows (uuid tags) and never truncate tables, so
  reruns against the same database are safe.
- Raw-SQL seeding bypasses model signals (no builtin-scheduler seeding, no
  default-pod creation): each test sees exactly what it inserted.

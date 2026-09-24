"""Shared harness for Rust contract tests (stage 1 oracles).

Every domain suite under ``rust-api/contract-tests/<domain>/`` uses these
helpers. Extend this package with broadly reusable helpers; keep
domain-specific seeding inside the domain directory. Never fork it.

The suite runs against a live backend (Django today, Rust via the proxy
tomorrow) driven by environment:

- ``BASE_URL`` — e.g. ``http://127.0.0.1:18094`` (no trailing slash).
- ``DATABASE_URL`` — psycopg-connectable URL for the backend's Postgres;
  suites seed rows straight into Postgres with raw SQL.
- ``SECRET_KEY`` — the backend's Django ``SECRET_KEY``; needed to mint
  session cookies (see :mod:`_harness.sessions`).

The suite never imports Django and never uses its test client.
"""

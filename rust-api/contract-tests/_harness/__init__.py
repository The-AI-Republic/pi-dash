"""Shared black-box helpers for Rust contract suites.

Every suite runs against a live server (Django today, Rust via the proxy
tomorrow) driven by BASE_URL, and seeds rows straight into Postgres via
DATABASE_URL. Nothing here imports Django or touches its test client.
"""

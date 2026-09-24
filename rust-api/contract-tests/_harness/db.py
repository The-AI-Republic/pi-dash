"""Raw Postgres access for contract suites.

Suites seed data straight into Postgres with plain SQL (no ORM, no Django
imports). Connections are autocommit; each test seeds uniquely-named rows so
no cleanup pass is needed for repeat runs against the same database.
"""

import psycopg

from . import config


def connect() -> psycopg.Connection:
    conn = psycopg.connect(config.database_url())
    conn.autocommit = True
    return conn

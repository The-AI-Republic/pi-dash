#!/bin/bash
set -e

pidash-api ops wait_for_db
# Fail fast when the schema is not current (NEWPIDASH-380): the release
# migrator owns upgrades; services never wait for migrations and never run DDL.
pidash-api migrate check

# watchmedo (Python-only) is dropped: local autoreload is `cargo watch`
# outside the image. No --concurrency flag, like Python (binary default 4;
# Celery would default to nproc).
exec pidash-api worker

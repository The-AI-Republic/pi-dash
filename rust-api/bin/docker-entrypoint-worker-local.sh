#!/bin/bash
set -e

pidash-api ops wait_for_db
pidash-api ops wait_for_migrations

# watchmedo (Python-only) is dropped: local autoreload is `cargo watch`
# outside the image. No --concurrency flag, like Python (binary default 4;
# Celery would default to nproc).
exec pidash-api worker

#!/bin/bash
set -e

pidash-api ops wait_for_db
pidash-api ops wait_for_migrations

# watchmedo (Python-only) is dropped: local autoreload is `cargo watch`
# outside the image. The worker also runs the scheduler loop (no
# scheduler-only mode); the scheduler lock keeps worker + beat from
# double-scheduling.
exec pidash-api worker

#!/bin/bash
set -e

pidash-api ops wait_for_db
# Wait for migrations
pidash-api ops wait_for_migrations
# Run the processes
# No `exec`, like Python (:8). The Rust worker also runs the scheduler
# loop (there is no scheduler-only mode); the scheduler lock keeps a
# worker + beat pair from double-scheduling.
pidash-api worker

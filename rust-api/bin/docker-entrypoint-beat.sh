#!/bin/bash
set -e

pidash-api ops wait_for_db
# Fail fast when the schema is not current (NEWPIDASH-380): the release
# migrator owns upgrades; services never wait for migrations and never run DDL.
pidash-api migrate check
# Run the processes
# No `exec`, like Python (:8). The Rust worker also runs the scheduler
# loop (there is no scheduler-only mode); the scheduler lock keeps a
# worker + beat pair from double-scheduling.
pidash-api worker

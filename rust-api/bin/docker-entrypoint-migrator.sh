#!/bin/bash
set -e

# $1 is dropped here: the release step takes no arguments, and `ops
# wait_for_db` takes none (clap would exit 2).
pidash-api ops wait_for_db

# The schema is owned by the native Rust migrator (NEWPIDASH-380): no
# Python, no Django. This runs once as the release/install step; if it
# fails or does not finish, the release fails and the old services keep
# running. Services never migrate — they run only `migrate check`.
pidash-api migrate up

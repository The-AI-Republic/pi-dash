#!/bin/bash
set -e

# $1 is dropped here: Django's wait_for_db ignores extra positionals while
# `ops wait_for_db` takes none (clap would exit 2); migrate below keeps it.
pidash-api ops wait_for_db

# migrate stays Django — Django owns the schema until switchover — so this
# step still shells to Python. The Rust image ships no interpreter: the
# migrator stays pinned to the Python image (deployments follow-up) until
# migrate itself is ported.
python manage.py migrate $1

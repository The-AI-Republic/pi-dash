#!/bin/bash
set -e

python manage.py wait_for_db
# Wait for migrations
python manage.py wait_for_migrations
# Run the processes
#
# ``--concurrency`` bounds the prefork pool. Left unset, Celery forks one child
# per CPU, and every child holds its own Postgres connection for the life of
# the worker — so on a large host the worker's connection footprint scales with
# the box rather than with the database, which here is shared with home-page.
# One child is one connection, so this number IS the worker's budget.
#
# The default is min(nproc, 8): a ceiling, never a floor. Defaulting to a flat
# 8 would *raise* the footprint on a small host (the prod API EC2 is a 2-vCPU
# t3.medium, where Celery would otherwise pick 2), which is the opposite of the
# intent. Set CELERY_WORKER_CONCURRENCY to override deliberately.
concurrency="${CELERY_WORKER_CONCURRENCY:-}"
if [ -z "$concurrency" ]; then
  # ``|| echo 8`` so a missing nproc falls back to the ceiling rather than an
  # empty --concurrency. Plain `if`, not `[ … ] && …`: under `set -e` a false
  # test as the last command of the script's list would abort the container.
  concurrency=$(nproc 2>/dev/null || echo 8)
  if [ "$concurrency" -gt 8 ]; then
    concurrency=8
  fi
fi

# ``exec`` so celery is PID 1 of the container and receives SIGTERM directly:
# as a bash child it does not, and a redeploy then leaves its connections to be
# reaped by timeout instead of closed on shutdown.
exec celery -A pi_dash worker -l info --concurrency "$concurrency"
#!/bin/bash
set -e

python manage.py wait_for_db
python manage.py wait_for_migrations

exec watchmedo auto-restart \
  --directory=./pi_dash \
  --pattern="*.py" \
  --recursive \
  -- celery -A pi_dash worker -l info

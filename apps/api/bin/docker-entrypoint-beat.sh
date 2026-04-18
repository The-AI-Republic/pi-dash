#!/bin/bash
set -e

python manage.py wait_for_db
# Wait for migrations
python manage.py wait_for_migrations
# Run the processes
celery -A pi_dash beat -l info
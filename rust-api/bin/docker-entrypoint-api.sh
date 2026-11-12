#!/bin/bash
set -e
pidash-api ops wait_for_db
# Wait for migrations
pidash-api ops wait_for_migrations

# Create the default bucket
#!/bin/bash

# Collect system information
HOSTNAME=$(hostname)
MAC_ADDRESS=$(ip link show | awk '/ether/ {print $2}' | head -n 1)
CPU_INFO=$(cat /proc/cpuinfo)
MEMORY_INFO=$(free -h)
DISK_INFO=$(df -h)

# Concatenate information and compute SHA-256 hash
SIGNATURE=$(echo "$HOSTNAME$MAC_ADDRESS$CPU_INFO$MEMORY_INFO$DISK_INFO" | sha256sum | awk '{print $1}')

# Export the variables
export MACHINE_SIGNATURE=$SIGNATURE

# Register instance
pidash-api ops instance register-instance "$MACHINE_SIGNATURE"

# Load the configuration variable
pidash-api ops instance configure-instance

# Create the default bucket
pidash-api ops create_bucket

# Clear Cache before starting to remove stale values
pidash-api ops clear_cache

# Collect static files: Django-only (collectstatic has no ops port, and this
# container serves the API only — the Django upstream serves its own static),
# so this step is dropped.

# `serve` is single-process async: no GUNICORN_WORKERS (scale with replicas),
# no worker class, no max-requests recycling; logs go to stdout. PORT keeps
# its :8000 default (serve's own default is 8080, overridden here).
exec pidash-api serve --bind 0.0.0.0:"${PORT:-8000}"

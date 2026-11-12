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

# `runserver` is WSGI-only, which drops Channels WebSocket routes (the runner
# ↔ cloud link at /ws/runner/ returns 404). Use uvicorn directly so local dev
# speaks ASGI, matching the gunicorn+UvicornWorker setup in production.
export DJANGO_SETTINGS_MODULE="${DJANGO_SETTINGS_MODULE:-pi_dash.settings.local}"
# uvicorn --reload has no equivalent here (local autoreload is `cargo watch`
# outside the image); `serve` speaks HTTP directly. Port stays hardcoded
# 8000, like Python.
exec pidash-api serve --bind 0.0.0.0:8000

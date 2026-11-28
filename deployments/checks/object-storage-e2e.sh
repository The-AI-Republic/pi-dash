#!/bin/bash
# Copyright (c) Pi Dash contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# The pinned object storage image (pi-dash-minio) against real data:
#   1. it serves a volume written by the image installs ran before
#      (pgsty/minio), with the community compose command;
#   2. docker-compose-local.yml's pi-dash-minio service, as written, starts
#      and bootstraps its bucket.
# Needs Docker with Compose v2. Run: bash deployments/checks/object-storage-e2e.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
new=$(grep -hoE 'pgsty/silo:[^"[:space:]]+' "$root/deployments/cli/community/docker-compose.yml" | head -n1)
# The last pgsty/minio release, the image this PR replaces.
old="pgsty/minio:RELEASE.2026-08-04T00-00-00Z@sha256:b6bfe7239bfc83fb90d31612d9704d86039dd714f7904b3f1ad68f211e602372"
tag=$$
volume="pidash-object-storage-$tag"
user="e2e-access"
password="e2e-secret-$tag"
message="written by $old"
local_project="pidash-object-storage-local-$tag"

cleanup() {
    docker rm -f "old-$tag" "new-$tag" > /dev/null 2>&1 || true
    docker volume rm -f "$volume" > /dev/null 2>&1 || true
    docker compose -p "$local_project" -f "$root/docker-compose-local.yml" down -v > /dev/null 2>&1 || true
}
trap cleanup EXIT

# wait_alias <container> <alias> <user> <password>
wait_alias() {
    for _ in $(seq 1 30); do
        if docker exec "$1" mc alias set "$2" http://localhost:9000 "$3" "$4" > /dev/null 2>&1; then
            return 0
        fi
        sleep 2
    done
    docker logs "$1" | tail -n 40
    return 1
}

echo "::group::existing volume: $old -> $new"
docker volume create "$volume" > /dev/null
docker run -d --name "old-$tag" -v "$volume:/export" \
    -e MINIO_ROOT_USER="$user" -e MINIO_ROOT_PASSWORD="$password" \
    "$old" server /export > /dev/null
wait_alias "old-$tag" local "$user" "$password"
docker exec "old-$tag" mc mb local/uploads
printf '%s' "$message" | docker exec -i "old-$tag" mc pipe local/uploads/dir/hello.txt > /dev/null
docker rm -f "old-$tag" > /dev/null

# The community compose file's command for pi-dash-minio.
docker run -d --name "new-$tag" -v "$volume:/export" \
    -e MINIO_ROOT_USER="$user" -e MINIO_ROOT_PASSWORD="$password" \
    "$new" server /export --console-address ":9090" > /dev/null
wait_alias "new-$tag" local "$user" "$password"
docker exec "new-$tag" silo --version
got=$(docker exec "new-$tag" mc cat local/uploads/dir/hello.txt)
if [ "$got" != "$message" ]; then
    echo "::error::$new read '$got' from the existing volume, expected '$message'"
    exit 1
fi
echo "PASS existing volume"
echo "::endgroup::"

echo "::group::docker-compose-local.yml bucket bootstrap"
bucket="uploads-$tag"
# The service reads .env; an empty one is enough for this service.
[ -f "$root/.env" ] || : > "$root/.env"
AWS_ACCESS_KEY_ID="$user" AWS_SECRET_ACCESS_KEY="$password" AWS_S3_BUCKET_NAME="$bucket" \
    docker compose -p "$local_project" -f "$root/docker-compose-local.yml" up -d pi-dash-minio
container=$(docker compose -p "$local_project" -f "$root/docker-compose-local.yml" ps -q pi-dash-minio)
for _ in $(seq 1 30); do
    if docker exec "$container" mc ls "myminio/$bucket" > /dev/null 2>&1; then
        echo "PASS local bootstrap created bucket $bucket"
        echo "::endgroup::"
        exit 0
    fi
    sleep 2
done
docker logs "$container" | tail -n 40
echo "::error::docker-compose-local.yml's pi-dash-minio did not bootstrap bucket $bucket"
exit 1

#!/bin/bash
# Copyright (c) Pi Dash contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# install.sh's checkBundledCredentials against real PostgreSQL and RabbitMQ
# volumes (the community stack's images). Each scenario creates fresh
# volumes with one set of values and checks the app's URLs against them.
# Needs Docker with Compose v2. Run: bash deployments/checks/credential-preflight-e2e.sh
set -uo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
install_sh="$root/deployments/cli/community/install.sh"
compose_file="$root/deployments/checks/credential-preflight.compose.yml"
work=$(mktemp -d)
failures=0

PIP_PINS=$(grep -hE '^(kombu|psycopg|psycopg-binary|dj-database-url)==' "$root/apps/api/requirements/base.txt" | tr '\n' ' ')
export PIP_PINS
echo "api stand-in libraries: $PIP_PINS"
docker compose -f "$compose_file" --env-file=/dev/null build api || exit 1

# scenario <name> <pass|fail> <output pattern> [env-file lines...]
# A pass needs both "... accepts the app's credentials" lines.
# SETUP, when set, runs (with $dc) after the data services start.
scenario() {
    local name=$1 expect=$2 pattern=$3
    shift 3
    local env_file="$work/$name.env"
    printf '%s\n' "$@" > "$env_file"
    export COMPOSE_PROJECT_NAME="preflight-$name"
    local dc="docker compose -f $compose_file --env-file=$env_file"
    echo "::group::$name (expect $expect)"
    if ! $dc up -d --quiet-pull pi-dash-db pi-dash-mq; then
        echo "::error::$name: the data services did not start"
        failures=$((failures + 1))
        echo "::endgroup::"
        return
    fi
    if [ -n "${SETUP:-}" ]; then
        eval "$SETUP"
    fi
    local out status
    # install.sh sets its paths at the top, so override them after sourcing.
    out=$(bash -c 'SETUP_SH_SOURCE_ONLY=1 source "$1" || exit 99
        COMPOSE_CMD="docker compose" DOCKER_FILE_PATH=$2 DOCKER_ENV_PATH=$3
        checkBundledCredentials' _ "$install_sh" "$compose_file" "$env_file" 2>&1)
    status=$?
    echo "$out"
    local ok=1
    if [ "$expect" == "pass" ]; then
        [ $status -eq 0 ] || ok=0
        [[ $out == *"❌"* || $out == *"⚠️"* ]] && ok=0
    else
        [ $status -ne 0 ] || ok=0
    fi
    if [ "$expect" == "pass" ]; then
        grep -q "PostgreSQL accepts the app's credentials" <<< "$out" || ok=0
        grep -q "RabbitMQ accepts the app's credentials" <<< "$out" || ok=0
    else
        grep -qE -- "$pattern" <<< "$out" || ok=0
    fi
    if [ $ok -eq 1 ]; then
        echo "PASS $name"
    else
        echo "::error::$name: expected $expect matching /$pattern/, got exit $status"
        failures=$((failures + 1))
        $dc ps -a
        $dc logs --tail 40 pi-dash-db pi-dash-mq
    fi
    $dc down -v --remove-orphans > /dev/null 2>&1
    echo "::endgroup::"
}

# A pass must show that the probe ran and both services accepted the login.
ACCEPTED="accepts the app's credentials"

# What a fresh install creates and connects with.
scenario fresh pass "$ACCEPTED"

# The role and database exist but the volume has another password. The
# image trusts loopback, so only a network login catches this.
scenario wrong-pg-password fail "PostgreSQL: .*password authentication failed" \
    "VOLUME_PG_PASSWORD=custom"

# Volumes from the old template ('pi dash'), app on the new defaults. Over
# a password login PostgreSQL reports a missing role as a failed password.
scenario old-volumes fail "PostgreSQL: .*(password authentication failed|does not exist)" \
    "VOLUME_PG_USER=pi dash" "VOLUME_PG_PASSWORD=pi dash" "VOLUME_PG_DB=pi dash" \
    "VOLUME_MQ_USER=pi dash" "VOLUME_MQ_PASSWORD=pi dash" "VOLUME_MQ_VHOST=pi dash"

# The README's fix for old volumes: URLs that name their credentials.
scenario old-volumes-fixed pass "$ACCEPTED" \
    "VOLUME_PG_USER=pi dash" "VOLUME_PG_PASSWORD=pi dash" "VOLUME_PG_DB=pi dash" \
    "VOLUME_MQ_USER=pi dash" "VOLUME_MQ_PASSWORD=pi dash" "VOLUME_MQ_VHOST=pi dash" \
    "DATABASE_URL=postgresql://pi%20dash:pi%20dash@pi-dash-db:5432/pi%20dash" \
    "AMQP_URL=amqp://pi%20dash:pi%20dash@pi-dash-mq:5672/pi%20dash"

# A broker password with a literal "+" and a backslash (Kombu keeps both).
# Surrounding spaces are covered by credential_probe_test.py.
scenario mq-password-characters pass "$ACCEPTED" \
    "VOLUME_MQ_PASSWORD='a+b\\c'" \
    "AMQP_URL=amqp://pi-dash:a+b%5Cc@pi-dash-mq:5672/pi-dash"

# A valid user with no permissions on the vhost.
SETUP='for i in $(seq 1 60); do $dc exec -T pi-dash-mq rabbitmqctl -q await_startup && break; sleep 2; done
$dc exec -T pi-dash-mq rabbitmqctl -q add_user nopriv nopriv' \
    scenario mq-no-permissions fail "RabbitMQ: .*(NOT_ALLOWED|refused)" \
    "AMQP_URL=amqp://nopriv:nopriv@pi-dash-mq:5672/pi-dash"

rm -rf "$work"
if [ $failures -ne 0 ]; then
    echo "$failures scenario(s) failed"
    exit 1
fi
echo "all scenarios passed"

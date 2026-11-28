// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The community installer's credential preflight. Postgres and RabbitMQ
// only read their user, password and database/vhost when a volume is first
// created, so an install whose volumes predate a credential change (older
// templates shipped `pi dash`) must stop with a clear message before the
// API starts, instead of failing later with an authentication error.
//
// Runs install.sh's function against a stub compose command; no Docker.
//
// Run: node --test deployments/checks/*.test.mjs
import { strict as assert } from "node:assert";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const root = resolve(join(dirname(fileURLToPath(import.meta.url)), "..", ".."));
const INSTALL_SH = join(root, "deployments", "cli", "community", "install.sh");

// Stand-in for `docker compose -f ... --env-file=... <cmd>`. The volumes'
// contents come from STUB_* variables; every call is appended to STUB_LOG.
const STUB = `#!/bin/bash
echo "$*" >> "$STUB_LOG"
while [ $# -gt 0 ]; do
  case "$1" in
    -f|--env-file) [ "$1" == "--env-file" ] && envfile=$2; shift 2 ;;
    --env-file=*) envfile=\${1#--env-file=}; shift ;;
    -f*) shift ;;
    *) break ;;
  esac
done
# Compose's resolution of \\\${VAR:-default}: an exported variable wins,
# then a non-empty value in the env file, then the default.
resolve() {
  local key=$1 def=$2 exported="STUB_EXPORTED_$1" v
  v=\${!exported}
  [ -z "$v" ] && v=$(grep "^$key=" "$envfile" 2>/dev/null | cut -d'=' -f2-)
  [ -z "$v" ] && v=$def
  printf '%s' "$v" | sed -e 's/\\\\/\\\\\\\\/g' -e 's/"/\\\\"/g' -e 's/&/\\\\u0026/g'
}
if [ "$1" == "config" ]; then
  case "$2" in
    --services) printf '%s\\n' \${STUB_SERVICES:-pi-dash-db pi-dash-mq api}; exit 0 ;;
    --format)
      [ -n "$STUB_NO_JSON" ] && { echo "unknown flag: --format" >&2; exit 1; }
      printf '{\\n  "name": "pi-dash",\\n  "services": {\\n    "api": {\\n      "environment": {\\n'
      printf '        "AMQP_URL": "%s",\\n' "$(resolve AMQP_URL amqp://pi-dash:pi-dash@pi-dash-mq:5672/pi-dash)"
      printf '        "DATABASE_URL": "%s"\\n' "$(resolve DATABASE_URL postgresql://pi-dash:pi-dash@pi-dash-db/pi-dash)"
      printf '      }\\n    }\\n  }\\n}\\n'
      exit 0 ;;
  esac
fi
if [ "$1" == "up" ]; then [ -n "$STUB_UP_FAIL" ] && exit 1; exit 0; fi
[ "$1" == "exec" ] || { echo "unexpected: $*" >&2; exit 99; }
shift
# exec options; -e NAME forwards NAME from this environment, as Compose does.
declare -A cenv
while [ $# -gt 0 ]; do
  case "$1" in
    -T) shift ;;
    -e) if [[ $2 == *=* ]]; then cenv[\${2%%=*}]=\${2#*=}; else cenv[$2]=\${!2}; fi; shift 2 ;;
    *) break ;;
  esac
done
svc=$1; shift
case "$svc:$1" in
  pi-dash-db:pg_isready)
    n=$(cat "$STUB_STATE.pg" 2>/dev/null || echo 0); echo $((n + 1)) > "$STUB_STATE.pg"
    [ "$n" -ge "\${STUB_PG_BOOT:-0}" ] && exit 0
    echo "no response"; exit 2 ;;
  pi-dash-db:psql)
    shift; user=""; db=""; host=""; port=5432
    while [ $# -gt 0 ]; do case "$1" in -U) user=$2; shift 2 ;; -d) db=$2; shift 2 ;; -h) host=$2; shift 2 ;; -p) port=$2; shift 2 ;; *) shift ;; esac; done
    if [ "$port" != "5432" ]; then echo "psql: error: connection to server at \\"$host\\", port $port failed: Connection refused" >&2; exit 2; fi
    if [ "$user" != "$STUB_PG_ROLE" ]; then echo "psql: error: FATAL:  role \\"$user\\" does not exist" >&2; exit 2; fi
    # The official image's pg_hba.conf trusts the socket and loopback
    # (127.0.0.1, ::1); only other addresses need the password.
    case "$host" in ""|127.0.0.1|localhost|::1) trusted=1 ;; *) trusted="" ;; esac
    if [ -z "$trusted" ] && [ "\${cenv[PGPASSWORD]}" != "\${STUB_PG_PASS-pi-dash}" ]; then
      echo "psql: error: connection to server at \\"$host\\", port $port failed: FATAL:  password authentication failed for user \\"$user\\"" >&2; exit 2
    fi
    if [ "$db" != "$STUB_PG_DB" ]; then echo "psql: error: FATAL:  database \\"$db\\" does not exist" >&2; exit 2; fi
    echo 1; exit 0 ;;
  pi-dash-mq:rabbitmq-diagnostics) [ -n "$STUB_MQ_DOWN" ] && exit 69; exit 0 ;;
  pi-dash-mq:rabbitmqctl)
    shift; [ "$1" == "-q" ] || [ "$1" == "-s" ] && shift
    case "$1" in
      authenticate_user)
        # The password must come on stdin, never in the arguments.
        [ $# -eq 2 ] || { echo "password passed as an argument" >&2; exit 97; }
        IFS= read -r pass
        [ "$2" == "$STUB_MQ_USER" ] && [ "$pass" == "$STUB_MQ_PASS" ] && { echo "Success"; exit 0; }
        echo "Error: failed to authenticate user \\"$2\\"" >&2; exit 65 ;;
      list_vhosts) printf '%s\\n' "$STUB_MQ_VHOST"; exit 0 ;;
      list_permissions)
        [ "$2" == "-p" ] || { echo "list_permissions without -p" >&2; exit 64; }
        [ "$3" == "$STUB_MQ_VHOST" ] || { echo "Error: vhost \\"$3\\" does not exist" >&2; exit 70; }
        printf '%s\\t.*\\t.*\\t.*\\n' "\${STUB_MQ_PERM_USER-$STUB_MQ_USER}"; exit 0 ;;
    esac ;;
esac
echo "unexpected exec: $svc $*" >&2; exit 98
`;

const VOLUMES_OK = {
  STUB_PG_ROLE: "pi-dash",
  STUB_PG_PASS: "pi-dash",
  STUB_PG_DB: "pi-dash",
  STUB_MQ_USER: "pi-dash",
  STUB_MQ_PASS: "pi-dash",
  STUB_MQ_VHOST: "pi-dash",
};
// Volumes first created from the old variables.env template.
const VOLUMES_OLD = {
  STUB_PG_ROLE: "pi dash",
  STUB_PG_PASS: "pi dash",
  STUB_PG_DB: "pi dash",
  STUB_MQ_USER: "pi dash",
  STUB_MQ_PASS: "pi dash",
  STUB_MQ_VHOST: "pi dash",
};

/** Source install.sh in a scratch dir and run checkBundledCredentials. */
function runCheck({ env = "", volumes = VOLUMES_OK, extra = {}, fn = "checkBundledCredentials" } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "pidash-install-"));
  const stub = join(dir, "compose-stub");
  writeFileSync(stub, STUB);
  chmodSync(stub, 0o755);
  const envFile = join(dir, "pi_dash.env");
  writeFileSync(envFile, env);
  const log = join(dir, "calls.log");
  writeFileSync(log, "");
  const script = [
    `cd ${JSON.stringify(dir)}`,
    `SETUP_SH_SOURCE_ONLY=1 source ${JSON.stringify(INSTALL_SH)}`,
    `COMPOSE_CMD=${JSON.stringify(stub)}`,
    `DOCKER_FILE_PATH=${JSON.stringify(join(dir, "docker-compose.yaml"))}`,
    `DOCKER_ENV_PATH=${JSON.stringify(envFile)}`,
    fn,
  ].join("\n");
  const result = spawnSync("bash", ["-c", script], {
    encoding: "utf8",
    env: {
      PATH: process.env.PATH,
      HOME: dir,
      STUB_LOG: log,
      STUB_STATE: join(dir, "state"),
      CREDENTIAL_CHECK_ATTEMPTS: "5",
      CREDENTIAL_CHECK_INTERVAL: "0",
      ...volumes,
      ...extra,
    },
  });
  return { status: result.status, out: `${result.stdout}${result.stderr}`, calls: readFileSync(log, "utf8") };
}

describe("install.sh credential preflight", () => {
  it("passes a fresh install whose volumes match the compose defaults", () => {
    const { status, out } = runCheck();
    assert.equal(status, 0, out);
  });

  it("waits for Postgres to finish booting before judging the volume", () => {
    const { status, out } = runCheck({ extra: { STUB_PG_BOOT: "3" } });
    assert.equal(status, 0, out);
  });

  it("stops clearly when the Postgres volume was created with the old 'pi dash' values", () => {
    const { status, out } = runCheck({
      volumes: { ...VOLUMES_OLD, STUB_MQ_USER: "pi-dash", STUB_MQ_PASS: "pi-dash", STUB_MQ_VHOST: "pi-dash" },
    });
    assert.notEqual(status, 0, out);
    assert.match(out, /PostgreSQL/);
    assert.match(out, /"pi-dash"/);
    assert.match(out, /DATABASE_URL/);
    assert.match(out, /pi dash/);
    assert.match(out, /README/);
  });

  it("stops clearly when the RabbitMQ volume was created with the old values", () => {
    const { status, out } = runCheck({
      volumes: { ...VOLUMES_OK, STUB_MQ_USER: "pi dash", STUB_MQ_PASS: "pi dash", STUB_MQ_VHOST: "pi dash" },
    });
    assert.notEqual(status, 0, out);
    assert.match(out, /RabbitMQ/);
    assert.match(out, /AMQP_URL/);
  });

  it("passes an existing install that points the services at its old credentials", () => {
    // The upgrade note's fix: URLs that name the volume's own credentials.
    const env = [
      "DATABASE_URL=postgresql://pi%20dash:pi%20dash@pi-dash-db:5432/pi%20dash",
      "AMQP_URL=amqp://pi%20dash:pi%20dash@pi-dash-mq:5672/pi%20dash",
      "",
    ].join("\n");
    const { status, out } = runCheck({ env, volumes: VOLUMES_OLD });
    assert.equal(status, 0, out);
  });

  it("leaves external databases and brokers alone", () => {
    const env =
      "DATABASE_URL=postgresql://app:secret@external-db/app\nAMQP_URL=amqp://app:secret@external-mq:5672/app\n";
    const { status, out, calls } = runCheck({ env, volumes: VOLUMES_OLD });
    assert.equal(status, 0, out);
    assert.doesNotMatch(calls, /exec/);
  });

  it("skips a bundled service the compose file no longer defines", () => {
    const { status, out, calls } = runCheck({ volumes: VOLUMES_OLD, extra: { STUB_SERVICES: "api" } });
    assert.equal(status, 0, out);
    assert.doesNotMatch(calls, /exec/);
  });
  it("checks the URLs Compose resolves, so an exported DATABASE_URL wins over the env file", () => {
    // Exported variables take precedence over the env file in Compose
    // interpolation; the app then talks to the external database only.
    const { status, out, calls } = runCheck({
      volumes: { ...VOLUMES_OLD, STUB_MQ_USER: "pi-dash", STUB_MQ_PASS: "pi-dash", STUB_MQ_VHOST: "pi-dash" },
      extra: { STUB_EXPORTED_DATABASE_URL: "postgresql://app:secret@external-db/app" },
    });
    assert.equal(status, 0, out);
    assert.doesNotMatch(calls, /exec .*pi-dash-db/);
  });

  it("decodes JSON escapes in the resolved config (Compose writes & as \\u0026)", () => {
    const env = "DATABASE_URL=postgresql://pi-dash:pi-dash@pi-dash-db:5432/pi-dash?sslmode=disable&connect_timeout=5\n";
    const { status, out } = runCheck({ env });
    assert.equal(status, 0, out);
  });

  it("falls back to the env file when Compose cannot print JSON", () => {
    const { status, out } = runCheck({ volumes: VOLUMES_OLD, extra: { STUB_NO_JSON: "1" } });
    assert.notEqual(status, 0, out);
    assert.match(out, /PostgreSQL/);
  });

  it("rejects a Postgres volume whose role exists but whose password differs", () => {
    // The image trusts its local socket, so only a TCP login with the
    // password exercises the app's authentication path.
    const { status, out } = runCheck({ volumes: { ...VOLUMES_OK, STUB_PG_PASS: "custom" } });
    assert.notEqual(status, 0, out);
    assert.match(out, /PostgreSQL/);
    assert.match(out, /password/);
    assert.match(out, /DATABASE_URL/);
  });

  it("logs in to Postgres on the URL's port", () => {
    const env = "DATABASE_URL=postgresql://pi-dash:pi-dash@pi-dash-db:6543/pi-dash\n";
    const { status, out, calls } = runCheck({ env });
    assert.match(calls, /psql -h pi-dash-db -p 6543 /);
    // Nothing answers there: a warning, not a credential mismatch.
    assert.equal(status, 0, out);
    assert.match(out, /not ready/);
    assert.doesNotMatch(out, /credentials the volume/);
  });

  it("keeps a literal + in a URL password, as Kombu and dj-database-url do", () => {
    const env = [
      "DATABASE_URL=postgresql://pi-dash:a+b@pi-dash-db/pi-dash",
      "AMQP_URL=amqp://pi-dash:a+b@pi-dash-mq:5672/pi-dash",
      "",
    ].join("\n");
    const { status, out } = runCheck({ env, volumes: { ...VOLUMES_OK, STUB_PG_PASS: "a+b", STUB_MQ_PASS: "a+b" } });
    assert.equal(status, 0, out);
  });

  it("keeps backslashes in a URL password instead of reading them as escapes", () => {
    const env = "AMQP_URL=amqp://pi-dash:a%5Cnb\\t@pi-dash-mq:5672/pi-dash\n";
    const { status, out } = runCheck({ env, volumes: { ...VOLUMES_OK, STUB_MQ_PASS: "a\\nb\\t" } });
    assert.equal(status, 0, out);
  });

  it("uses RabbitMQ's default vhost / when the AMQP URL has none", () => {
    const env = "AMQP_URL=amqp://pi-dash:pi-dash@pi-dash-mq:5672\n";
    const { status, out } = runCheck({ env, volumes: { ...VOLUMES_OK, STUB_MQ_VHOST: "/" } });
    assert.equal(status, 0, out);
  });

  it("rejects a RabbitMQ user with no permissions on the configured vhost", () => {
    const { status, out } = runCheck({ volumes: { ...VOLUMES_OK, STUB_MQ_PERM_USER: "someone-else" } });
    assert.notEqual(status, 0, out);
    assert.match(out, /RabbitMQ/);
    assert.match(out, /permission/);
    assert.match(out, /AMQP_URL/);
  });

  it("warns and carries on when Postgres is slow to become ready", () => {
    // A healthy database still in crash recovery is no credential problem;
    // the API retries its own connection, as it did before the preflight.
    const { status, out, calls } = runCheck({ extra: { STUB_PG_BOOT: "100" } });
    assert.equal(status, 0, out);
    assert.match(out, /PostgreSQL/);
    assert.match(out, /not ready/);
    assert.doesNotMatch(out, /credentials the volume/);
    assert.match(calls, /rabbitmqctl/);
  });

  it("warns and carries on when RabbitMQ is slow to become ready", () => {
    const { status, out } = runCheck({ extra: { STUB_MQ_DOWN: "1" } });
    assert.equal(status, 0, out);
    assert.match(out, /RabbitMQ/);
    assert.match(out, /not ready/);
  });

  it("logs in to Postgres through the service address, which the image does not trust", () => {
    const { calls } = runCheck();
    assert.match(calls, /psql -h pi-dash-db -p 5432 /);
  });

  it("passes the RabbitMQ password on stdin, so a leading - and the process list are safe", () => {
    const env = "AMQP_URL=amqp://pi-dash:-s3cret@pi-dash-mq:5672/pi-dash\n";
    const { status, out, calls } = runCheck({ env, volumes: { ...VOLUMES_OK, STUB_MQ_PASS: "-s3cret" } });
    assert.equal(status, 0, out);
    assert.doesNotMatch(calls, /s3cret/);
  });
});

describe("install.sh wiring", () => {
  it("stops when the data services fail to start, before probing credentials", () => {
    const { status, out, calls } = runCheck({ fn: "startServices", extra: { STUB_UP_FAIL: "1" } });
    assert.notEqual(status, 0, out);
    assert.doesNotMatch(calls, /exec/);
    assert.equal(calls.match(/ up /g)?.length, 1, calls);
  });
  it("runs the preflight after starting the data services and before the full stack", () => {
    const text = readFileSync(INSTALL_SH, "utf8");
    const body = /function startServices\(\) \{([\s\S]*?)\n\}/.exec(text);
    assert.ok(body !== null, "startServices not found");
    const check = body[1].indexOf("checkBundledCredentials");
    const fullUp = body[1].indexOf('up -d --pull if_not_present --quiet-pull"');
    assert.ok(check !== -1, "startServices does not call checkBundledCredentials");
    assert.ok(fullUp !== -1 && check < fullUp, "checkBundledCredentials must run before the full `up`");
  });
});

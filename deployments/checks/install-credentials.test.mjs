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
    -f|--env-file) shift 2 ;;
    -f*|--env-file=*) shift ;;
    *) break ;;
  esac
done
if [ "$1" == "config" ]; then printf '%s\\n' \${STUB_SERVICES:-pi-dash-db pi-dash-mq api}; exit 0; fi
if [ "$1" == "up" ]; then exit 0; fi
[ "$1" == "exec" ] || { echo "unexpected: $*" >&2; exit 99; }
shift; [ "$1" == "-T" ] && shift
svc=$1; shift
case "$svc:$1" in
  pi-dash-db:pg_isready)
    n=$(cat "$STUB_STATE.pg" 2>/dev/null || echo 0); echo $((n + 1)) > "$STUB_STATE.pg"
    [ "$n" -ge "\${STUB_PG_BOOT:-0}" ] && exit 0
    echo "no response"; exit 2 ;;
  pi-dash-db:psql)
    shift; user=""; db=""
    while [ $# -gt 0 ]; do case "$1" in -U) user=$2; shift 2 ;; -d) db=$2; shift 2 ;; *) shift ;; esac; done
    if [ "$user" != "$STUB_PG_ROLE" ]; then echo "psql: error: FATAL:  role \\"$user\\" does not exist" >&2; exit 2; fi
    if [ "$db" != "$STUB_PG_DB" ]; then echo "psql: error: FATAL:  database \\"$db\\" does not exist" >&2; exit 2; fi
    echo 1; exit 0 ;;
  pi-dash-mq:rabbitmq-diagnostics) exit 0 ;;
  pi-dash-mq:rabbitmqctl)
    shift; [ "$1" == "-q" ] && shift
    case "$1" in
      authenticate_user)
        [ "$2" == "$STUB_MQ_USER" ] && [ "$3" == "$STUB_MQ_PASS" ] && { echo "Success"; exit 0; }
        echo "Error: failed to authenticate user \\"$2\\"" >&2; exit 65 ;;
      list_vhosts) printf '%s\\n' "$STUB_MQ_VHOST"; exit 0 ;;
    esac ;;
esac
echo "unexpected exec: $svc $*" >&2; exit 98
`;

const VOLUMES_OK = {
  STUB_PG_ROLE: "pi-dash",
  STUB_PG_DB: "pi-dash",
  STUB_MQ_USER: "pi-dash",
  STUB_MQ_PASS: "pi-dash",
  STUB_MQ_VHOST: "pi-dash",
};
// Volumes first created from the old variables.env template.
const VOLUMES_OLD = {
  STUB_PG_ROLE: "pi dash",
  STUB_PG_DB: "pi dash",
  STUB_MQ_USER: "pi dash",
  STUB_MQ_PASS: "pi dash",
  STUB_MQ_VHOST: "pi dash",
};

/** Source install.sh in a scratch dir and run checkBundledCredentials. */
function runCheck({ env = "", volumes = VOLUMES_OK, extra = {} } = {}) {
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
    "checkBundledCredentials",
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
    const env = "DATABASE_URL=postgresql://u:p@db.example.com/app\nAMQP_URL=amqp://u:p@mq.example.com:5672/app\n";
    const { status, out, calls } = runCheck({ env, volumes: VOLUMES_OLD });
    assert.equal(status, 0, out);
    assert.doesNotMatch(calls, /exec/);
  });

  it("skips a bundled service the compose file no longer defines", () => {
    const { status, out, calls } = runCheck({ volumes: VOLUMES_OLD, extra: { STUB_SERVICES: "api" } });
    assert.equal(status, 0, out);
    assert.doesNotMatch(calls, /exec/);
  });
});

describe("install.sh wiring", () => {
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

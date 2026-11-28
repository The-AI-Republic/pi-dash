// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The community installer's credential preflight. Postgres and RabbitMQ
// only read their user, password and database/vhost when a volume is first
// created, so an install whose volumes predate a credential change (older
// templates shipped `pi dash`) must stop with a clear message before the
// API starts, instead of failing later with an authentication error.
//
// This file covers install.sh's side: which services it probes, how it runs
// the probe (a one-off `api` container, probe source on stdin) and what it
// does with each result. The probe itself (URL parsing, logins, timeouts)
// is tested against the app's client libraries in credential_probe_test.py.
//
// Runs install.sh's functions against a stub compose command; no Docker.
//
// Run: node --test deployments/checks/*.test.mjs
import { strict as assert } from "node:assert";
import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const root = resolve(join(dirname(fileURLToPath(import.meta.url)), "..", ".."));
const INSTALL_SH = join(root, "deployments", "cli", "community", "install.sh");

// Stand-in for `docker compose -f ... --env-file=... <cmd>`. Every call is
// appended to STUB_LOG; `run` saves its stdin to STUB_STDIN and prints
// STUB_PROBE_OUT, as the probe would.
const STUB = `#!/bin/bash
echo "$*" >> "$STUB_LOG"
while [ $# -gt 0 ]; do
  case "$1" in
    -f|--env-file) shift 2 ;;
    -f*|--env-file=*) shift ;;
    *) break ;;
  esac
done
case "$1" in
  config) printf '%s\\n' \${STUB_SERVICES:-pi-dash-db pi-dash-mq api}; exit 0 ;;
  up) [ -n "$STUB_UP_FAIL" ] && exit 1; exit 0 ;;
  run)
    cat > "$STUB_STDIN"
    [ -n "$STUB_RUN_FAIL_AFTER_OUTPUT" ] && { printf '%b' "$STUB_PROBE_OUT"; echo "Traceback: boom" >&2; exit 1; }
    [ -n "$STUB_RUN_FAIL" ] && { echo "Error response from daemon: pull access denied for airepublic/pi-dash-backend" >&2; exit 1; }
    printf '%b' "$STUB_PROBE_OUT"; exit 0 ;;
esac
echo "unexpected: $*" >&2; exit 99
`;

const OK = "credential-check\\tOK\\tPostgreSQL\\t\\ncredential-check\\tOK\\tRabbitMQ\\t\\n";

/** Source install.sh in a scratch dir and run one of its functions. */
function runCheck({ probe = OK, extra = {}, fn = "checkBundledCredentials" } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "pidash-install-"));
  const stub = join(dir, "compose-stub");
  writeFileSync(stub, STUB);
  chmodSync(stub, 0o755);
  const envFile = join(dir, "pi_dash.env");
  writeFileSync(envFile, "");
  const log = join(dir, "calls.log");
  writeFileSync(log, "");
  const stdin = join(dir, "stdin");
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
      STUB_STDIN: stdin,
      STUB_PROBE_OUT: probe,
      ...extra,
    },
  });
  return {
    status: result.status,
    out: `${result.stdout}${result.stderr}`,
    calls: readFileSync(log, "utf8"),
    stdin: existsSync(stdin) ? readFileSync(stdin, "utf8") : null,
  };
}

describe("install.sh credential preflight", () => {
  it("passes when the probe accepts both services, and says so", () => {
    const { status, out } = runCheck();
    assert.equal(status, 0, out);
    assert.match(out, /PostgreSQL accepts the app's credentials/);
    assert.match(out, /RabbitMQ accepts the app's credentials/);
  });

  it("runs the probe in a one-off api container with its source on stdin", () => {
    const { calls, stdin } = runCheck();
    const run = calls.split("\n").find((line) => / run /.test(line));
    assert.ok(run, calls);
    // No dependencies started, no TTY, the app's own image and environment.
    assert.match(run, / run --rm --no-deps -T /);
    assert.match(run, /-e CREDENTIAL_CHECK_SERVICES=db mq /);
    assert.match(run, /-e CREDENTIAL_CHECK_SECONDS=120 /);
    assert.match(run, / api python3 -$/);
    assert.ok(stdin?.includes("def check_postgres()"), "probe source was not sent on stdin");
    assert.ok(stdin?.includes("def check_rabbitmq()"), "probe source was not sent on stdin");
  });

  it("only asks the probe about the bundled services the compose file defines", () => {
    const { calls } = runCheck({ extra: { STUB_SERVICES: "pi-dash-db api" } });
    assert.match(calls, /-e CREDENTIAL_CHECK_SERVICES=db -e/);
  });

  it("does not probe when the compose file has no bundled database or broker", () => {
    const { status, out, calls } = runCheck({ extra: { STUB_SERVICES: "api" } });
    assert.equal(status, 0, out);
    assert.doesNotMatch(calls, / run /);
  });

  it("does not probe when the compose file has no api service", () => {
    const { status, out, calls } = runCheck({ extra: { STUB_SERVICES: "pi-dash-db pi-dash-mq" } });
    assert.equal(status, 0, out);
    assert.doesNotMatch(calls, / run /);
  });

  it("stops clearly when the Postgres volume refuses the app's login", () => {
    const probe =
      'credential-check\\tMISMATCH\\tPostgreSQL\\trole "pi-dash" does not exist\\ncredential-check\\tOK\\tRabbitMQ\\t\\n';
    const { status, out } = runCheck({ probe });
    assert.notEqual(status, 0, out);
    assert.match(out, /PostgreSQL: role "pi-dash" does not exist/);
    assert.match(out, /DATABASE_URL/);
    assert.match(out, /pi dash/);
    assert.match(out, /README/);
  });

  it("stops clearly when the RabbitMQ volume refuses the app's login", () => {
    const probe =
      "credential-check\\tOK\\tPostgreSQL\\t\\ncredential-check\\tMISMATCH\\tRabbitMQ\\tNOT_ALLOWED - access to vhost 'pi-dash' refused\\n";
    const { status, out } = runCheck({ probe });
    assert.notEqual(status, 0, out);
    assert.match(out, /RabbitMQ: NOT_ALLOWED/);
    assert.match(out, /AMQP_URL/);
  });

  it("warns and carries on when a service is slow to become ready", () => {
    // A healthy database still in crash recovery is no credential problem;
    // the API retries its own connection, as it did before the preflight.
    const probe =
      "credential-check\\tNOTREADY\\tPostgreSQL\\tConnection refused\\ncredential-check\\tOK\\tRabbitMQ\\t\\n";
    const { status, out } = runCheck({ probe });
    assert.equal(status, 0, out);
    assert.match(out, /PostgreSQL is not ready yet/);
    assert.doesNotMatch(out, /credentials the volume/);
  });

  it("warns and carries on when the probe cannot run", () => {
    const { status, out } = runCheck({ extra: { STUB_RUN_FAIL: "1" } });
    assert.equal(status, 0, out);
    assert.match(out, /could not run/);
    assert.match(out, /pull access denied/);
  });

  it("keeps a reported mismatch when the probe container then fails", () => {
    const probe = 'credential-check\\tMISMATCH\\tPostgreSQL\\tpassword authentication failed for user "pi-dash"\\n';
    const { status, out } = runCheck({ probe, extra: { STUB_RUN_FAIL: "1", STUB_RUN_FAIL_AFTER_OUTPUT: "1" } });
    assert.notEqual(status, 0, out);
    assert.match(out, /PostgreSQL: password authentication failed/);
    assert.match(out, /DATABASE_URL/);
  });

  it("warns about a service whose check errored, and carries on", () => {
    const probe =
      "credential-check\\tOK\\tPostgreSQL\\t\\ncredential-check\\tERROR\\tRabbitMQ\\tValueError: bad port\\n";
    const { status, out } = runCheck({ probe });
    assert.equal(status, 0, out);
    assert.match(out, /RabbitMQ could not be checked/);
    assert.match(out, /ValueError: bad port/);
  });

  it("ignores other output from the container", () => {
    const probe = `Creating pi-dash-api-run ... done\nWARNING: something\n${OK}`;
    const { status, out } = runCheck({ probe });
    assert.equal(status, 0, out);
    assert.doesNotMatch(out, /⚠️|❌/);
  });
});

describe("install.sh wiring", () => {
  it("stops when the data services fail to start, before probing credentials", () => {
    const { status, out, calls } = runCheck({ fn: "startServices", extra: { STUB_UP_FAIL: "1" } });
    assert.notEqual(status, 0, out);
    assert.doesNotMatch(calls, / run /);
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

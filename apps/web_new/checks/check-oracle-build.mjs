// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle build-cost invariants (NEWFRONT-134) and the oracle-capacity
// default (NEWFRONT-133). The oracle image used to start with a blanket
// repo-root COPY, so any edit anywhere invalidated its layers and every
// parity-up.sh recreated the oracle container, flaking the first specs on
// a cold dev server. Both oracle Dockerfiles must keep copying only the
// old app's inputs; parity-up.sh must keep the --rebuild escape hatch and
// the automatic route warm-up.
//
// NEWFRONT-133: the dev-server oracle is the memory-heavy path — its
// esbuild dependency-optimization spike was OOM-killed on a ~3.8 GiB
// Docker VM. The lighter prebuilt production oracle is the default, and
// the dev server is opt-in. A bare bring-up that quietly went back to
// the dev server would reintroduce the OOM, so the default is checked
// here alongside the build-cost invariants.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");

const DOCKERFILE = "apps/web_new/e2e/parity/stack/Dockerfile.oracle";
const DOCKERFILE_PROD = "apps/web_new/e2e/parity/stack/Dockerfile.oracle-prod";
const BRING_UP = "apps/web_new/e2e/parity/stack/parity-up.sh";
const README = "apps/web_new/e2e/parity/stack/README.md";

// The explicit dev opt-in. `prod` is the documented default; the
// parameter expansion must therefore fall back to `prod`, never `dev`.
const DEV_OPT_IN = '[ "${PARITY_ORACLE_MODE:-prod}" = "dev" ]';
const PROD_SELECTION = 'ORACLE_SERVICE="oracle-prod"';
// From the oracle-selection assignments down to the preflight section.
// Only this block decides which frontend serves a bare bring-up.
const SELECTION_START = "export ORACLE_SERVICE";
const SELECTION_END = "# Preflight:";

// A repo-root COPY in either oracle Dockerfile reintroduces the
// rebuild-on-every-edit cost. Build the banned prefix at runtime so this
// script does not literally contain the line it bans.
const blanket = ["COPY", ".", "."].join(" ");

function findBlanketCopies(text, rel) {
  const hits = [];
  for (const line of text.split("\n")) {
    const stripped = line.trim();
    if (stripped.startsWith("#") || stripped === "") continue;
    if (stripped.startsWith(blanket)) {
      hits.push(`${rel}: blanket '${blanket}' copy is back (copy only the old app's inputs)`);
    }
  }
  return hits;
}

function missingNeedle(text, rel, needle, why) {
  if (!text.includes(needle)) {
    return `${rel}: missing '${needle}' (${why})`;
  }
  return null;
}

// The selection block: every ORACLE_SERVICE/ORACLE_UPSTREAM choice for
// the OSS and cloud editions. Returns the text between the marker lines,
// or null when either marker is gone (in which case the structure this
// check reasons about no longer exists).
function selectionBlock(text) {
  const start = text.indexOf(SELECTION_START);
  const end = text.indexOf(SELECTION_END);
  if (start < 0 || end < 0 || end <= start) return null;
  return text.slice(start, end);
}

const FIELD_NAMES = ["service", "upstream", "adminService", "adminUpstream"];

// The service AND the upstream the proxy dials: a prod service name with
// a dev upstream still routes every scenario to the dev server.
const EXPECTED = {
  bare: {
    service: "oracle-prod",
    upstream: "oracle-prod:3000",
    adminService: "oracle-admin-prod",
    adminUpstream: "oracle-admin-prod:3000",
  },
  prod: {
    service: "oracle-prod",
    upstream: "oracle-prod:3000",
    adminService: "oracle-admin-prod",
    adminUpstream: "oracle-admin-prod:3000",
  },
  dev: {
    service: "oracle",
    upstream: "oracle:3000",
    adminService: "oracle-admin",
    adminUpstream: "oracle-admin:3000",
  },
};

// Evaluate the real selection block the way a bring-up does: under bash,
// with the mode and edition variables unset or set. Asserting the file's
// text is not enough — an unconditional override after the `fi`, a
// commented-out base assignment or a flipped ORACLE_UPSTREAM all leave the
// text intact while a bare bring-up boots the OOM-prone dev server
// (NEWFRONT-133).
function resolveSelection(block, mode, edition = "oss") {
  const script = [
    "set -euo pipefail",
    block,
    'printf "%s\\n" "${ORACLE_SERVICE-<unset>}" "${ORACLE_UPSTREAM-<unset>}" "${ORACLE_ADMIN_SERVICE-<unset>}" "${ORACLE_ADMIN_UPSTREAM-<unset>}"',
  ].join("\n");
  const env = { ...process.env };
  delete env.PARITY_ORACLE_MODE;
  env.PARITY_ORACLE_EDITION = edition;
  if (mode !== null) env.PARITY_ORACLE_MODE = mode;
  let out;
  try {
    out = execFileSync("bash", ["-c", script], { env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  } catch (error) {
    return { error: String(error) };
  }
  const fields = out.trim().split("\n");
  if (fields.length !== FIELD_NAMES.length) {
    return { error: `the selection block printed ${fields.length} of ${FIELD_NAMES.length} oracle variables` };
  }
  const [service, upstream, adminService, adminUpstream] = fields;
  return { service, upstream, adminService, adminUpstream };
}

function resolvedFailure(label, actual, expected) {
  if (actual.error !== undefined) {
    return `${BRING_UP}: could not evaluate the ${label} oracle selection (${actual.error})`;
  }
  const matches = FIELD_NAMES.every((key) => actual[key] === expected[key]);
  if (matches) return null;
  return `${BRING_UP}: a ${label} bring-up resolves to web '${actual.service}' -> '${actual.upstream}' and admin '${actual.adminService}' -> '${actual.adminUpstream}', expected '${expected.service}' -> '${expected.upstream}' and '${expected.adminService}' -> '${expected.adminUpstream}' (a silent flip away from the memory-safe production oracle reintroduces the dev-server OOM, NEWFRONT-133)`;
}

// The selection block's text guard: the explicit dev opt-in must stay
// exactly as documented, and the production assignment must sit before
// the mode switch so the dev branch can only override it, never supply it.
function checkGuardText(block) {
  const failures = [];
  const guardAt = block.indexOf(DEV_OPT_IN);
  if (guardAt < 0) {
    const loose = block.match(/\[ "\$\{PARITY_ORACLE_MODE:-([a-z]+)\}" = "dev" \]/);
    failures.push(
      loose
        ? `${BRING_UP}: PARITY_ORACLE_MODE defaults to '${loose[1]}', not 'prod' (a bare bring-up must use the production oracle, NEWFRONT-133)`
        : `${BRING_UP}: missing the dev opt-in guard '${DEV_OPT_IN}' (a bare bring-up must use the production oracle)`
    );
    return failures;
  }
  if (block.slice(guardAt).includes(PROD_SELECTION)) {
    failures.push(
      `${BRING_UP}: '${PROD_SELECTION}' appears in the dev branch — the default is inverted (NEWFRONT-133)`
    );
  }
  if (!block.slice(0, guardAt).includes(PROD_SELECTION)) {
    failures.push(
      `${BRING_UP}: '${PROD_SELECTION}' must be assigned before the mode switch so a bare bring-up serves the production oracle`
    );
  }
  return failures;
}

function checkOracleDefault(bringUp, readme) {
  const failures = [];
  const block = selectionBlock(bringUp);
  if (block === null) {
    failures.push(
      `${BRING_UP}: oracle selection block not found (looked for '${SELECTION_START}' before '${SELECTION_END}')`
    );
  } else {
    failures.push(...checkGuardText(block));
    // Prove the outcome, not just the text: a bare bring-up (mode unset)
    // and an explicit `prod` must both serve the production oracle; `dev`
    // is the only mode that may serve the dev server, and it must move the
    // upstream with the service.
    for (const [label, mode] of [
      ["bare", null],
      ["PARITY_ORACLE_MODE=prod", "prod"],
      ["PARITY_ORACLE_MODE=dev", "dev"],
    ]) {
      const failure = resolvedFailure(label, resolveSelection(block, mode), EXPECTED[mode ?? "bare"]);
      if (failure !== null) failures.push(failure);
    }
  }
  // The README is the operator's only warning that dev mode is the
  // memory-heavy path; a silent flip without it hides the OOM risk.
  if (!readme.includes("PARITY_ORACLE_MODE=dev")) {
    failures.push(`${README}: must document 'PARITY_ORACLE_MODE=dev' as the opt-in for the dev-server oracle`);
  }
  if (!/production build/i.test(readme)) {
    failures.push(`${README}: must state that the production build is the default oracle`);
  }
  return failures;
}

function runOracleBuildCheck(readFile = (rel) => readFileSync(join(root, rel), "utf8")) {
  const failures = [];
  failures.push(...findBlanketCopies(readFile(DOCKERFILE), DOCKERFILE));
  failures.push(...findBlanketCopies(readFile(DOCKERFILE_PROD), DOCKERFILE_PROD));

  const required = [
    // The narrowed Dockerfile must still track the old app itself —
    // narrowing down to nothing would "fix" rebuilds with a stale oracle.
    [DOCKERFILE, "COPY apps/web apps/web", "the oracle must rebuild when the old app changes"],
    // The install must stay frozen: a resolving install drifts with the
    // registry, so every cache miss recomputes a different image and every
    // bring-up recreates the oracle container.
    [DOCKERFILE, "pnpm install --frozen-lockfile", "reproducible installs keep recomputes identical"],
    // The bring-up script keeps its escape hatch and its warm-up step.
    [BRING_UP, "--rebuild", "clean no-cache rebuilds need a flag"],
    [BRING_UP, "warming the oracle", "cold dev-server boots flake the first specs"],
  ];
  for (const [rel, needle, why] of required) {
    const miss = missingNeedle(readFile(rel), rel, needle, why);
    if (miss !== null) failures.push(miss);
  }
  failures.push(...checkOracleDefault(readFile(BRING_UP), readFile(README)));
  return failures;
}

function main() {
  const failures = runOracleBuildCheck();
  if (failures.length > 0) {
    console.error("Oracle build check failed:");
    for (const failure of failures) console.error(`  - ${failure}`);
    process.exit(1);
  }
  console.log("Oracle build check passed.");
}

// Run as a CLI when invoked directly (`node .../check-oracle-build.mjs`);
// stay a pure module when imported (e.g. by the test), so importing has
// no side effect.
if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}

export { runOracleBuildCheck };

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
const DEV_SELECTION = 'ORACLE_SERVICE="oracle"';
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

function checkOracleDefault(bringUp, readme) {
  const failures = [];
  const block = selectionBlock(bringUp);
  if (block === null) {
    failures.push(
      `${BRING_UP}: oracle selection block not found (looked for '${SELECTION_START}' before '${SELECTION_END}')`
    );
  } else {
    // A bare bring-up must select the production build. Check the base
    // assignments before the mode switch, not just anywhere in the
    // block: a prod assignment inside the dev branch would invert the
    // default.
    const guardAt = block.indexOf(DEV_OPT_IN);
    if (guardAt < 0) {
      const loose = block.match(/\[ "\$\{PARITY_ORACLE_MODE:-([a-z]+)\}" = "dev" \]/);
      failures.push(
        loose
          ? `${BRING_UP}: PARITY_ORACLE_MODE defaults to '${loose[1]}', not 'prod' (a bare bring-up must use the production oracle, NEWFRONT-133)`
          : `${BRING_UP}: missing the dev opt-in guard '${DEV_OPT_IN}' (a bare bring-up must use the production oracle)`
      );
    } else {
      const base = block.slice(0, guardAt);
      const devBranch = block.slice(guardAt);
      if (!base.includes(PROD_SELECTION)) {
        failures.push(
          `${BRING_UP}: '${PROD_SELECTION}' must be assigned before the mode switch so a bare bring-up serves the production oracle`
        );
      }
      if (!devBranch.includes(DEV_SELECTION)) {
        failures.push(
          `${BRING_UP}: '${DEV_SELECTION}' must stay in the dev branch so PARITY_ORACLE_MODE=dev still selects the dev server`
        );
      }
      if (devBranch.includes(PROD_SELECTION)) {
        failures.push(
          `${BRING_UP}: '${PROD_SELECTION}' appears in the dev branch — the default is inverted (NEWFRONT-133)`
        );
      }
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

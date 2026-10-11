// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle build-cost invariants (NEWFRONT-134). The oracle image used to
// start with a blanket repo-root COPY, so any edit anywhere invalidated
// its layers and every parity-up.sh recreated the oracle container,
// flaking the first specs on a cold dev server. Both oracle Dockerfiles
// must keep copying only the old app's inputs; parity-up.sh must keep the
// --rebuild escape hatch and the automatic route warm-up.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");

const DOCKERFILE = "apps/web_new/e2e/parity/stack/Dockerfile.oracle";
const DOCKERFILE_PROD = "apps/web_new/e2e/parity/stack/Dockerfile.oracle-prod";
const BRING_UP = "apps/web_new/e2e/parity/stack/parity-up.sh";

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

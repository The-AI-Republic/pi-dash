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
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");

const failures = [];

function mustNotContain(rel, banned, why) {
  const text = readFileSync(join(root, rel), "utf8");
  for (const line of text.split("\n")) {
    const stripped = line.trim();
    if (stripped.startsWith("#") || stripped === "") continue;
    if (stripped.startsWith(banned)) {
      failures.push(`${rel}: blanket '${banned}' copy is back (${why})`);
    }
  }
}

function mustContain(rel, needle, why) {
  const text = readFileSync(join(root, rel), "utf8");
  if (!text.includes(needle)) {
    failures.push(`${rel}: missing '${needle}' (${why})`);
  }
}

// A repo-root COPY in either oracle Dockerfile reintroduces the
// rebuild-on-every-edit cost. Build the banned prefix at runtime so this
// script does not literally contain the line it bans.
const blanket = ["COPY", ".", "."].join(" ");
mustNotContain("apps/web_new/e2e/parity/stack/Dockerfile.oracle", blanket, "copy only the old app's inputs");
mustNotContain("apps/web_new/e2e/parity/stack/Dockerfile.oracle-prod", blanket, "copy only the old app's inputs");

// The narrowed Dockerfile must still track the old app itself —
// narrowing down to nothing would "fix" rebuilds with a stale oracle.
mustContain(
  "apps/web_new/e2e/parity/stack/Dockerfile.oracle",
  "COPY apps/web apps/web",
  "the oracle must rebuild when the old app changes"
);

// The install must stay frozen: a resolving install drifts with the
// registry, so every cache miss recomputes a different image and every
// bring-up recreates the oracle container.
mustContain(
  "apps/web_new/e2e/parity/stack/Dockerfile.oracle",
  "pnpm install --frozen-lockfile",
  "reproducible installs keep recomputes identical"
);

// The bring-up script keeps its escape hatch and its warm-up step.
mustContain("apps/web_new/e2e/parity/stack/parity-up.sh", "--rebuild", "clean no-cache rebuilds need a flag");
mustContain(
  "apps/web_new/e2e/parity/stack/parity-up.sh",
  "warming the oracle",
  "cold dev-server boots flake the first specs"
);

if (failures.length > 0) {
  console.error("Oracle build check failed:");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("Oracle build check passed.");

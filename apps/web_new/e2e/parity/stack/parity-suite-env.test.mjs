// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Regression test for the parity-up.sh suite-env block (run: pnpm
// --filter web_new test:parity-stack). NEWFRONT-472: the block omitted
// PARITY_API_CONTAINER / PARITY_REDIS_CONTAINER, so every spec using the
// older docker-exec helpers (apiShell, desktopRuntimeShell, redis — all
// defaulting to fixed parity19-* names with no namespace derivation)
// failed on namespaced stacks with "No such container: parity19-api"
// unless the runner hand-exported two undocumented variables. The block
// must now carry both names, derived from the resolved container prefix,
// with explicit settings still winning. Never touches docker: the
// functional checks eval parity-env.sh plus the exact derivation lines
// extracted from parity-up.sh under bash.
import { strict as assert } from "node:assert";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const upScript = join(here, "parity-up.sh");
const envScript = join(here, "parity-env.sh");

const EXPECTED_DERIVATIONS = new Map([
  ["PARITY_API_CONTAINER", 'PARITY_API_CONTAINER="${PARITY_API_CONTAINER:-${PARITY_CONTAINER_PREFIX:-parity19}-api}"'],
  [
    "PARITY_REDIS_CONTAINER",
    'PARITY_REDIS_CONTAINER="${PARITY_REDIS_CONTAINER:-${PARITY_CONTAINER_PREFIX:-parity19}-redis}"',
  ],
]);

function derivationLines(text) {
  const lines = text.split("\n");
  return [...EXPECTED_DERIVATIONS.entries()].map(([name, expected]) => {
    const found = lines.filter((line) => line.startsWith(`${name}="`));
    assert.equal(found.length, 1, `parity-up.sh must derive ${name} exactly once`);
    assert.equal(found[0], expected, `${name} must derive from the container prefix with explicit-wins`);
    return found[0];
  });
}

// Resolves the two container names the way parity-up.sh does: the
// resolver first (same eval), then the script's own derivation lines,
// verbatim. `extra` seeds the ambient environment past a clean
// PARITY_* slate.
function resolvedContainers(extra) {
  const text = readFileSync(upScript, "utf8");
  const derivations = derivationLines(text);
  const env = { ...process.env };
  for (const key of Object.keys(env)) {
    if (key.startsWith("PARITY_")) delete env[key];
  }
  Object.assign(env, extra);
  // Quote the resolver path: it comes from this file's own location,
  // but a checkout path with spaces must still run.
  const quotedEnv = `'${envScript.replaceAll("'", "'\\''")}'`;
  const program = [
    `eval "$(${quotedEnv} 2>/dev/null)"`,
    ...derivations,
    `printf '%s\\n%s\\n' "$PARITY_API_CONTAINER" "$PARITY_REDIS_CONTAINER"`,
  ].join("\n");
  const out = execFileSync("bash", ["-c", program], { env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  return out.trim().split("\n");
}

describe("parity-up.sh suite-env block (NEWFRONT-472)", () => {
  it("prints both container exports in the suite-env block", () => {
    const text = readFileSync(upScript, "utf8");
    const marker = text.indexOf("suite env for this stack");
    assert.ok(marker >= 0, "parity-up.sh must print a suite-env block");
    const block = text.slice(marker);
    for (const name of EXPECTED_DERIVATIONS.keys()) {
      assert.ok(block.includes(`echo "export ${name}='$${name}'"`), `suite-env block must print ${name}`);
    }
  });

  it("derives both names before the block prints, from the container prefix", () => {
    const text = readFileSync(upScript, "utf8");
    const marker = text.indexOf("suite env for this stack");
    for (const [, expected] of EXPECTED_DERIVATIONS) {
      const at = text.indexOf(expected);
      assert.ok(at >= 0, "parity-up.sh must carry the derivation line");
      assert.ok(at < marker, "derivation must run before the suite-env block prints");
    }
  });

  it("resolves namespaced names from PARITY_NS", () => {
    assert.deepEqual(resolvedContainers({ PARITY_NS: "40" }), ["parity40-api", "parity40-redis"]);
  });

  it("lets explicit container settings win over the derivation", () => {
    assert.deepEqual(
      resolvedContainers({
        PARITY_NS: "40",
        PARITY_API_CONTAINER: "custom-api",
        PARITY_REDIS_CONTAINER: "custom-redis",
      }),
      ["custom-api", "custom-redis"]
    );
  });

  it("follows an explicit container prefix", () => {
    assert.deepEqual(resolvedContainers({ PARITY_CONTAINER_PREFIX: "other" }), ["other-api", "other-redis"]);
  });

  it("falls back to exactly the legacy parity19 names", () => {
    assert.deepEqual(resolvedContainers({}), ["parity19-api", "parity19-redis"]);
  });
});

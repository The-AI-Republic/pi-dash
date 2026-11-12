// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tests for the oracle build-cost check (NEWFRONT-134) and its oracle
// capacity default guard (NEWFRONT-133). The invariant tests use in-memory
// fixtures via the injected reader; the outcome test reads the real
// parity-up.sh and evaluates its selection block under bash, because a
// fixture there once passed while the shipped script booted the dev oracle.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { runOracleBuildCheck } from "./check-oracle-build.mjs";

const DOCKERFILE = "apps/web_new/e2e/parity/stack/Dockerfile.oracle";
const DOCKERFILE_PROD = "apps/web_new/e2e/parity/stack/Dockerfile.oracle-prod";
const BRING_UP = "apps/web_new/e2e/parity/stack/parity-up.sh";
const README = "apps/web_new/e2e/parity/stack/README.md";
const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

// Built at runtime, like the check does, so this file never literally
// contains the banned line.
const blanket = ["COPY", ".", "."].join(" ");

const goodDockerfile = [
  "# Narrowed oracle build: old-app inputs only.",
  "FROM node:22-slim",
  "",
  "COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./",
  "COPY apps/web/package.json apps/web/package.json",
  "RUN pnpm install --frozen-lockfile",
  "COPY apps/web apps/web",
  'CMD ["pnpm", "--filter", "web", "dev"]',
  "",
].join("\n");

const goodProdDockerfile = ["FROM node:22-slim", "COPY apps/web apps/web", ""].join("\n");

// Mirrors the shipped selection block: production first, the dev opt-in
// second, then the cloud edition's own oracle.
const PROD_ASSIGNMENTS = [
  "export ORACLE_SERVICE",
  'ORACLE_SERVICE="oracle-prod"',
  "export ORACLE_UPSTREAM",
  'ORACLE_UPSTREAM="oracle-prod:3000"',
  "export ORACLE_ADMIN_SERVICE",
  'ORACLE_ADMIN_SERVICE="oracle-admin-prod"',
  "export ORACLE_ADMIN_UPSTREAM",
  'ORACLE_ADMIN_UPSTREAM="oracle-admin-prod:3000"',
];
const DEV_BRANCH = [
  'if [ "${PARITY_ORACLE_MODE:-prod}" = "dev" ]; then',
  '  ORACLE_SERVICE="oracle"',
  '  ORACLE_UPSTREAM="oracle:3000"',
  '  ORACLE_ADMIN_SERVICE="oracle-admin"',
  '  ORACLE_ADMIN_UPSTREAM="oracle-admin:3000"',
  "fi",
];

const goodBringUp = [
  "#!/usr/bin/env bash",
  "# usage: parity-up.sh [--rebuild]",
  'if [ "${1:-}" = "--rebuild" ]; then',
  "  PARITY_REBUILD=1",
  "fi",
  ...PROD_ASSIGNMENTS,
  ...DEV_BRANCH,
  "# Preflight: ownership and published ports.",
  'echo "[parity] warming the oracle (entry + project routes)"',
  "",
].join("\n");

const goodReadme = [
  "## Oracle build: dev or production",
  "",
  "By default the oracle is the old app as a production build.",
  "",
  "```sh",
  "PARITY_ORACLE_MODE=dev apps/web_new/e2e/parity/stack/parity-up.sh",
  "```",
  "",
].join("\n");

function check(files) {
  return runOracleBuildCheck((rel) => {
    if (!(rel in files)) throw new Error(`unexpected read: ${rel}`);
    return files[rel];
  });
}

function goodTree() {
  return {
    [DOCKERFILE]: goodDockerfile,
    [DOCKERFILE_PROD]: goodProdDockerfile,
    [BRING_UP]: goodBringUp,
    [README]: goodReadme,
  };
}

describe("check-oracle-build", () => {
  it("passes a compliant tree", () => {
    expect(check(goodTree())).toEqual([]);
  });

  it("flags a blanket repo-root COPY in either oracle Dockerfile", () => {
    for (const rel of [DOCKERFILE, DOCKERFILE_PROD]) {
      const files = goodTree();
      files[rel] = `${files[rel]}\n${blanket}\n`;
      const failures = check(files);
      expect(failures).toHaveLength(1);
      expect(failures[0]).toContain(rel);
      expect(failures[0]).toContain("blanket");
    }
  });

  it("ignores commented-out copies and blank lines", () => {
    const files = goodTree();
    files[DOCKERFILE] = `# ${blanket}\n\n   \n${goodDockerfile}\n#${blanket}\n`;
    expect(check(files)).toEqual([]);
  });

  it("flags a Dockerfile narrowed past the old app itself", () => {
    const files = goodTree();
    files[DOCKERFILE] = goodDockerfile.replace("COPY apps/web apps/web\n", "");
    const failures = check(files);
    expect(failures).toHaveLength(1);
    expect(failures[0]).toContain("COPY apps/web apps/web");
  });

  it("flags a resolving (non-frozen) install", () => {
    const files = goodTree();
    files[DOCKERFILE] = goodDockerfile.replace("--frozen-lockfile", "--no-frozen-lockfile");
    const failures = check(files);
    expect(failures).toHaveLength(1);
    expect(failures[0]).toContain("pnpm install --frozen-lockfile");
  });

  it("flags a bring-up script without the rebuild escape hatch", () => {
    const files = goodTree();
    files[BRING_UP] = goodBringUp.replaceAll("--rebuild", "--from-scratch");
    const failures = check(files);
    expect(failures).toHaveLength(1);
    expect(failures[0]).toContain("--rebuild");
  });

  it("flags a bring-up script without the warm-up step", () => {
    const files = goodTree();
    files[BRING_UP] = goodBringUp.replace("warming the oracle", "starting the oracle");
    const failures = check(files);
    expect(failures).toHaveLength(1);
    expect(failures[0]).toContain("warming the oracle");
  });

  it("collects every failure in one run", () => {
    const failures = check({
      [DOCKERFILE]: `FROM node:22-slim\n${blanket}\nRUN pnpm install --no-frozen-lockfile\n`,
      [DOCKERFILE_PROD]: `FROM node:22-slim\n${blanket}\n`,
      [BRING_UP]: "#!/usr/bin/env bash\necho hi\n",
      [README]: "",
    });
    expect(failures).toHaveLength(9);
    expect(failures.filter((f) => f.includes(DOCKERFILE_PROD))).toHaveLength(1);
    expect(failures.filter((f) => f.includes(BRING_UP))).toHaveLength(3);
    expect(failures.filter((f) => f.includes(README))).toHaveLength(2);
  });

  describe("oracle capacity default (NEWFRONT-133)", () => {
    it("flags a default flipped back to the dev server", () => {
      const files = goodTree();
      files[BRING_UP] = goodBringUp.replace("${PARITY_ORACLE_MODE:-prod}", "${PARITY_ORACLE_MODE:-dev}");
      const failures = check(files);
      // The text guard names the flipped fallback; the bash evaluation
      // also reports that a bare bring-up now boots the dev server.
      expect(failures.some((f) => f.includes("defaults to 'dev'"))).toBe(true);
      expect(failures.some((f) => f.includes("bare bring-up resolves to web 'oracle'"))).toBe(true);
    });

    it("flags an inverted assignment that moves the prod oracle into the dev branch", () => {
      const files = goodTree();
      files[BRING_UP] = [
        "#!/usr/bin/env bash",
        "# usage: parity-up.sh [--rebuild]",
        'if [ "${1:-}" = "--rebuild" ]; then',
        "  PARITY_REBUILD=1",
        "fi",
        "export ORACLE_SERVICE",
        'ORACLE_SERVICE="oracle"',
        "export ORACLE_UPSTREAM",
        'ORACLE_UPSTREAM="oracle:3000"',
        "export ORACLE_ADMIN_SERVICE",
        'ORACLE_ADMIN_SERVICE="oracle-admin"',
        "export ORACLE_ADMIN_UPSTREAM",
        'ORACLE_ADMIN_UPSTREAM="oracle-admin:3000"',
        'if [ "${PARITY_ORACLE_MODE:-prod}" = "dev" ]; then',
        '  ORACLE_SERVICE="oracle-prod"',
        '  ORACLE_UPSTREAM="oracle-prod:3000"',
        '  ORACLE_ADMIN_SERVICE="oracle-admin-prod"',
        '  ORACLE_ADMIN_UPSTREAM="oracle-admin-prod:3000"',
        "fi",
        "# Preflight: ownership and published ports.",
        'echo "[parity] warming the oracle (entry + project routes)"',
        "",
      ].join("\n");
      const failures = check(files);
      expect(failures.some((f) => f.includes("inverted"))).toBe(true);
      expect(failures.some((f) => f.includes("must be assigned before the mode switch"))).toBe(true);
    });

    it("fails when the selection block is gone", () => {
      const files = goodTree();
      files[BRING_UP] = [
        "#!/usr/bin/env bash",
        "# usage: parity-up.sh [--rebuild]",
        'if [ "${1:-}" = "--rebuild" ]; then',
        "  PARITY_REBUILD=1",
        "fi",
        'echo "[parity] warming the oracle (entry + project routes)"',
        "",
      ].join("\n");
      const failures = check(files);
      expect(failures).toHaveLength(1);
      expect(failures[0]).toContain("oracle selection block not found");
    });

    it("fails when the markers appear out of order", () => {
      const files = goodTree();
      files[BRING_UP] = `# Preflight: mentioned before the selection\n${goodBringUp}`;
      const failures = check(files);
      expect(failures).toHaveLength(1);
      expect(failures[0]).toContain("oracle selection block not found");
    });

    it("fails when the dev opt-in disappears", () => {
      const files = goodTree();
      files[BRING_UP] = goodBringUp.replace(`${DEV_BRANCH.join("\n")}\n`, "");
      const failures = check(files);
      // Without the switch, PARITY_ORACLE_MODE=dev can no longer reach the
      // dev server either — both the missing guard and the outright
      // evaluation say so.
      expect(failures.some((f) => f.includes("missing the dev opt-in guard"))).toBe(true);
      expect(failures.some((f) => f.includes("PARITY_ORACLE_MODE=dev bring-up resolves to web 'oracle-prod'"))).toBe(
        true
      );
    });

    it("names the wrong fallback when the opt-in survives with a different default", () => {
      const files = goodTree();
      files[BRING_UP] = goodBringUp.replace(
        'if [ "${PARITY_ORACLE_MODE:-prod}" = "dev" ]; then',
        'if [ "${PARITY_ORACLE_MODE:-local}" = "dev" ]; then'
      );
      const failures = check(files);
      expect(failures).toHaveLength(1);
      expect(failures[0]).toContain("defaults to 'local', not 'prod'");
    });

    it("requires the README to name the dev opt-in and the production default", () => {
      const noOptIn = goodTree();
      noOptIn[README] = "## Oracle build\n\nIt uses a few MB.\n";
      const failures = check(noOptIn);
      expect(failures).toHaveLength(2);
      expect(failures.every((f) => f.includes(README))).toBe(true);
    });

    it("catches an override after the mode switch that silently boots the dev oracle", () => {
      const files = goodTree();
      files[BRING_UP] = goodBringUp.replace(
        "# Preflight: ownership and published ports.",
        'ORACLE_SERVICE="oracle"\nORACLE_UPSTREAM="oracle:3000"\n# Preflight: ownership and published ports.'
      );
      const failures = check(files);
      expect(failures).toHaveLength(2);
      expect(failures.every((f) => f.includes("resolves to web 'oracle'"))).toBe(true);
    });

    it("catches a commented-out production assignment", () => {
      const files = goodTree();
      files[BRING_UP] = goodBringUp.replace('ORACLE_SERVICE="oracle-prod"', '# ORACLE_SERVICE="oracle-prod"');
      const failures = check(files);
      expect(failures.some((f) => f.includes("web '<unset>'"))).toBe(true);
    });

    it("catches an upstream that still points at the dev server", () => {
      const files = goodTree();
      files[BRING_UP] = goodBringUp.replace('ORACLE_UPSTREAM="oracle-prod:3000"', 'ORACLE_UPSTREAM="oracle:3000"');
      const failures = check(files);
      expect(failures.some((f) => f.includes("-> 'oracle:3000'"))).toBe(true);
    });

    it("catches an admin oracle that follows a different mode than the web oracle", () => {
      const files = goodTree();
      files[BRING_UP] = goodBringUp.replace(
        'ORACLE_ADMIN_SERVICE="oracle-admin-prod"',
        'ORACLE_ADMIN_SERVICE="oracle-admin"'
      );
      const failures = check(files);
      expect(failures).toHaveLength(2);
      expect(failures.every((f) => f.includes("admin 'oracle-admin' -> 'oracle-admin-prod:3000'"))).toBe(true);
    });

    it("rejects a selection block bash cannot evaluate", () => {
      const files = goodTree();
      files[BRING_UP] = goodBringUp.replace(`fi\n# Preflight:`, `if true\n# Preflight:`);
      const failures = check(files);
      expect(failures.some((f) => f.includes("could not evaluate"))).toBe(true);
    });

    it("reports a selection block that does not print every oracle variable", () => {
      const files = goodTree();
      files[BRING_UP] = goodBringUp.replace("export ORACLE_ADMIN_UPSTREAM", "export ORACLE_ADMIN_UPSTREAM\nexit 0");
      const failures = check(files);
      expect(failures.some((f) => f.includes("printed 1 of 4 oracle variables"))).toBe(true);
    });

    it("proves the shipped parity-up.sh really selects the prod service by default", () => {
      // Read the real script, not a fixture: a fixture here once passed
      // while the shipped script (with an override after the `fi`) booted
      // the dev oracle. Evaluate its selection block the way a bring-up
      // does — under bash, mode unset / prod / dev — and assert the
      // resulting web and admin oracle wiring.
      const script = readFileSync(join(appRoot, "e2e/parity/stack/parity-up.sh"), "utf8");
      const block = script.slice(script.indexOf("export ORACLE_SERVICE"), script.indexOf("# Preflight:"));
      expect(block).toContain('ORACLE_SERVICE="oracle-prod"');

      const resolve = (mode) => {
        const env = { ...process.env };
        delete env.PARITY_ORACLE_MODE;
        // The block reads the edition the script resolves further up.
        env.PARITY_ORACLE_EDITION = "oss";
        if (mode !== null) env.PARITY_ORACLE_MODE = mode;
        const out = execFileSync(
          "bash",
          [
            "-c",
            [
              "set -euo pipefail",
              block,
              'printf "%s\\n" "$ORACLE_SERVICE" "$ORACLE_UPSTREAM" "$ORACLE_ADMIN_SERVICE" "$ORACLE_ADMIN_UPSTREAM"',
            ].join("\n"),
          ],
          { env, encoding: "utf8" }
        );
        const [service, upstream, adminService, adminUpstream] = out.trim().split("\n");
        return { service, upstream, adminService, adminUpstream };
      };

      const prod = {
        service: "oracle-prod",
        upstream: "oracle-prod:3000",
        adminService: "oracle-admin-prod",
        adminUpstream: "oracle-admin-prod:3000",
      };
      expect(resolve(null)).toEqual(prod);
      expect(resolve("prod")).toEqual(prod);
      expect(resolve("dev")).toEqual({
        service: "oracle",
        upstream: "oracle:3000",
        adminService: "oracle-admin",
        adminUpstream: "oracle-admin:3000",
      });
    });
  });
});

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tests for the oracle build-cost check (NEWFRONT-134). Hermetic: the file
// reader is injected from in-memory fixtures, so no test touches the real
// tree (the check itself runs against the real tree in CI).
import { describe, expect, it } from "vitest";
import { runOracleBuildCheck } from "./check-oracle-build.mjs";

const DOCKERFILE = "apps/web_new/e2e/parity/stack/Dockerfile.oracle";
const DOCKERFILE_PROD = "apps/web_new/e2e/parity/stack/Dockerfile.oracle-prod";
const BRING_UP = "apps/web_new/e2e/parity/stack/parity-up.sh";

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

const goodBringUp = [
  "#!/usr/bin/env bash",
  "# usage: parity-up.sh [--rebuild]",
  'if [ "${1:-}" = "--rebuild" ]; then',
  "  PARITY_REBUILD=1",
  "fi",
  'echo "[parity] warming the oracle (entry + project routes)"',
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
    });
    expect(failures).toHaveLength(6);
    expect(failures.filter((f) => f.includes(DOCKERFILE_PROD))).toHaveLength(1);
    expect(failures.filter((f) => f.includes(BRING_UP))).toHaveLength(2);
  });
});

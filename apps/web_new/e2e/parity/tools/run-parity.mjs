#!/usr/bin/env node
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
/**
 * Runs the parity suite: `run-parity.mjs <oracle|new|all> [playwright args]`.
 *
 * Everything after the project goes to Playwright unchanged, so a spec path
 * or `--grep @iss-007` narrows the run. Two things made that silently run
 * the whole suite before, and both are handled here:
 *  - a literal `--` (pnpm passes it through when you type
 *    `pnpm test:parity:oracle -- <filter>`): Playwright ignores every
 *    argument after it, so it is dropped;
 *  - `--project oracle <path>`: the space form reads the path as a second
 *    project name, so the project is always passed as `--project=<name>`.
 */
import { spawnSync } from "node:child_process";

const [project, ...rest] = process.argv.slice(2);
if (!["oracle", "new", "all"].includes(project)) {
  console.error("usage: run-parity.mjs <oracle|new|all> [playwright args]");
  process.exit(2);
}

const args = [
  "test",
  "--config",
  "e2e/parity/playwright.config.ts",
  ...(project === "all" ? [] : [`--project=${project}`]),
  ...rest.filter((arg) => arg !== "--"),
];
const result = spawnSync("playwright", args, { stdio: "inherit", shell: process.platform === "win32" });
process.exit(result.status ?? 1);

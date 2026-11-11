// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the size gate's budgets and entries (run: pnpm --filter
// web_new test:size-initial). It pins the one ceiling, proves the entries use
// it, and reads the real config, so a second, switchable budget cannot
// quietly grow back next to it. The retired spellings are built at runtime,
// the way check-headers.mjs builds the old copyright line, so this file never
// contains them itself.
import { strict as assert } from "node:assert";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

import * as budgets from "./size-budget.mjs";

const RETIRED_KB = ["2", "20"].join("");
const ENV_SWITCH = ["SIZE", "TARGET"].join("_");
const CHAIN = ["dist/web/assets/index-ENTRY.js", "dist/web/assets/route-FIRST.js"];

const configPath = join(dirname(fileURLToPath(import.meta.url)), "..", ".size-limit.mjs");

describe("size budgets", () => {
  it("blocks above a single 276 KB initial-JS ceiling", () => {
    assert.equal(budgets.INITIAL_JS_CEILING, "276 KB");
  });

  it("keeps the 30 KB initial-CSS ceiling", () => {
    assert.equal(budgets.INITIAL_CSS_CEILING, "30 KB");
  });

  it("exports exactly two budgets, so there is nowhere to add a second number", () => {
    const ceilings = Object.keys(budgets).filter((name) => name.endsWith("_CEILING"));
    assert.deepEqual(ceilings.sort(), ["INITIAL_CSS_CEILING", "INITIAL_JS_CEILING"]);
  });
});

describe("sizeLimits", () => {
  const entries = budgets.sizeLimits(CHAIN);

  it("measures the resolved initial chain against the JS ceiling, gzipped", () => {
    assert.deepEqual(entries[0], {
      name: "web initial JS (shell + first route)",
      path: CHAIN,
      limit: "276 KB",
      gzip: true,
    });
  });

  it("measures the emitted stylesheets against the CSS ceiling, gzipped", () => {
    assert.deepEqual(entries[1], {
      name: "web initial CSS",
      path: "dist/web/assets/*.css",
      limit: "30 KB",
      gzip: true,
    });
  });

  it("returns exactly the two entries and nothing switchable", () => {
    assert.equal(entries.length, 2);
    for (const entry of entries) assert.deepEqual(Object.keys(entry).sort(), ["gzip", "limit", "name", "path"]);
  });
});

describe(".size-limit.mjs", () => {
  const config = readFileSync(configPath, "utf8");

  it("delegates to the tested entries for the resolved chain", () => {
    assert.match(config, /import \{ sizeLimits \} from "\.\/checks\/size-budget\.mjs"/);
    assert.match(config, /export default sizeLimits\(initialJs\);/);
  });

  it("never reads the environment", () => {
    assert.doesNotMatch(config, /process\.env/);
  });

  it("does not carry the retired ceiling or its switch, in code or prose", () => {
    assert.ok(!config.includes(RETIRED_KB), `config still mentions ${RETIRED_KB}`);
    assert.ok(!config.includes(ENV_SWITCH), `config still mentions ${ENV_SWITCH}`);
  });
});

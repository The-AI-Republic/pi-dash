// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the coexistence generator (run: pnpm --filter web_new
// test:coexistence). Exercises block rendering, list validation, and the
// insert/replace/check cycle on throwaway Caddyfile copies.
import { strict as assert } from "node:assert";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";

import { buildAioLines, buildCeLines, loadPrefixes, syncFile } from "./coexistence-routes.mjs";

const CE_ANCHOR = "\treverse_proxy /* web:3000";
const AIO_ANCHOR = "    handle_path /* {";

const ceFixture = ["\treverse_proxy /api/* api:8000", "", CE_ANCHOR, ""].join("\n");
const aioFixture = ["    handle /api/* {", "        reverse_proxy localhost:3004", "    }", AIO_ANCHOR, ""].join("\n");

function scratch(files) {
  const dir = mkdtempSync(join(tmpdir(), "coexistence-"));
  const paths = {};
  for (const [name, text] of Object.entries(files)) {
    paths[name] = join(dir, name);
    writeFileSync(paths[name], text);
  }
  return paths;
}

describe("buildCeLines", () => {
  it("emits an empty note for an empty list", () => {
    assert.deepEqual(buildCeLines([]), ["\t# (none — the list is empty, so every path routes to the old app)"]);
  });

  it("routes a prefix to web_new with an exact-path redir", () => {
    assert.deepEqual(buildCeLines(["/sign-in"]), [
      "\tredir /sign-in /sign-in/ permanent",
      "\treverse_proxy /sign-in/* web_new:3000",
    ]);
  });

  it("shadows the old catch-all when everything has migrated", () => {
    assert.deepEqual(buildCeLines(["/"]), ["\treverse_proxy /* web_new:3000"]);
  });
});

describe("buildAioLines", () => {
  it("serves a prefix from the web_new static root, mirroring the sibling frontend blocks", () => {
    assert.deepEqual(buildAioLines(["/sign-in"]), [
      "    handle_path /sign-in* {",
      "        root * /app/web_new",
      "        try_files {path} {path}/ /index.html",
      "        file_server",
      "    }",
    ]);
  });

  it("emits an empty note for an empty list", () => {
    assert.equal(buildAioLines([]).length, 1);
  });
});

describe("loadPrefixes", () => {
  it("rejects missing slashes, trailing slashes, queries and duplicates", () => {
    const dir = mkdtempSync(join(tmpdir(), "coexistence-list-"));
    const write = (prefixes) => {
      const path = join(dir, `list-${Math.random().toString(36).slice(2)}.json`);
      writeFileSync(path, JSON.stringify({ prefixes }));
      return path;
    };
    assert.throws(() => loadPrefixes(write(["sign-in"])), /leading-slash/);
    assert.throws(() => loadPrefixes(write(["/a/"])), /trailing slash|end with a slash/);
    assert.throws(() => loadPrefixes(write(["/a?b"])), /must not contain/);
    assert.throws(() => loadPrefixes(write(["/a", "/a"])), /duplicate/);
    assert.deepEqual(loadPrefixes(write(["/sign-in", "/"])), ["/sign-in", "/"]);
  });
});

describe("syncFile", () => {
  it("inserts the block before the anchor, then reports unchanged", () => {
    const { ce } = scratch({ ce: ceFixture });
    assert.equal(syncFile(ce, CE_ANCHOR, "\t", buildCeLines([])), "updated");
    const text = readFileSync(ce, "utf8");
    assert.match(text, /BEGIN MIGRATED ROUTES/);
    assert.ok(text.indexOf("BEGIN MIGRATED ROUTES") < text.indexOf(CE_ANCHOR));
    assert.equal(syncFile(ce, CE_ANCHOR, "\t", buildCeLines([])), "unchanged");
  });

  it("replaces the block when the list changes and flags drift in check mode", () => {
    const { ce } = scratch({ ce: ceFixture });
    syncFile(ce, CE_ANCHOR, "\t", buildCeLines([]));
    assert.equal(syncFile(ce, CE_ANCHOR, "\t", buildCeLines(["/sign-in"])), "updated");
    assert.match(readFileSync(ce, "utf8"), /reverse_proxy \/sign-in\/\* web_new:3000/);
    assert.throws(() => syncFile(ce, CE_ANCHOR, "\t", buildCeLines([]), { check: true }), /stale/);
    syncFile(ce, CE_ANCHOR, "\t", buildCeLines(["/sign-in"]), { check: true });
  });

  it("fails check mode when the block was never generated", () => {
    const { aio } = scratch({ aio: aioFixture });
    assert.throws(() => syncFile(aio, AIO_ANCHOR, "    ", buildAioLines([]), { check: true }), /missing/);
  });
});

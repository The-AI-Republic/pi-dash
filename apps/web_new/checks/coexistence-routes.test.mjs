// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the coexistence generator (run: pnpm --filter web_new
// test:coexistence). Exercises entry validation, matcher rendering for
// literal and workspace-scoped entries, the reserved-path exclusion, and the
// insert/replace/check cycle on throwaway Caddyfile copies.
import { strict as assert } from "node:assert";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";

import {
  buildAioLines,
  buildCeLines,
  entryRegex,
  isWorkspaceScoped,
  loadEntries,
  parseEntry,
  reservedTopLevelOf,
  syncFile,
  validateList,
} from "./coexistence-routes.mjs";

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

function listFile(raw) {
  const dir = mkdtempSync(join(tmpdir(), "coexistence-list-"));
  const path = join(dir, `list-${Math.random().toString(36).slice(2)}.json`);
  writeFileSync(path, JSON.stringify(raw));
  return path;
}

describe("entryRegex", () => {
  it("matches a workspace home at exactly one segment, with an optional slash", () => {
    assert.equal(entryRegex(parseEntry("/{ws}")), "^/[^/.]+/?$");
  });

  it("matches a deeper workspace route at its exact depth", () => {
    assert.equal(entryRegex(parseEntry("/{ws}/projects")), "^/[^/.]+/projects/?$");
    assert.equal(entryRegex(parseEntry("/{ws}/browse/{workItem}")), "^/[^/.]+/browse/[^/.]+/?$");
  });

  it("continues to any depth after a trailing wildcard, including the entry itself", () => {
    assert.equal(entryRegex(parseEntry("/{ws}/projects/*")), "^/[^/.]+/projects(?:/.*)?$");
    assert.equal(entryRegex(parseEntry("/god-mode/*")), "^/god-mode(?:/.*)?$");
  });

  it("escapes regex metacharacters in literal segments", () => {
    assert.equal(entryRegex(parseEntry("/a.b+c/d")), "^/a\\.b\\+c/d/?$");
  });
});

describe("parseEntry and isWorkspaceScoped", () => {
  it("classifies workspace-scoped entries by their first segment", () => {
    assert.equal(isWorkspaceScoped(parseEntry("/{ws}")), true);
    assert.equal(isWorkspaceScoped(parseEntry("/{ws}/projects")), true);
    assert.equal(isWorkspaceScoped(parseEntry("/sign-in")), false);
    assert.equal(isWorkspaceScoped(parseEntry("/nowhere/{x}")), false);
    assert.equal(isWorkspaceScoped(parseEntry("/")), false);
  });

  it("rejects a wildcard that is not last and a malformed variable", () => {
    assert.throws(() => parseEntry("/{ws}/*/x"), /last segment/);
    assert.throws(() => parseEntry("/{ws}/{bad name}"), /bad segment/);
    assert.throws(() => parseEntry("/{ws/x}"), /bad segment/);
    assert.throws(() => parseEntry("/a//b"), /bad segment/);
  });
});

describe("reservedTopLevelOf", () => {
  it("unions the reserved list with every literal entry's first segment", () => {
    const names = reservedTopLevelOf([parseEntry("/sign-in"), parseEntry("/god-mode/users")], ["api", "Spaces"]);
    assert.deepEqual(names, ["api", "god-mode", "sign-in", "spaces"]);
  });

  it("ignores entries that start with a variable", () => {
    assert.deepEqual(reservedTopLevelOf([parseEntry("/{ws}")], ["api"]), ["api"]);
  });
});

describe("buildCeLines", () => {
  it("emits an empty note for an empty list", () => {
    assert.deepEqual(buildCeLines([]), ["\t# (none — the list is empty, so every path routes to the old app)"]);
  });

  it("routes a literal entry to web_new with an exact-path redir, unchanged", () => {
    assert.deepEqual(buildCeLines(["/sign-in"]), [
      "\tredir /sign-in /sign-in/ permanent",
      "\treverse_proxy /sign-in/* web_new:3000",
    ]);
  });

  it("shadows the old catch-all when everything has migrated", () => {
    assert.deepEqual(buildCeLines(["/"]), ["\treverse_proxy /* web_new:3000"]);
  });

  it("renders a named matcher for a workspace entry, excluding reserved top-level names", () => {
    assert.deepEqual(buildCeLines(["/sign-in", "/{ws}/projects"], ["api", "settings"]), [
      "\tredir /sign-in /sign-in/ permanent",
      "\treverse_proxy /sign-in/* web_new:3000",
      "\t@web_new_1 {",
      "\t    path_regexp ^/[^/.]+/projects/?$",
      "\t    not path /api /api/* /settings /settings/* /sign-in /sign-in/*",
      "\t}",
      "\treverse_proxy @web_new_1 web_new:3000",
    ]);
  });
});

describe("buildAioLines", () => {
  it("serves a literal entry from the web_new static root without stripping its prefix", () => {
    assert.deepEqual(buildAioLines(["/sign-in"]), [
      "    handle /sign-in* {",
      "        root * /app/web_new",
      "        try_files {path} {path}/ /index.html",
      "        file_server",
      "    }",
    ]);
  });

  it("uses handle, not handle_path, for a workspace entry so the path is not stripped", () => {
    assert.deepEqual(buildAioLines(["/{ws}"], ["god-mode"]), [
      "    @web_new_0 {",
      "        path_regexp ^/[^/.]+/?$",
      "        not path /god-mode /god-mode/*",
      "    }",
      "    handle @web_new_0 {",
      "        root * /app/web_new",
      "        try_files {path} {path}/ /index.html",
      "        file_server",
      "    }",
    ]);
  });

  it("serves everything from web_new for the catch-all", () => {
    const lines = buildAioLines(["/"]);
    assert.equal(lines[0], "    handle * {");
    assert.ok(lines.includes("        root * /app/web_new"));
  });

  it("emits an empty note for an empty list", () => {
    assert.equal(buildAioLines([]).length, 1);
  });
});

describe("validateList and loadEntries", () => {
  it("rejects missing slashes, trailing slashes, queries and duplicates", () => {
    assert.throws(() => validateList({ entries: ["sign-in"] }), /leading-slash/);
    assert.throws(() => validateList({ entries: ["/a/"] }), /end with, or repeat/);
    assert.throws(() => validateList({ entries: ["/a//b"] }), /repeat, a slash/);
    assert.throws(() => validateList({ entries: ["/a?b"] }), /must not contain/);
    assert.throws(() => validateList({ entries: ["/a", "/a"] }), /duplicate entry/);
    assert.throws(() => validateList({ entries: ["/{ws"] }), /unbalanced/);
    assert.deepEqual(validateList({ entries: ["/sign-in", "/a"] }), ["/sign-in", "/a"]);
  });

  it("only accepts a lone catch-all entry", () => {
    assert.throws(() => validateList({ entries: ["/sign-in", "/"] }), /catch-all/);
    assert.deepEqual(validateList({ entries: ["/"] }), ["/"]);
  });

  it("validates the reserved list as lowercase slashless slugs", () => {
    assert.throws(() => validateList({ entries: [], reservedTopLevel: "/api" }), /must be an array/);
    assert.throws(() => validateList({ entries: [], reservedTopLevel: ["/api"] }), /lowercase slugs/);
    assert.throws(() => validateList({ entries: [], reservedTopLevel: ["API"] }), /lowercase slugs/);
    assert.throws(() => validateList({ entries: [], reservedTopLevel: ["api", "api"] }), /duplicate reserved/);
    assert.deepEqual(validateList({ entries: ["/{ws}"], reservedTopLevel: ["api"] }), ["/{ws}"]);
  });

  it("reads the bundled list with its entries and reserved names", () => {
    const { entries, reservedTopLevel } = loadEntries();
    assert.ok(entries.includes("/{ws}"));
    assert.ok(reservedTopLevel.includes("api"));
  });

  it("loads a list from an explicit path", () => {
    const path = listFile({ entries: ["/sign-in", "/{ws}"], reservedTopLevel: ["api"] });
    assert.deepEqual(loadEntries(path), { entries: ["/sign-in", "/{ws}"], reservedTopLevel: ["api"] });
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

  it("replaces a stale literal block with the workspace matcher", () => {
    const { aio } = scratch({ aio: aioFixture });
    syncFile(aio, AIO_ANCHOR, "    ", buildAioLines(["/sign-in"]));
    assert.equal(syncFile(aio, AIO_ANCHOR, "    ", buildAioLines(["/{ws}"], ["api"])), "updated");
    const text = readFileSync(aio, "utf8");
    assert.match(text, /handle @web_new_0 \{/);
    assert.doesNotMatch(text, /handle_path \/\{ws\}/);
    assert.throws(() => syncFile(aio, AIO_ANCHOR, "    ", buildAioLines(["/sign-in"]), { check: true }), /stale/);
  });

  it("fails check mode when the block was never generated", () => {
    const { aio } = scratch({ aio: aioFixture });
    assert.throws(() => syncFile(aio, AIO_ANCHOR, "    ", buildAioLines([]), { check: true }), /missing/);
  });

  it("fails when the anchor is missing", () => {
    const { ce } = scratch({ ce: "# nothing here\n" });
    assert.throws(() => syncFile(ce, CE_ANCHOR, "\t", buildCeLines([])), /anchor line not found/);
  });

  it("refuses a half-present marker block", () => {
    const { ce } = scratch({ ce: ["# END MIGRATED ROUTES", CE_ANCHOR, ""].join("\n") });
    assert.throws(() => syncFile(ce, CE_ANCHOR, "\t", buildCeLines([])), /half-present/);
  });
});

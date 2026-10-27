// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the coexistence generator (run: pnpm --filter web_new
// test:coexistence). Exercises entry validation, matcher rendering for
// literal and workspace-scoped entries, the reserved-path exclusion, the
// legacy-shadow guard (NEWFRONT-953), and the insert/replace/check cycle on
// throwaway Caddyfile copies, and the CLI entry point.
import { strict as assert } from "node:assert";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

import {
  buildAioLines,
  buildCeLines,
  entryRegex,
  findShadowingRoutes,
  isWorkspaceScoped,
  loadEntries,
  main,
  matcherCoversEntry,
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

describe("matcherCoversEntry", () => {
  it("covers the entry in every exact and subtree spelling", () => {
    for (const matcher of ["/god-mode", "/god-mode/", "/god-mode/*", "/god-mode*"]) {
      assert.equal(matcherCoversEntry(matcher, "/god-mode"), true, matcher);
    }
  });

  it("covers the entry below an ancestor subtree spelling, but not a bare ancestor", () => {
    assert.equal(matcherCoversEntry("/auth/*", "/auth/device"), true);
    assert.equal(matcherCoversEntry("/auth*", "/auth/device"), true);
    assert.equal(matcherCoversEntry("/auth", "/auth/device"), false);
    assert.equal(matcherCoversEntry("/auth/", "/auth/device"), false);
  });

  it("stops at segment boundaries and ignores unrelated prefixes", () => {
    assert.equal(matcherCoversEntry("/auth*", "/authn/device"), false);
    assert.equal(matcherCoversEntry("/god-modefoo*", "/god-mode"), false);
    assert.equal(matcherCoversEntry("/other/*", "/auth/device"), false);
    assert.equal(matcherCoversEntry("/auth/device/other", "/auth/device"), false);
    assert.equal(matcherCoversEntry("/{$BUCKET_NAME}/*", "/god-mode"), false);
    assert.equal(matcherCoversEntry("/{$BUCKET_NAME}", "/god-mode"), false);
  });

  it("treats a catch-all as covering everything and a non-path as covering nothing", () => {
    assert.equal(matcherCoversEntry("/*", "/god-mode"), true);
    assert.equal(matcherCoversEntry("*", "/auth/device"), true);
    assert.equal(matcherCoversEntry("@legacy", "/god-mode"), false);
    assert.equal(matcherCoversEntry("localhost:3004", "/god-mode"), false);
    assert.equal(matcherCoversEntry("web_new:3000", "/god-mode"), false);
  });

  it("compares case-sensitively, like Caddy path matching", () => {
    assert.equal(matcherCoversEntry("/GOD-MODE/*", "/god-mode"), false);
  });
});

describe("findShadowingRoutes", () => {
  function markedCe(headLines) {
    const { ce } = scratch({ ce: ceFixture });
    syncFile(ce, CE_ANCHOR, "\t", buildCeLines(["/sign-in"]));
    const lines = readFileSync(ce, "utf8").split("\n");
    const at = lines.findIndex((line) => line.includes("BEGIN MIGRATED ROUTES"));
    lines.splice(at, 0, ...headLines);
    return lines.join("\n");
  }

  function markedAio(headLines) {
    const { aio } = scratch({ aio: aioFixture });
    syncFile(aio, AIO_ANCHOR, "    ", buildAioLines(["/sign-in"]));
    const lines = readFileSync(aio, "utf8").split("\n");
    const at = lines.findIndex((line) => line.includes("BEGIN MIGRATED ROUTES"));
    lines.splice(at, 0, ...headLines);
    return lines.join("\n");
  }

  it("flags the CE hand-written redir plus proxy pair", () => {
    const text = markedCe(["\tredir /god-mode /god-mode/ permanent", "\treverse_proxy /god-mode/* admin:3000"]);
    assert.deepEqual(findShadowingRoutes(text, ["/god-mode"]), [
      { line: 3, directive: "redir", matcher: "/god-mode", entries: ["/god-mode"] },
      { line: 4, directive: "reverse_proxy", matcher: "/god-mode/*", entries: ["/god-mode"] },
    ]);
  });

  it("flags the AIO handle_path block while ignoring its inner upstream line", () => {
    const text = markedAio([
      "    handle_path /god-mode* {",
      "        root * /app/admin",
      "        try_files {path} {path}/ /index.html",
      "        file_server",
      "    }",
    ]);
    assert.deepEqual(findShadowingRoutes(text, ["/god-mode"]), [
      { line: 4, directive: "handle_path", matcher: "/god-mode*", entries: ["/god-mode"] },
    ]);
  });

  it("flags an ancestor prefix covering a deeper entry in both flavors", () => {
    assert.deepEqual(findShadowingRoutes(markedCe(["\treverse_proxy /auth/* api:8000"]), ["/auth/device"]), [
      { line: 3, directive: "reverse_proxy", matcher: "/auth/*", entries: ["/auth/device"] },
    ]);
    assert.deepEqual(findShadowingRoutes(markedAio(["    handle /auth/* {"]), ["/auth/device"]), [
      { line: 4, directive: "handle", matcher: "/auth/*", entries: ["/auth/device"] },
    ]);
  });

  it("ignores bare ancestors, unmigrated prefixes, matchers without paths, and comments", () => {
    const text = markedCe([
      "\treverse_proxy /auth api:8000",
      "\treverse_proxy /spaces/* space:3000",
      "\treverse_proxy @legacy api:8000",
      "\treverse_proxy /{$BUCKET_NAME}/* pi-dash-minio:9000",
      "\t# reverse_proxy /god-mode/* admin:3000",
    ]);
    assert.deepEqual(findShadowingRoutes(text, ["/auth/device", "/god-mode", "/sign-in"]), []);
  });

  it("allows a legacy fallback below the generated block", () => {
    const lines = markedCe([]).split("\n");
    const end = lines.findIndex((line) => line.includes("END MIGRATED ROUTES"));
    lines.splice(end + 1, 0, "\treverse_proxy /sign-in/* old:3000");
    assert.deepEqual(findShadowingRoutes(lines.join("\n"), ["/sign-in"]), []);
  });

  it("reports nothing without markers, and skips workspace-scoped and catch-all entries", () => {
    assert.deepEqual(findShadowingRoutes(ceFixture, ["/api"]), []);
    assert.deepEqual(findShadowingRoutes(markedCe(["\treverse_proxy /ws/* api:8000"]), ["/{ws}/runners/*"]), []);
    assert.deepEqual(findShadowingRoutes(markedCe(["\treverse_proxy /acme/* old:3000"]), ["/"]), []);
  });

  it("flags a pre-block catch-all against every literal entry", () => {
    const text = markedCe(["\treverse_proxy /* old:3000"]);
    assert.deepEqual(findShadowingRoutes(text, ["/sign-in", "/god-mode"]), [
      { line: 3, directive: "reverse_proxy", matcher: "/*", entries: ["/sign-in", "/god-mode"] },
    ]);
  });

  it("finds no shadows in the repo's own Caddyfiles", () => {
    const proxyDir = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "proxy");
    const { entries } = loadEntries();
    for (const name of ["Caddyfile.ce", "Caddyfile.aio.ce"]) {
      assert.deepEqual(findShadowingRoutes(readFileSync(join(proxyDir, name), "utf8"), entries), [], name);
    }
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

describe("main", () => {
  function runMain(argv) {
    const lines = [];
    const stdout = console.log;
    console.log = (text) => lines.push(text);
    try {
      main(argv);
    } finally {
      console.log = stdout;
    }
    return lines.join("\n");
  }

  function project() {
    const list = listFile({ entries: ["/sign-in", "/{ws}"], reservedTopLevel: ["api"] });
    const { ce, aio } = scratch({ ce: ceFixture, aio: aioFixture });
    return { argv: ["--list", list, "--caddy-ce", ce, "--caddy-aio", aio], ce, aio };
  }

  it("generates both files and reports the per-file outcome", () => {
    const { argv, ce, aio } = project();
    assert.match(runMain(argv), /Coexistence routes: CE updated, AIO updated \(2 migrated route\(s\)\)\./);
    assert.match(readFileSync(ce, "utf8"), /@web_new_\d/);
    assert.match(readFileSync(aio, "utf8"), /handle @web_new_\d+ \{/);
    assert.match(runMain(argv), /Coexistence routes: CE unchanged, AIO unchanged/);
  });

  it("passes check mode on fresh output and throws on drift", () => {
    const { argv, ce } = project();
    runMain(argv);
    assert.match(runMain([...argv, "--check"]), /Coexistence check passed \(2 migrated route\(s\)\)\./);
    writeFileSync(ce, readFileSync(ce, "utf8").replace("web_new:3000", "web_new:3001"));
    assert.throws(() => runMain([...argv, "--check"]), /stale/);
  });

  it("rejects an unknown argument", () => {
    assert.throws(() => runMain(["--bogus"]), /unknown argument/);
  });

  it("refuses to generate or check a project with a shadowed entry", () => {
    const { argv, ce, aio } = project();
    runMain(argv);
    const shadow = (path, line) => {
      const lines = readFileSync(path, "utf8").split("\n");
      const at = lines.findIndex((candidate) => candidate.includes("BEGIN MIGRATED ROUTES"));
      lines.splice(at, 0, line);
      writeFileSync(path, lines.join("\n"));
    };
    shadow(ce, "\treverse_proxy /sign-in/* old:3000");
    assert.throws(() => runMain([...argv, "--check"]), /shadows migrated route \/sign-in/);
    assert.throws(() => runMain(argv), /shadows migrated route \/sign-in/);
    writeFileSync(ce, readFileSync(ce, "utf8").replace("\treverse_proxy /sign-in/* old:3000\n", ""));
    shadow(aio, "    handle /sign-in* {");
    assert.throws(
      () => runMain([...argv, "--check"]),
      (error) => {
        assert.match(error.message, /hand-written `handle \/sign-in\*`/);
        assert.ok(error.message.includes(aio));
        return true;
      },
      "expected the error to name the shadowed AIO file"
    );
  });
});

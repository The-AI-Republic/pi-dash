// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the initial-JS chain resolver (run: pnpm --filter web_new
// test:size-initial). The fixture mirrors the real web manifest shape: an
// entry, two first-route splits sharing chunks, and lazy splits that must
// stay excluded.
import { strict as assert } from "node:assert";
import { describe, it } from "node:test";

import { ENTRY_KEY, FIRST_ROUTE_KEYS, WEB_OUT_DIR, initialChainFiles } from "./size-initial.mjs";

const [LAYOUT_KEY, LEAF_KEY] = FIRST_ROUTE_KEYS;

function fixture() {
  return {
    [ENTRY_KEY]: {
      file: "assets/index-ENTRY.js",
      dynamicImports: [LAYOUT_KEY, LEAF_KEY, "src/routes/sign-in.tsx?tsr-split=component"],
    },
    [LAYOUT_KEY]: {
      file: "assets/route-LAYOUT.js",
      imports: [ENTRY_KEY, "_toaster-HASH.js", "_api-HASH.js"],
    },
    [LEAF_KEY]: {
      file: "assets/index-LEAF.js",
      imports: [ENTRY_KEY, "_toaster-HASH.js"],
    },
    "_toaster-HASH.js": { file: "assets/toaster-HASH.js", imports: [ENTRY_KEY] },
    "_api-HASH.js": { file: "assets/api-HASH.js", imports: [ENTRY_KEY] },
    "src/routes/sign-in.tsx?tsr-split=component": {
      file: "assets/sign-in-HASH.js",
      imports: [ENTRY_KEY, "_api-HASH.js"],
    },
    "src/routes/$ws/route.tsx?tsr-split=notFoundComponent": {
      file: "assets/route-NOTFOUND.js",
      imports: [ENTRY_KEY],
    },
  };
}

const CHAIN = [
  "dist/web/assets/api-HASH.js",
  "dist/web/assets/index-ENTRY.js",
  "dist/web/assets/index-LEAF.js",
  "dist/web/assets/route-LAYOUT.js",
  "dist/web/assets/toaster-HASH.js",
];

describe("initialChainFiles", () => {
  it("returns the entry plus the first-route static closure, sorted and deduped", () => {
    assert.deepEqual(initialChainFiles(fixture()), CHAIN);
  });

  it("excludes lazy splits even when they share chunks with the chain", () => {
    const files = initialChainFiles(fixture());
    assert.ok(!files.some((file) => file.includes("sign-in-HASH")));
    assert.ok(!files.some((file) => file.includes("route-NOTFOUND")));
    // ... while keeping the shared chunk the lazy split also imports.
    assert.ok(files.includes("dist/web/assets/api-HASH.js"));
  });

  it("ignores dynamicImports of the entry (routes load on navigation, not first paint)", () => {
    const manifest = fixture();
    manifest[ENTRY_KEY] = { file: "assets/index-ENTRY.js", dynamicImports: ["src/routes/sign-in.tsx"] };
    delete manifest["src/routes/sign-in.tsx?tsr-split=component"];
    assert.deepEqual(initialChainFiles(manifest), CHAIN);
  });

  it("joins files under the configured out dir", () => {
    const files = initialChainFiles(fixture(), { outDir: "dist/desktop" });
    assert.ok(files.every((file) => file.startsWith("dist/desktop/")));
    assert.equal(files.length, CHAIN.length);
  });

  it("throws when a first-route key is missing, instead of shrinking the set silently", () => {
    const manifest = fixture();
    delete manifest[LEAF_KEY];
    assert.throws(() => initialChainFiles(manifest), /first-route manifest key/);
  });

  it("throws when the entry key is missing", () => {
    const manifest = fixture();
    delete manifest[ENTRY_KEY];
    assert.throws(() => initialChainFiles(manifest), /entry manifest key/);
  });

  it("throws when a static import dangles", () => {
    const manifest = fixture();
    delete manifest["_api-HASH.js"];
    assert.throws(() => initialChainFiles(manifest), /imported but not emitted/);
  });

  it("throws on a manifest that is not an object", () => {
    assert.throws(() => initialChainFiles(null), /not an object/);
    assert.throws(() => initialChainFiles([]), /not an object/);
  });

  it("pins the web out dir the size-limit config globs", () => {
    assert.equal(WEB_OUT_DIR, "dist/web");
  });
});

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for test-impact.mjs on a small in-memory tree: how imports and
// exports are read and resolved, and which test files a change reaches by
// imported name.
import { describe, expect, it } from "vitest";

import { affectedTests, bindings, createGraph, parseExports, parseImports, resolveSpecifier } from "./test-impact.mjs";

const tree = {
  "packages/kit/src/index.ts": [
    'export { Button } from "./Button";',
    'export { Dialog, type DialogProps } from "./Dialog";',
    'export { Table as DataTable } from "./Table";',
    'export * from "./tokens";',
    'export * as icons from "./icons";',
    'export { Missing } from "./Missing";',
    'export * from "./gone";',
    'export * from "./tokens";',
  ].join("\n"),
  "packages/kit/src/Button.tsx": 'import { cx } from "./cx";\nexport function Button() {}',
  "packages/kit/src/Dialog.tsx":
    'import { Button } from "./Button";\nexport const Dialog = 1;\nexport interface DialogProps {}',
  "packages/kit/src/Table.tsx": 'import { VirtualList } from "./VirtualList.js";\nexport class Table {}',
  "packages/kit/src/VirtualList.tsx": "export const VirtualList = 1;",
  "packages/kit/src/cx.ts": "export const cx = 1;",
  "packages/kit/src/tokens/index.ts": 'export * from "./colors";\nconst size = 2;\nexport { size as SIZE };',
  "packages/kit/src/tokens/colors.ts": "export const RED = 1;",
  "packages/kit/src/icons.ts": "export const Plus = 1;",
  "packages/api-client/src/index.ts":
    'export { listProjects } from "./projects.js";\nexport { listCycles } from "./cycles.js";',
  "packages/api-client/src/projects.ts": "export async function listProjects() {}",
  "packages/api-client/src/cycles.ts":
    'import { listProjects } from "./projects.js";\nexport const listCycles = listProjects;',
  "apps/web_new/src/features/auth/index.ts":
    'export { useSession } from "./session";\nexport { SignIn } from "./SignIn";',
  "apps/web_new/src/features/auth/session.ts":
    'import { listProjects } from "@pidash/api-client";\nexport const useSession = listProjects;',
  "apps/web_new/src/features/auth/SignIn.tsx":
    'import { Button } from "@pidash/kit";\nexport default function SignIn() {}\nexport { SignIn };',
  "apps/web_new/src/features/auth/session.test.ts": 'import { useSession } from "@web_new/features/auth";',
  "apps/web_new/src/features/auth/SignIn.test.tsx": 'import { SignIn } from "../auth";',
  "apps/web_new/src/features/grid/Grid.tsx":
    'import { DataTable as T, type DialogProps } from "@pidash/kit";\nimport "./grid.css";',
  "apps/web_new/src/features/grid/grid.css": ".grid {}",
  "apps/web_new/src/features/grid/Grid.test.tsx": 'import Grid from "./Grid";',
  "apps/web_new/src/features/all/All.test.tsx": 'import * as kit from "@pidash/kit";',
  "apps/web_new/src/features/all/Lazy.test.tsx": 'const m = await import("../cycles/cycles");',
  "apps/web_new/src/features/all/Types.test.ts":
    'import type { Button } from "@pidash/kit";\nlet t: typeof import("@pidash/kit");',
  "apps/web_new/src/features/cycles/cycles.ts":
    'import { listCycles } from "@pidash/api-client";\nexport const cycles = listCycles;',
  "apps/web_new/src/features/cycles/cycles.test.ts": 'import { cycles } from "./cycles";',
  "apps/web_new/src/core/edition/oss.ts": "export const edition = 1;",
  "apps/web_new/src/core/edition/edition.test.ts": 'import { edition } from "@pidash/edition";',
  "apps/web_new/e2e/parity/drivers/x.test.ts": 'import { RED } from "@pidash/kit";',
  "apps/web_new/src/setup.ts": 'import "react";',
};
const files = Object.keys(tree);
const fileSet = new Set(files);
const read = (file) => tree[file] ?? null;

describe("parseExports", () => {
  it("reads declarations, defaults, local lists, re-exports and stars", () => {
    const parsed = parseExports(tree["packages/kit/src/index.ts"]);
    expect(parsed.reexports[1]).toEqual({
      from: "./Dialog",
      specifiers: [
        { name: "Dialog", alias: "Dialog", type: false },
        { name: "DialogProps", alias: "DialogProps", type: true },
      ],
    });
    expect(parsed.reexports.find((entry) => entry.from === "./icons").specifiers).toEqual([
      { name: "*", alias: "icons", type: false },
    ]);
    expect(parsed.stars).toEqual(["./tokens", "./gone", "./tokens"]);
    expect([...parseExports(tree["apps/web_new/src/features/auth/SignIn.tsx"]).local]).toEqual(["default", "SignIn"]);
    expect([...parseExports("export { a, };\nexport async function b() {}").local]).toEqual(["b", "a"]);
  });
});

describe("parseImports", () => {
  it("reads value imports and skips type-only ones and re-exports", () => {
    expect(parseImports(tree["apps/web_new/src/features/grid/Grid.tsx"])).toEqual([
      { from: "@pidash/kit", names: new Set(["DataTable"]) },
      { from: "./grid.css", names: null },
    ]);
    expect(parseImports(tree["apps/web_new/src/features/all/Types.test.ts"])).toEqual([]);
    expect(parseImports('export { a } from "./a";\nimport { type T } from "./t";')).toEqual([]);
  });

  it("treats default as a name and namespace, side-effect and dynamic imports as everything", () => {
    expect(parseImports('import Grid, { a } from "./Grid";')).toEqual([
      { from: "./Grid", names: new Set(["a", "default"]) },
    ]);
    expect(parseImports('import * as kit from "@pidash/kit";\nawait import("./x");')).toEqual([
      { from: "@pidash/kit", names: null },
      { from: "./x", names: null },
    ]);
  });
});

describe("resolveSpecifier", () => {
  it("resolves aliases, @web_new paths, relative paths, .js suffixes and index files", () => {
    expect(resolveSpecifier("x", "@pidash/kit", fileSet)).toBe("packages/kit/src/index.ts");
    expect(resolveSpecifier("x", "@pidash/edition", fileSet)).toBe("apps/web_new/src/core/edition/oss.ts");
    expect(resolveSpecifier("x", "@web_new/features/auth", fileSet)).toBe("apps/web_new/src/features/auth/index.ts");
    expect(resolveSpecifier("packages/kit/src/Table.tsx", "./VirtualList.js", fileSet)).toBe(
      "packages/kit/src/VirtualList.tsx"
    );
    expect(resolveSpecifier("apps/web_new/src/features/all/Lazy.test.tsx", "../cycles/cycles", fileSet)).toBe(
      "apps/web_new/src/features/cycles/cycles.ts"
    );
    expect(resolveSpecifier("apps/web_new/src/features/grid/Grid.tsx", "./grid.css", fileSet)).toBe(
      "apps/web_new/src/features/grid/grid.css"
    );
  });

  it("returns null for npm packages and missing files", () => {
    expect(resolveSpecifier("x", "react", fileSet)).toBeNull();
    expect(resolveSpecifier("packages/kit/src/index.ts", "./nowhere", fileSet)).toBeNull();
  });
});

describe("createGraph", () => {
  it("maps each public name to its declaring file through aliases, nested entries and stars", () => {
    const { exportMap } = createGraph(files, read);
    const kit = exportMap("packages/kit/src/index.ts");
    expect([...kit.get("Button")]).toEqual(["packages/kit/src/Button.tsx"]);
    expect([...kit.get("DataTable")]).toEqual(["packages/kit/src/Table.tsx"]);
    expect([...kit.get("RED")]).toEqual(["packages/kit/src/tokens/colors.ts"]);
    expect([...kit.get("SIZE")]).toEqual(["packages/kit/src/tokens/index.ts"]);
    expect([...kit.get("icons")]).toEqual(["packages/kit/src/icons.ts"]);
    expect(kit.has("Missing")).toBe(false);
  });

  it("survives files it cannot read and re-export cycles", () => {
    const loop = {
      "a.ts": 'export * from "./b";\nexport { x } from "./b";',
      "b.ts": 'export * from "./a";\nexport const x = 1;',
    };
    const { exportMap } = createGraph([...Object.keys(loop), "c.ts"], (file) => loop[file] ?? null);
    expect([...exportMap("a.ts").get("x")]).toEqual(["b.ts"]);
    expect(exportMap("c.ts").size).toBe(0);
    const ghost = createGraph(
      ["d.ts", "e.ts"],
      (file) => ({ "d.ts": 'export { ghost } from "./e";', "e.ts": "" })[file]
    );
    expect([...ghost.exportMap("d.ts").get("ghost")]).toEqual(["e.ts"]);
  });
});

describe("bindings", () => {
  it("keys each direct export by where it comes from", () => {
    expect(bindings('export { A as B } from "./a";\nexport const C = 1;\nexport * from "./d";')).toEqual(
      new Map([
        ["C", "local"],
        ["B", "./a#A"],
        ["*./d", "./d"],
      ])
    );
  });
});

describe("affectedTests", () => {
  const select = (changedFiles, readBase = (file) => read(file)) =>
    affectedTests({ changedFiles, read, readBase, files });

  it("follows a change only to the tests that import its names, through entry files", () => {
    expect(select(["apps/web_new/src/features/auth/session.ts"])).toEqual([
      "apps/web_new/src/features/auth/session.test.ts",
    ]);
    expect(select(["packages/kit/src/Button.tsx"])).toEqual([
      "apps/web_new/src/features/all/All.test.tsx",
      "apps/web_new/src/features/auth/SignIn.test.tsx",
    ]);
  });

  it("follows chains inside packages, aliases, side-effect and dynamic imports", () => {
    expect(select(["packages/kit/src/VirtualList.tsx"])).toEqual([
      "apps/web_new/src/features/all/All.test.tsx",
      "apps/web_new/src/features/grid/Grid.test.tsx",
    ]);
    expect(select(["apps/web_new/src/features/grid/grid.css"])).toEqual([
      "apps/web_new/src/features/grid/Grid.test.tsx",
    ]);
    expect(select(["packages/api-client/src/projects.ts"])).toEqual([
      "apps/web_new/src/features/all/Lazy.test.tsx",
      "apps/web_new/src/features/auth/session.test.ts",
      "apps/web_new/src/features/cycles/cycles.test.ts",
    ]);
    expect(select(["apps/web_new/src/core/edition/oss.ts"])).toEqual(["apps/web_new/src/core/edition/edition.test.ts"]);
  });

  it("selects a changed test itself, and never a deleted one or a type-only user", () => {
    expect(select(["apps/web_new/src/features/cycles/cycles.test.ts", "apps/web_new/src/gone.test.ts"])).toEqual([
      "apps/web_new/src/features/cycles/cycles.test.ts",
    ]);
    expect(select(["packages/kit/src/tokens/colors.ts"])).not.toContain("apps/web_new/src/features/all/Types.test.ts");
  });

  it("treats an edited re-export as a change for the files importing that name from it", () => {
    const base = 'export { useSession } from "./session";\nexport { SignIn } from "./Old";';
    const result = select(["apps/web_new/src/features/auth/index.ts"], (file) =>
      file === "apps/web_new/src/features/auth/index.ts" ? base : read(file)
    );
    expect(result).toEqual(["apps/web_new/src/features/auth/SignIn.test.tsx"]);
    expect(select(["packages/kit/src/index.ts"], () => null)).toContain("apps/web_new/e2e/parity/drivers/x.test.ts");
    expect(select(["apps/web_new/src/features/auth/index.ts"], () => null)).toEqual([
      "apps/web_new/src/features/auth/SignIn.test.tsx",
      "apps/web_new/src/features/auth/session.test.ts",
    ]);
  });
});

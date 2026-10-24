// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for test-impact.mjs on small in-memory trees: how modules are
// parsed and resolved, and which test files a change reaches by imported name.
import { describe, expect, it } from "vitest";

import { affectedTests, aliasesFor, bindings, createGraph, parseModule, resolveSpecifier } from "./test-impact.mjs";

const W = "apps/web_new/src/";
const tree = {
  "packages/kit/src/index.ts": [
    'export { Button } from "./Button";',
    'export { Dialog, type DialogProps } from "./Dialog";',
    'export { Table as DataTable } from "./Table";',
    'export * from "./tokens";',
    'export * as icons from "./icons";',
    'export { Missing } from "./Missing";',
    'export * from "./gone";',
    'export type { Only } from "./Dialog";',
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
  [`${W}features/auth/index.ts`]:
    'export { useSession } from "./session";\nexport { SignIn } from "./SignIn";\nexport * from "./register";',
  [`${W}features/auth/session.ts`]:
    'import { listProjects } from "@pidash/api-client";\nexport const useSession = listProjects;',
  [`${W}features/auth/SignIn.tsx`]:
    'import { Button } from "@pidash/kit";\nexport default function SignIn() {}\nexport { SignIn };',
  [`${W}features/auth/register.ts`]:
    'import { cx } from "../../shared/cx";\nregisterAuth(cx);\nexport const registered = true;',
  [`${W}shared/cx.ts`]: 'import { useState } from "react";\nexport const cx = 2;',
  [`${W}features/auth/session.test.ts`]: 'import { useSession } from "@web_new/features/auth";',
  [`${W}features/auth/SignIn.test.tsx`]: 'import { SignIn } from "../auth";',
  [`${W}features/grid/Grid.tsx`]:
    'import { DataTable as T, type DialogProps } from "@pidash/kit";\nimport "./grid.css";\nexport default T;',
  [`${W}features/grid/grid.css`]: ".grid {}",
  [`${W}features/grid/Grid.test.tsx`]: 'import Grid from "./Grid";',
  [`${W}features/all/All.test.tsx`]: 'import * as kit from "@pidash/kit";',
  [`${W}features/all/Lazy.test.tsx`]: 'const m = await import("../cycles/cycles");',
  [`${W}features/all/Types.test.ts`]:
    'import type { Button } from "@pidash/kit";\nlet t: typeof import("@pidash/kit");',
  [`${W}features/cycles/cycles.ts`]:
    'import { listCycles } from "@pidash/api-client";\nexport const cycles = listCycles;',
  [`${W}features/cycles/cycles.test.ts`]: 'import { cycles } from "./cycles";',
  [`${W}core/edition/oss.ts`]: "export const edition = 1;",
  [`${W}core/edition/edition.test.ts`]: 'import { edition } from "@pidash/edition";',
  [`${W}test-setup.ts`]: 'import { budget } from "./testing/budget";',
  [`${W}testing/budget.ts`]: "export const budget = 1;",
  "apps/web_new/e2e/parity/drivers/x.test.ts": 'import { RED } from "@pidash/kit";',
};
const files = Object.keys(tree);
const fileSet = new Set(files);
const read = (file) => tree[file] ?? null;
const select = (changedFiles, readBase = read, over = {}) => {
  const all = { ...tree, ...over };
  return affectedTests({ changedFiles, read: (file) => all[file] ?? null, readBase, files: Object.keys(all) }).tests;
};

describe("parseModule", () => {
  it("reads value imports, skipping type-only ones, comments and re-export statements", () => {
    expect(parseModule(tree[`${W}features/grid/Grid.tsx`], "Grid.tsx").imports).toEqual([
      { from: "@pidash/kit", names: new Set(["DataTable"]) },
      { from: "./grid.css", names: null },
    ]);
    expect(parseModule(tree[`${W}features/all/Types.test.ts`]).imports).toEqual([]);
    expect(
      parseModule('import {\n  a, // first, important\n  type B,\n} from "./a"\nexport { c } from "./c";').imports
    ).toEqual([{ from: "./a", names: new Set(["a"]) }]);
    expect(parseModule('import { type T } from "./t";').imports).toEqual([]);
  });

  it("treats default as a name and namespace, side-effect and dynamic imports as everything", () => {
    expect(parseModule('import d, { a } from "./m";').imports).toEqual([
      { from: "./m", names: new Set(["default", "a"]) },
    ]);
    expect(
      parseModule('import d, * as ns from "./m";\nimport "./s";\nawait import("./x");\nimport(name);', "m.mjs").imports
    ).toEqual([
      { from: "./m", names: null },
      { from: "./s", names: null },
      { from: "./x", names: null },
    ]);
  });

  it("reads declarations of every kind, defaults, local lists, re-exports and stars", () => {
    const parsed = parseModule(
      [
        "export const { a, b: [c, , d] } = x;",
        "export const enum E { A }",
        "export function f() {}",
        "export class K {}",
        "export default function () {}",
        "const g = 1;",
        "export { g as G, type H };",
        'export { i, type J } from "./i";',
        'export type { L } from "./l";',
        'export * as ns from "./ns";',
        'export * from "./star";',
        '"use client";',
      ].join("\n"),
      "x.jsx"
    );
    expect([...parsed.local].sort()).toEqual(["E", "G", "K", "a", "c", "d", "default", "f"]);
    expect(parsed.reexports).toEqual([
      { from: "./i", specifiers: [{ name: "i", alias: "i" }] },
      { from: "./ns", specifiers: [{ name: "*", alias: "ns" }] },
    ]);
    expect(parsed.stars).toEqual(["./star"]);
    expect(parsed.effects).toBe(false);
    expect(parseModule("export default x;").local).toEqual(new Set(["default"]));
    expect(parseModule('export {} from "./e";').reexports).toEqual([]);
  });

  it("flags modules that run code at load time", () => {
    expect(parseModule(tree[`${W}features/auth/register.ts`]).effects).toBe(true);
    expect(parseModule("if (x) y();").effects).toBe(true);
    expect(parseModule("export const a = make();").effects).toBe(false);
  });
});

describe("resolveSpecifier", () => {
  it("resolves aliases, @web_new paths, relative paths, .js suffixes and index files", () => {
    expect(resolveSpecifier("x", "@pidash/kit", fileSet)).toBe("packages/kit/src/index.ts");
    expect(resolveSpecifier("x", "@pidash/edition", fileSet)).toBe(`${W}core/edition/oss.ts`);
    expect(resolveSpecifier("x", "@pidash/edition", fileSet, aliasesFor("cloud.ts"))).toBe("cloud.ts");
    expect(resolveSpecifier("x", "@web_new/features/auth", fileSet)).toBe(`${W}features/auth/index.ts`);
    expect(resolveSpecifier("packages/kit/src/Table.tsx", "./VirtualList.js", fileSet)).toBe(
      "packages/kit/src/VirtualList.tsx"
    );
    expect(resolveSpecifier(`${W}features/all/Lazy.test.tsx`, "../cycles/cycles", fileSet)).toBe(
      `${W}features/cycles/cycles.ts`
    );
    expect(resolveSpecifier(`${W}features/grid/Grid.tsx`, "./grid.css", fileSet)).toBe(`${W}features/grid/grid.css`);
  });

  it("returns null for npm packages and missing files", () => {
    expect(resolveSpecifier("x", "react", fileSet)).toBeNull();
    expect(resolveSpecifier("packages/kit/src/index.ts", "./nowhere", fileSet)).toBeNull();
  });
});

describe("createGraph", () => {
  it("maps each public name to its declaring file through aliases, nested entries and stars", () => {
    const kit = createGraph(files, read).exportMap("packages/kit/src/index.ts");
    expect([...kit.get("Button")]).toEqual(["packages/kit/src/Button.tsx"]);
    expect([...kit.get("DataTable")]).toEqual(["packages/kit/src/Table.tsx"]);
    expect([...kit.get("RED")]).toEqual(["packages/kit/src/tokens/colors.ts"]);
    expect([...kit.get("SIZE")]).toEqual(["packages/kit/src/tokens/index.ts"]);
    expect([...kit.get("icons")]).toEqual(["packages/kit/src/icons.ts"]);
    expect(kit.has("Missing")).toBe(false);
    expect(kit.has("Only")).toBe(false);
  });

  it("does not cache an export map left incomplete by a re-export cycle", () => {
    const loop = {
      "p.ts": 'export * from "./q";\nexport const pa = 1;',
      "q.ts": 'export * from "./p";\nexport { qa as qb } from "./p";\nexport const qa = 1;',
      "r.ts": 'export { ghost } from "./s";',
      "s.ts": "",
    };
    const graph = createGraph(Object.keys(loop), (file) => loop[file]);
    expect([...graph.exportMap("q.ts").get("pa")]).toEqual(["p.ts"]);
    expect([...graph.exportMap("p.ts").get("qa")]).toEqual(["q.ts"]);
    expect([...graph.exportMap("r.ts").get("ghost")]).toEqual(["s.ts"]);
  });
});

describe("bindings", () => {
  it("keys each direct export by where it comes from", () => {
    expect(bindings(parseModule('export { A as B } from "./a";\nexport const C = 1;\nexport * from "./d";'))).toEqual(
      new Map([
        ["C", "local"],
        ["B", "./a#A"],
        ["*./d", "./d"],
      ])
    );
  });
});

describe("affectedTests", () => {
  it("follows a change only to the tests that import its names, through entry files", () => {
    expect(select([`${W}features/auth/session.ts`])).toEqual([`${W}features/auth/session.test.ts`]);
    expect(select(["packages/kit/src/Button.tsx"])).toEqual([
      `${W}features/all/All.test.tsx`,
      `${W}features/auth/SignIn.test.tsx`,
    ]);
  });

  it("follows chains inside packages, aliases, side-effect and dynamic imports", () => {
    expect(select(["packages/kit/src/VirtualList.tsx"])).toEqual([
      `${W}features/all/All.test.tsx`,
      `${W}features/grid/Grid.test.tsx`,
    ]);
    expect(select([`${W}features/grid/grid.css`])).toEqual([`${W}features/grid/Grid.test.tsx`]);
    expect(select(["packages/api-client/src/projects.ts"])).toEqual([
      `${W}features/all/Lazy.test.tsx`,
      `${W}features/auth/session.test.ts`,
      `${W}features/cycles/cycles.test.ts`,
    ]);
    expect(select([`${W}core/edition/oss.ts`])).toEqual([`${W}core/edition/edition.test.ts`]);
  });

  it("selects every test that loads a module running code at load time, whatever it imports", () => {
    expect(select([`${W}shared/cx.ts`])).toEqual([
      `${W}features/auth/SignIn.test.tsx`,
      `${W}features/auth/session.test.ts`,
    ]);
    const before = 'import { cx } from "../../shared/cx";\nregisterAuth(cx);\nexport const registered = true;';
    const after = { [`${W}features/auth/register.ts`]: "export const registered = true;" };
    expect(select([`${W}features/auth/register.ts`], () => before, after)).toEqual([
      `${W}features/auth/SignIn.test.tsx`,
      `${W}features/auth/session.test.ts`,
    ]);
  });

  it("selects a changed test itself, and never a deleted one or a type-only user", () => {
    expect(select([`${W}features/cycles/cycles.test.ts`, `${W}gone.test.ts`])).toEqual([
      `${W}features/cycles/cycles.test.ts`,
    ]);
    expect(select(["packages/kit/src/tokens/colors.ts"])).not.toContain(`${W}features/all/Types.test.ts`);
  });

  it("follows an edited re-export to importers of the edited file and of entry files passing it on", () => {
    const nested = {
      "packages/kit/src/index.ts":
        'export * from "./components";\nexport { Button as Btn, Other as O } from "./components";\nexport * as comps from "./components";',
      "packages/kit/src/components/index.ts": 'export { Button } from "./B2";',
      "packages/kit/src/components/B1.ts": "export const Button = 1;",
      "packages/kit/src/components/B2.ts": "export const Button = 2;",
      [`${W}a.test.ts`]: 'import { Button } from "@pidash/kit";',
      [`${W}b.test.ts`]: 'import { Btn } from "@pidash/kit";',
      [`${W}c.test.ts`]: 'import { Other } from "@pidash/kit";',
      [`${W}d.test.ts`]: 'import { comps } from "@pidash/kit";',
    };
    const result = affectedTests({
      changedFiles: ["packages/kit/src/components/index.ts"],
      read: (file) => nested[file] ?? null,
      readBase: () => 'export { Button } from "./B1";',
      files: Object.keys(nested),
    });
    expect(result.tests).toEqual([`${W}a.test.ts`, `${W}b.test.ts`, `${W}d.test.ts`]);
    const starAdded = affectedTests({
      changedFiles: ["packages/kit/src/components/index.ts"],
      read: (file) => nested[file] ?? null,
      readBase: () => 'export { Button } from "./B2";\nexport * from "./B1";',
      files: Object.keys(nested),
    });
    expect(starAdded.tests).toEqual([`${W}a.test.ts`, `${W}b.test.ts`, `${W}c.test.ts`, `${W}d.test.ts`]);
  });

  it("treats added, removed and star changes in an entry file as changes for their importers", () => {
    const base =
      'export { useSession } from "./session";\nexport { SignIn } from "./Old";\nexport * from "./register";';
    expect(select([`${W}features/auth/index.ts`], () => base)).toEqual([`${W}features/auth/SignIn.test.tsx`]);
    const removed = `${tree[`${W}features/auth/index.ts`]}\nexport { gone } from "./gone";`;
    expect(select([`${W}features/auth/index.ts`], () => removed)).toEqual([]);
    expect(select([`${W}features/auth/index.ts`], () => null)).toEqual([
      `${W}features/auth/SignIn.test.tsx`,
      `${W}features/auth/session.test.ts`,
    ]);
    expect(select(["packages/kit/src/index.ts"], () => null)).toContain("apps/web_new/e2e/parity/drivers/x.test.ts");
  });

  it("follows an edited re-export around a re-export cycle without looping", () => {
    const loop = {
      [`${W}p.ts`]: 'export * from "./q";\nexport const pa = 1;',
      [`${W}q.ts`]: 'export * from "./p";\nexport { qa } from "./r";',
      [`${W}r.ts`]: "export const qa = 1;",
      [`${W}t.test.ts`]: 'import { qa } from "./p";',
    };
    const result = affectedTests({
      changedFiles: [`${W}q.ts`],
      read: (file) => loop[file] ?? null,
      readBase: () => 'export * from "./p";',
      files: Object.keys(loop),
    });
    expect(result.tests).toEqual([`${W}t.test.ts`]);
  });

  it("reports a change that reaches the test setup file", () => {
    const run = (changedFiles) => affectedTests({ changedFiles, read, readBase: read, files });
    expect(run([`${W}testing/budget.ts`])).toEqual({ tests: [], setupReached: true });
    expect(run([`${W}shared/cx.ts`]).setupReached).toBe(false);
  });
});

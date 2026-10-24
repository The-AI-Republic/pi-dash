// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for package-impact.mjs on a small in-memory @pidash/kit and
// @pidash/api-client: which names a package change affects and which web_new
// files use them.
import { describe, expect, it } from "vitest";

import {
  affectedPackageFiles,
  entryBindings,
  importsAny,
  packageImpact,
  parseExports,
  publicSources,
  relativeImports,
  resolveModule,
} from "./package-impact.mjs";

const KIT = { name: "@pidash/kit", src: "packages/kit/src/" };

const tree = {
  "packages/kit/src/index.ts": [
    'export { Button } from "./components/Button";',
    'export type { ButtonProps } from "./components/Button";',
    'export { Dialog, type DialogProps } from "./components/Dialog";',
    'export { Table as DataTable } from "./components/Table";',
    'export * from "./tokens";',
    'export * as icons from "./icons";',
    'export { Missing } from "./components/Missing";',
    'export * from "./gone";',
    'export * from "./tokens";',
    "export const KIT_VERSION = 1;",
  ].join("\n"),
  "packages/kit/src/components/Button.tsx":
    'import { cx } from "../util/cx";\nexport function Button() {}\nexport type ButtonProps = {};',
  "packages/kit/src/components/Dialog.tsx":
    'import { Button } from "./Button";\nexport const Dialog = () => Button;\nexport interface DialogProps {}',
  "packages/kit/src/components/Table.tsx": 'import { VirtualList } from "./VirtualList.js";\nexport class Table {}',
  "packages/kit/src/components/VirtualList.tsx": "export const VirtualList = 1;",
  "packages/kit/src/util/cx.ts": "export const cx = 1;",
  "packages/kit/src/tokens/index.ts": 'export * from "./colors";\nconst size = 2;\nexport { size as SIZE };',
  "packages/kit/src/tokens/colors.ts": "export const RED = 1;\nexport enum Tone { A }",
  "packages/kit/src/icons.ts": "export const Plus = 1;",
  "packages/kit/src/styles.css": ".a {}",
  "packages/api-client/src/index.ts":
    'export { listProjects, Project } from "./contracts/projects.js";\nexport { listCycles } from "./contracts/cycles.js";',
  "packages/api-client/src/contracts/projects.ts": "export const Project = 1;\nexport async function listProjects() {}",
  "packages/api-client/src/contracts/cycles.ts":
    'import { Project } from "./projects.js";\nexport const listCycles = () => Project;',
  "apps/web_new/src/features/a/Screen.tsx": 'import { Button, type ButtonProps } from "@pidash/kit";',
  "apps/web_new/src/features/a/Types.ts":
    'import type { Button } from "@pidash/kit";\nimport { type Dialog } from "@pidash/kit";',
  "apps/web_new/src/features/b/Grid.tsx": 'import { DataTable as T } from "@pidash/kit";',
  "apps/web_new/src/features/b/All.tsx": 'import * as kit from "@pidash/kit";',
  "apps/web_new/src/features/b/Default.tsx": 'import kitDefault from "@pidash/kit";',
  "apps/web_new/src/shared/reexport.ts": 'export { Dialog } from "@pidash/kit";',
  "apps/web_new/src/shared/star.ts": 'export * from "@pidash/kit";',
  "apps/web_new/src/features/c/cycles.ts": 'import { listCycles } from "@pidash/api-client";',
  "apps/web_new/src/features/c/projects.test.ts": 'import { listProjects } from "@pidash/api-client";',
  "apps/web_new/e2e/parity/drivers/x.test.ts": 'import { RED } from "@pidash/kit";',
  "apps/web_new/src/styles.css": "@import '@pidash/kit';",
  "apps/web_new/package.json": "{}",
};
const files = Object.keys(tree);
const fileSet = new Set(files);
const read = (file) => tree[file] ?? null;

describe("parseExports", () => {
  it("reads declarations, re-exports, local export lists and stars", () => {
    const parsed = parseExports(tree["packages/kit/src/index.ts"]);
    expect([...parsed.local]).toEqual(["KIT_VERSION"]);
    expect(parsed.reexports[2]).toEqual({
      from: "./components/Dialog",
      specifiers: [
        { name: "Dialog", alias: "Dialog", type: false },
        { name: "DialogProps", alias: "DialogProps", type: true },
      ],
    });
    expect(parsed.reexports.find((entry) => entry.from === "./icons").specifiers).toEqual([
      { name: "*", alias: "icons", type: false },
    ]);
    expect(parsed.stars).toEqual(["./tokens", "./gone", "./tokens"]);
    expect([...parseExports(tree["packages/kit/src/tokens/index.ts"]).local]).toEqual(["SIZE"]);
    expect([...parseExports("export default function () {}\nexport { a, };").local]).toEqual(["a"]);
  });
});

describe("relativeImports and resolveModule", () => {
  it("collects relative imports, re-exports and dynamic imports only", () => {
    expect(
      relativeImports(
        'import a from "./a";\nexport * from "../b.js";\nconst c = import("./c");\nimport "./d";\nimport x from "react";'
      )
    ).toEqual(["./a", "../b.js", "./c", "./d"]);
  });

  it("resolves .js suffixes, extensions, parent dirs and index files", () => {
    expect(resolveModule("packages/kit/src/components/Table.tsx", "./VirtualList.js", fileSet)).toBe(
      "packages/kit/src/components/VirtualList.tsx"
    );
    expect(resolveModule("packages/kit/src/components/Button.tsx", "../util/cx", fileSet)).toBe(
      "packages/kit/src/util/cx.ts"
    );
    expect(resolveModule("packages/kit/src/index.ts", "./tokens", fileSet)).toBe("packages/kit/src/tokens/index.ts");
    expect(resolveModule("packages/kit/src/index.ts", "./styles.css", fileSet)).toBe("packages/kit/src/styles.css");
    expect(resolveModule("packages/kit/src/index.ts", "./nowhere", fileSet)).toBeNull();
  });
});

describe("publicSources", () => {
  it("maps each public name to its declaring file through aliases, nested barrels and stars", () => {
    const sources = publicSources(KIT, read, fileSet);
    expect(sources.get("Button")).toBe("packages/kit/src/components/Button.tsx");
    expect(sources.get("DialogProps")).toBe("packages/kit/src/components/Dialog.tsx");
    expect(sources.get("DataTable")).toBe("packages/kit/src/components/Table.tsx");
    expect(sources.get("RED")).toBe("packages/kit/src/tokens/colors.ts");
    expect(sources.get("SIZE")).toBe("packages/kit/src/tokens/index.ts");
    expect(sources.get("icons")).toBe("packages/kit/src/icons.ts");
    expect(sources.get("KIT_VERSION")).toBe("packages/kit/src/index.ts");
    expect(sources.has("Missing")).toBe(false);
  });

  it("survives an entry file that cannot be read", () => {
    expect(publicSources(KIT, () => null, fileSet).size).toBe(0);
  });
});

describe("affectedPackageFiles", () => {
  it("follows importers inside the package, but not through barrels", () => {
    expect([...affectedPackageFiles(KIT, ["packages/kit/src/components/Button.tsx"], read, fileSet)].sort()).toEqual([
      "packages/kit/src/components/Button.tsx",
      "packages/kit/src/components/Dialog.tsx",
    ]);
    expect([...affectedPackageFiles(KIT, ["packages/kit/src/util/cx.ts"], read, fileSet)].sort()).toEqual([
      "packages/kit/src/components/Button.tsx",
      "packages/kit/src/components/Dialog.tsx",
      "packages/kit/src/util/cx.ts",
    ]);
  });

  it("stops at files it has already reached when imports form a cycle", () => {
    const loop = new Set(["packages/kit/src/a.ts", "packages/kit/src/b.ts"]);
    const text = { "packages/kit/src/a.ts": 'import "./b";', "packages/kit/src/b.ts": 'import "./a";' };
    expect([...affectedPackageFiles(KIT, ["packages/kit/src/b.ts"], (file) => text[file], loop)].sort()).toEqual([
      "packages/kit/src/a.ts",
      "packages/kit/src/b.ts",
    ]);
  });

  it("ignores unresolvable imports and unreadable files", () => {
    const sparse = new Set(["packages/kit/src/a.ts", "packages/kit/src/b.ts"]);
    const text = { "packages/kit/src/a.ts": 'import x from "./missing";' };
    expect([...affectedPackageFiles(KIT, ["packages/kit/src/b.ts"], (file) => text[file] ?? null, sparse)]).toEqual([
      "packages/kit/src/b.ts",
    ]);
  });
});

describe("entryBindings", () => {
  it("keys each direct export by where it comes from", () => {
    expect(entryBindings('export { A as B } from "./a";\nexport const C = 1;\nexport * from "./d";')).toEqual(
      new Map([
        ["C", "local"],
        ["B", "./a#A"],
        ["*./d", "./d"],
      ])
    );
  });
});

describe("importsAny", () => {
  const names = new Set(["Button", "Dialog"]);

  it("finds value imports and re-exports of an affected name", () => {
    expect(importsAny(tree["apps/web_new/src/features/a/Screen.tsx"], KIT, names)).toBe(true);
    expect(importsAny(tree["apps/web_new/src/shared/reexport.ts"], KIT, names)).toBe(true);
    expect(importsAny('import { Button as B } from "@pidash/kit";', KIT, names)).toBe(true);
  });

  it("skips type-only imports, other names and other packages", () => {
    expect(importsAny(tree["apps/web_new/src/features/a/Types.ts"], KIT, names)).toBe(false);
    expect(importsAny('import { Avatar } from "@pidash/kit";', KIT, names)).toBe(false);
    expect(importsAny('import { Button } from "@pidash/kit-extra";', KIT, names)).toBe(false);
    expect(importsAny('import { Button } from "@pidash/api-client";', KIT, names)).toBe(false);
  });

  it("counts default, namespace and star imports as using everything", () => {
    expect(importsAny(tree["apps/web_new/src/features/b/All.tsx"], KIT, names)).toBe(true);
    expect(importsAny(tree["apps/web_new/src/features/b/Default.tsx"], KIT, names)).toBe(true);
    expect(importsAny('import kit, { Avatar } from "@pidash/kit";', KIT, names)).toBe(true);
    expect(importsAny(tree["apps/web_new/src/shared/star.ts"], KIT, names)).toBe(true);
  });
});

describe("packageImpact", () => {
  const impact = (changedFiles, readBase = () => null) => packageImpact({ changedFiles, read, files, readBase });

  it("does nothing when no package source changed", () => {
    expect(impact(["apps/web_new/src/features/a/Screen.tsx"])).toEqual({ packageFiles: [], names: [], consumers: [] });
  });

  it("swaps a changed component for the web_new files that use what it affects", () => {
    const result = impact(["packages/kit/src/components/Button.tsx", "apps/web_new/src/features/a/Screen.tsx"]);
    expect(result.packageFiles).toEqual(["packages/kit/src/components/Button.tsx"]);
    expect(result.names).toEqual([
      "@pidash/kit:Button",
      "@pidash/kit:ButtonProps",
      "@pidash/kit:Dialog",
      "@pidash/kit:DialogProps",
    ]);
    expect(result.consumers).toEqual([
      "apps/web_new/src/features/a/Screen.tsx",
      "apps/web_new/src/features/b/All.tsx",
      "apps/web_new/src/features/b/Default.tsx",
      "apps/web_new/src/shared/reexport.ts",
      "apps/web_new/src/shared/star.ts",
    ]);
  });

  it("follows names through aliases and covers e2e and test files", () => {
    expect(impact(["packages/kit/src/components/VirtualList.tsx"]).consumers).toContain(
      "apps/web_new/src/features/b/Grid.tsx"
    );
    expect(impact(["packages/kit/src/tokens/colors.ts"]).consumers).toContain(
      "apps/web_new/e2e/parity/drivers/x.test.ts"
    );
    const api = impact(["packages/api-client/src/contracts/projects.ts"]);
    expect(api.names).toEqual([
      "@pidash/api-client:Project",
      "@pidash/api-client:listCycles",
      "@pidash/api-client:listProjects",
    ]);
    expect(api.consumers).toEqual([
      "apps/web_new/src/features/c/cycles.ts",
      "apps/web_new/src/features/c/projects.test.ts",
    ]);
  });

  it("treats an edit to the entry file as a change to the names it added or re-pointed", () => {
    const base =
      'export { Button } from "./components/Button";\nexport { Table as DataTable } from "./components/VirtualList";';
    const result = impact(["packages/kit/src/index.ts"], (file) =>
      file === "packages/kit/src/index.ts" ? base : null
    );
    expect(result.names).toContain("@pidash/kit:Dialog");
    expect(result.names).toContain("@pidash/kit:DataTable");
    expect(result.names).not.toContain("@pidash/kit:Button");
    expect(result.names).toContain("@pidash/kit:KIT_VERSION");
  });

  it("copes with an entry file or a consumer it cannot read", () => {
    const partial = (file) => (file === "packages/kit/src/index.ts" ? null : read(file));
    const result = packageImpact({
      changedFiles: ["packages/kit/src/index.ts"],
      read: partial,
      files: [...files, "apps/web_new/src/deleted.ts"],
      readBase: () => 'export { Button } from "./components/Button";',
    });
    expect(result).toEqual({ packageFiles: ["packages/kit/src/index.ts"], names: [], consumers: [] });
  });

  it("treats a new entry file as new names and skips consumers it cannot read", () => {
    const added = impact(["packages/api-client/src/index.ts"]);
    expect(added.names).toEqual([
      "@pidash/api-client:Project",
      "@pidash/api-client:listCycles",
      "@pidash/api-client:listProjects",
    ]);
    const result = packageImpact({
      changedFiles: ["packages/kit/src/components/Button.tsx"],
      read,
      files: [...files, "apps/web_new/src/deleted.ts"],
      readBase: () => null,
    });
    expect(result.consumers).not.toContain("apps/web_new/src/deleted.ts");
  });

  it("finds no names for a package change nothing public depends on", () => {
    const result = impact(["packages/kit/src/styles.css", "packages/kit/src/components/Button.test.tsx"]);
    expect(result.packageFiles).toEqual(["packages/kit/src/styles.css", "packages/kit/src/components/Button.test.tsx"]);
    expect(result.names).toEqual([]);
    expect(result.consumers).toEqual([]);
  });
});

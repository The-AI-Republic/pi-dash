// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Build negatives for MOD-041 (parity/specs/modules.md §11.2): module
// order comes from the sort control only, so a list row and a board card
// carry no drag affordance and no drag handler; and no modules surface —
// list, board, detail, dialogs, sweep surface, palette entries — offers an
// export, import or print action. The timeline's own bar/row gestures are
// MOD-021's and stay out of this scan by name.
//
// The token lists below are matched against comment-free source, so the
// prose that explains the absence (this header included) never trips it,
// and each list is proved to flag real usage before it is trusted.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const SRC = join(here, "..", "..", "..");

/** Comment-free body: prose may name the absences, code may not. */
function scanBody(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[\s;])\/\/[^\n]*/g, "$1");
}

function hits(source: string, tokens: RegExp[]): string[] {
  return tokens.filter((token) => token.test(source)).map((token) => token.source);
}

function sourcesUnder(directory: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(directory)) {
    const full = join(directory, name);
    if (statSync(full).isDirectory()) out.push(...sourcesUnder(full));
    else if (/\.(ts|tsx)$/.test(name) && !/\.test\.tsx?$/.test(name)) out.push(full);
  }
  return out;
}

/** Files that render a module as a row or a card, and the list body that mounts them. */
const ROW_AND_CARD_SOURCES = [
  join(SRC, "features", "modules", "components", "ModuleListRow.tsx"),
  join(SRC, "features", "modules", "components", "ModuleBoardCard.tsx"),
  join(SRC, "features", "modules", "components", "ModulesListBody.tsx"),
];

/** Every modules source plus its routes; the timeline is excluded by name below. */
const MODULES_SOURCES = [
  ...sourcesUnder(join(SRC, "features", "modules")),
  join(SRC, "routes", "$ws", "_gated", "projects", "$projectId", "modules", "index.tsx"),
  join(SRC, "routes", "$ws", "_gated", "projects", "$projectId", "modules", "$moduleId.tsx"),
  join(SRC, "routes", "$ws", "_gated", "projects", "$projectId", "archives", "modules", "index.tsx"),
].filter((file) => !file.includes(join("modules", "components", "timeline")));

/**
 * Drag affordances a reorderable row or card would carry: the HTML
 * attribute, the handlers, and the sortable libraries. A menu entry that
 * moves a row is not on this list — that is a sort or a move control.
 */
const DRAG_TOKENS = [
  /\bdraggable\b/,
  /\bonDrag(Start|End|Over|Enter|Leave|Drop)\b/,
  /\bonDrop\b/,
  /\bonPointerDown\b/,
  /\buseSortable\b/,
  /\bSortableContext\b/,
  /\bDndContext\b/,
  /@dnd-kit/,
  /react-dnd/,
  /\breact-beautiful-dnd\b/,
];

/**
 * Export/import/print affordances. A quoted label is what a user would
 * read; the URL words catch a download or print endpoint reached without a
 * label. Ordinary identifiers (`export function`, `import * as`) are the
 * language's, not an affordance, so only quoted text counts here.
 */
const EXPORT_TOKENS = [
  /["'`][^"'`\n]*\b(Export|Import|Print|Download|CSV)\b[^"'`\n]*["'`]/,
  />\s*(Export|Import|Print|Download|CSV)\b/,
  /\/export\//,
  /\/import\//,
  /\/print\//,
  /\bwindow\.print\b/,
  /\bDataTransfer\b/,
  /\bnew Blob\b/,
];

const short = (path: string): string => relative(SRC, path);

describe("modules drag scan scope", () => {
  it("covers the row, card and list body", () => {
    for (const file of ROW_AND_CARD_SOURCES) expect(statSync(file).isDirectory()).toBe(false);
  });

  it("covers the feature and its routes, leaving the timeline's own gestures out", () => {
    expect(MODULES_SOURCES.length).toBeGreaterThan(40);
    expect(MODULES_SOURCES.some((file) => file.endsWith(join("components", "dialogs", "ModuleForm.tsx")))).toBe(true);
    expect(MODULES_SOURCES.some((file) => file.includes(join("components", "timeline")))).toBe(false);
    for (const file of MODULES_SOURCES) expect(short(file)).not.toMatch(/\.test\.tsx?$/);
  });
});

describe("modules row and card drag (MOD-041)", () => {
  it("flags the affordances a reorderable row would carry", () => {
    expect(hits(scanBody("<li draggable onDragStart={begin}>"), DRAG_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("const { attributes } = useSortable({ id })"), DRAG_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("<DndContext onDragEnd={settle}>"), DRAG_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('import { useDraggable } from "@dnd-kit/core"'), DRAG_TOKENS)).not.toEqual([]);
  });

  it("passes prose, a plain anchor row and the sort control's own click", () => {
    expect(hits(scanBody("// rows are not draggable; the sort control orders them"), DRAG_TOKENS)).toEqual([]);
    expect(hits(scanBody('onClick={() => setSort("name")}'), DRAG_TOKENS)).toEqual([]);
    expect(hits(scanBody("<a href={moduleHref}>Delivery</a>"), DRAG_TOKENS)).toEqual([]);
  });

  it("finds no drag affordance on the row, the card or the list body", () => {
    for (const file of ROW_AND_CARD_SOURCES) {
      expect(hits(scanBody(readFileSync(file, "utf8")), DRAG_TOKENS), short(file)).toEqual([]);
    }
  });
});

describe("modules export, import and print (MOD-041)", () => {
  it("flags labels and download plumbing", () => {
    expect(hits(scanBody("<button>Export CSV</button>"), EXPORT_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('const label = "Print";'), EXPORT_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('client.get("/api/workspaces/acme/export/")'), EXPORT_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("window.print()"), EXPORT_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("new Blob([csv])"), EXPORT_TOKENS)).not.toEqual([]);
  });

  it("passes ES module syntax, ordinary labels and prose", () => {
    expect(hits(scanBody('import * as React from "react"; export function Row() {}'), EXPORT_TOKENS)).toEqual([]);
    expect(hits(scanBody('const label = "Export modules";'), EXPORT_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('const heading = "Sort" + name;'), EXPORT_TOKENS)).toEqual([]);
    expect(hits(scanBody('import { ModuleRow } from "./rows.js";'), EXPORT_TOKENS)).toEqual([]);
  });

  it("finds no export, import or print action on any modules surface", () => {
    for (const file of MODULES_SOURCES) {
      expect(hits(scanBody(readFileSync(file, "utf8")), EXPORT_TOKENS), short(file)).toEqual([]);
    }
  });
});

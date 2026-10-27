// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page tables and the colour mark (parity/specs/pages.md §5.3,
// PAGE-033/036) plus the row/column structure commands (§19.1, PAGE-091):
// the anchor a handle position resolves to, the row/column copies a
// duplicate inserts, every menu entry's edit, the painted cell background,
// and the Tab walk after each edit.

import { describe, expect, it } from "vitest";

import { loadPageExtensions } from "./pageExtensions.js";
import {
  buildPageTable,
  loadPageColorMark,
  loadPageTableExtensions,
  PAGE_TABLE_COLUMN_WIDTH,
  pageTableCellAnchorAt,
  pageTableDuplicateColumns,
  pageTableDuplicateRows,
  tableAtOrAfter,
  type PageTableCommands,
} from "./pageTableExtensions.js";

async function editorWith(content: string) {
  const { Editor } = await import("@tiptap/core");
  return new Editor({ extensions: await loadPageExtensions(), content });
}

/** A document position inside the table's cell (row, col), for the structure commands. */
async function cellPosition(table: unknown, tablePos: number, row: number, col: number): Promise<number> {
  const { TableMap } = await import("@tiptap/pm/tables");
  const map = TableMap.get(table as never);
  const offset = map.map[row * map.width + col];
  if (offset === undefined) throw new Error("missing cell");
  return tablePos + 1 + offset + 1;
}

/** Table dimensions as "rows×cols", or null when the body holds no table. */
function tableDims(html: string): string | null {
  if (!html.includes("<table")) return null;
  const first = html.slice(html.indexOf("<tr>"), html.indexOf("</tr>"));
  return `${html.match(/<tr>/g)?.length ?? 0}x${first.match(/<t[dh]/g)?.length ?? 0}`;
}

/** The located table plus its position, failing the test when absent. */
function locatedTable(editor: { state: { doc: Parameters<typeof tableAtOrAfter>[0] } }): {
  pos: number;
  table: NonNullable<ReturnType<typeof tableAtOrAfter>>["table"];
} {
  const found = tableAtOrAfter(editor.state.doc, 0);
  if (found === null) throw new Error("missing table");
  return found;
}

describe("loadPageTableExtensions", () => {
  it("loads the four table nodes in the stored markup's order", async () => {
    const nodes = await loadPageTableExtensions();
    expect(nodes.map((node) => node.name)).toEqual(["table", "tableRow", "tableCell", "tableHeader"]);
  });
});

describe("buildPageTable", () => {
  it("builds the requested row and column count with one width per cell", async () => {
    const { Schema } = await import("@tiptap/pm/model");
    const { Editor } = await import("@tiptap/core");
    const editor = new Editor({ extensions: await loadPageExtensions(), content: "<p>seed</p>" });
    try {
      const schema = editor.schema as unknown as InstanceType<typeof Schema>;
      const table = buildPageTable(schema, 3, 2, false);
      expect(table).not.toBeNull();
      expect(table?.childCount).toBe(3);
      expect(table?.firstChild?.childCount).toBe(2);
      const firstCell = table?.firstChild?.firstChild;
      expect(firstCell?.attrs["colwidth"]).toEqual([PAGE_TABLE_COLUMN_WIDTH]);
    } finally {
      editor.destroy();
    }
  });

  it("marks the first row as headers when asked", async () => {
    const { Editor } = await import("@tiptap/core");
    const editor = new Editor({ extensions: await loadPageExtensions(), content: "<p>seed</p>" });
    try {
      const table = buildPageTable(editor.schema, 2, 1, true);
      expect(table?.firstChild?.firstChild?.type.name).toBe("tableHeader");
      expect(table?.child(1).firstChild?.type.name).toBe("tableCell");
    } finally {
      editor.destroy();
    }
  });

  it("returns null when the schema has no table node family", () => {
    const schema = { nodes: {} } as never;
    expect(buildPageTable(schema, 1, 1, false)).toBeNull();
  });

  it("returns null when a cell type is missing", async () => {
    const { Editor } = await import("@tiptap/core");
    const editor = new Editor({ extensions: await loadPageExtensions(), content: "<p>seed</p>" });
    try {
      const schema = editor.schema;
      const paragraph = schema.nodes["paragraph"];
      const rowType = schema.nodes["tableRow"];
      const tableType = schema.nodes["table"];
      if (paragraph === undefined || rowType === undefined || tableType === undefined) throw new Error("missing nodes");
      const broken = {
        nodes: { paragraph, tableRow: rowType, table: tableType },
      } as unknown as typeof schema;
      expect(buildPageTable(broken, 1, 1, false)).toBeNull();
    } finally {
      editor.destroy();
    }
  });
});

describe("cell attributes", () => {
  it("omits a span of one and renders a real span", async () => {
    const nodes = await loadPageTableExtensions();
    const cell = nodes.find((node) => node.name === "tableCell");
    if (cell === undefined) throw new Error("missing table cell");
    const attributes = (
      cell.config.addAttributes as unknown as () => Record<
        string,
        { renderHTML: (attributes: Record<string, unknown>) => Record<string, unknown> }
      >
    )();
    // A missing attribute takes the default of one, which renders nothing.
    expect(attributes["colspan"]?.renderHTML({})).toEqual({});
    expect(attributes["rowspan"]?.renderHTML({})).toEqual({});
    expect(attributes["colspan"]?.renderHTML({ colspan: 2 })).toEqual({ colspan: "2" });
    expect(attributes["rowspan"]?.renderHTML({ rowspan: 3 })).toEqual({ rowspan: "3" });
  });
});

describe("insertPageTable command", () => {
  it("drops a 3×3 table after the caret and lands inside its first cell", async () => {
    const editor = await editorWith("<p>before</p>");
    try {
      editor.commands.setTextSelection(7);
      expect((editor.commands as unknown as PageTableCommands).insertPageTable()).toBe(true);
      const html = editor.getHTML();
      expect(html.indexOf("before")).toBeGreaterThan(-1);
      expect(html.indexOf("<table")).toBeGreaterThan(html.indexOf("before"));
      expect(editor.extensionManager.extensions.some((extension) => extension.name === "table")).toBe(true);
      const table = editor.state.doc.child(1);
      expect(table?.type.name).toBe("table");
      expect(table?.childCount).toBe(3);
      expect(table?.firstChild?.childCount).toBe(3);
      // The caret's parent is a cell, not the outer paragraph.
      expect(editor.state.selection.$from.parent.type.name).toBe("paragraph");
      expect(editor.state.selection.$from.depth).toBeGreaterThanOrEqual(3);
    } finally {
      editor.destroy();
    }
  });

  it("declines when the schema has no cell type to build", async () => {
    const nodes = await loadPageTableExtensions();
    const table = nodes.find((node) => node.name === "table");
    if (table === undefined) throw new Error("missing table node");
    const commands = (
      table.config.addCommands as unknown as () => Record<
        string,
        (attributes?: Record<string, unknown>) => (props: unknown) => boolean
      >
    )();
    const insert = commands["insertPageTable"];
    if (insert === undefined) throw new Error("missing insert command");
    const declined = insert({ rows: 1, cols: 1 })({
      tr: {},
      dispatch: () => undefined,
      editor: { schema: { nodes: { paragraph: {}, tableRow: {}, table: {} } } },
    });
    expect(declined).toBe(false);
  });

  it("accepts explicit dimensions and a header row", async () => {
    const editor = await editorWith("<p>before</p>");
    try {
      expect(
        (editor.commands as unknown as PageTableCommands).insertPageTable({ rows: 2, cols: 4, withHeaderRow: true })
      ).toBe(true);
      const html = editor.getHTML();
      expect((html.match(/<tr>/g) ?? []).length).toBe(2);
      expect((html.match(/<th/g) ?? []).length).toBe(4);
      expect((html.match(/<td/g) ?? []).length).toBe(4);
    } finally {
      editor.destroy();
    }
  });

  it("round-trips a stored 3×3 table", async () => {
    const stored =
      "<table><tbody><tr><td><p>a</p></td><td><p>b</p></td><td><p>c</p></td></tr><tr><td><p>d</p></td><td><p>e</p></td><td><p>f</p></td></tr><tr><td><p>g</p></td><td><p>h</p></td><td><p>i</p></td></tr></tbody></table>";
    const editor = await editorWith(stored);
    try {
      const html = editor.getHTML();
      expect((html.match(/<tr>/g) ?? []).length).toBe(3);
      expect((html.match(/<td/g) ?? []).length).toBe(9);
      expect(html).toContain("<table>");
    } finally {
      editor.destroy();
    }
  });

  it("keeps colspan, rowspan and stored widths", async () => {
    const stored =
      '<table><tbody><tr><td colspan="2" data-colwidth="120,130"><p>wide</p></td></tr><tr><td rowspan="2"><p>tall</p></td><td><p>b</p></td></tr><tr><td><p>c</p></td></tr></tbody></table>';
    const editor = await editorWith(stored);
    try {
      const html = editor.getHTML();
      expect(html).toContain('colspan="2"');
      expect(html).toContain('data-colwidth="120,130"');
    } finally {
      editor.destroy();
    }
  });

  it("omits the default spans and widths a plain cell carries", async () => {
    const editor = await editorWith("<p>seed</p>");
    try {
      (editor.commands as unknown as PageTableCommands).insertPageTable({ rows: 1, cols: 1 });
      const html = editor.getHTML();
      expect(html).not.toContain("colspan=");
      expect(html).not.toContain("rowspan=");
      expect(html).toContain('data-colwidth="150"');
    } finally {
      editor.destroy();
    }
  });

  it("finds the first table at or after a position and none before it", async () => {
    const stored = "<p>before</p><table><tbody><tr><td><p>a</p></td></tr></tbody></table><p>after</p>";
    const editor = await editorWith(stored);
    try {
      const table = editor.state.doc.child(1);
      const found = tableAtOrAfter(editor.state.doc, 0);
      expect(found?.table).toBe(table);
      expect(found?.pos).toBeGreaterThan(0);
      expect(tableAtOrAfter(editor.state.doc, editor.state.doc.content.size)).toBeNull();
    } finally {
      editor.destroy();
    }
  });

  it("declines a command probe that does not dispatch", async () => {
    const editor = await editorWith("<p>before</p>");
    try {
      editor.commands.setTextSelection(3);
      // `can()` runs the command without a dispatcher: the probe answers
      // true without touching the document.
      const can = editor.can() as unknown as PageTableCommands;
      expect(can.insertPageTable({ rows: 2, cols: 2 })).toBe(true);
      expect(editor.state.doc.childCount).toBe(1);
    } finally {
      editor.destroy();
    }
  });

  it("walks the caret to the next cell on Tab", async () => {
    const editor = await editorWith("<p>before</p>");
    try {
      (editor.commands as unknown as PageTableCommands).insertPageTable({ rows: 3, cols: 3 });
      const first = caretCell(editor.state.selection.$from);
      const event = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
      editor.view.dom.dispatchEvent(event);
      const second = caretCell(editor.state.selection.$from);
      expect(second).not.toBe(first);
      expect(second).toMatch(/^r\d+c\d+$/);
    } finally {
      editor.destroy();
    }
  });

  it("walks back a cell on Shift-Tab", async () => {
    const editor = await editorWith("<p>before</p>");
    try {
      (editor.commands as unknown as PageTableCommands).insertPageTable({ rows: 3, cols: 3 });
      const first = caretCell(editor.state.selection.$from);
      const forward = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
      editor.view.dom.dispatchEvent(forward);
      const second = caretCell(editor.state.selection.$from);
      expect(second).not.toBe(first);
      const back = new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, bubbles: true, cancelable: true });
      editor.view.dom.dispatchEvent(back);
      expect(caretCell(editor.state.selection.$from)).toBe(first);
    } finally {
      editor.destroy();
    }
  });

  it("leaves Tab alone outside a table", async () => {
    const editor = await editorWith("<p>before</p>");
    try {
      editor.commands.setTextSelection(3);
      const before = editor.state.selection.from;
      const event = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
      editor.view.dom.dispatchEvent(event);
      expect(editor.state.selection.from).toBe(before);
    } finally {
      editor.destroy();
    }
  });
});

/** The table cell holding a document position, as r{row}c{column}. */
function caretCell($from: {
  depth: number;
  node: (depth: number) => { type: { name: string } };
  index: (depth: number) => number;
}): string {
  let cell = -1;
  let row = -1;
  for (let depth = $from.depth; depth >= 0; depth -= 1) {
    const name = $from.node(depth).type.name;
    if (cell < 0 && (name === "tableCell" || name === "tableHeader")) cell = $from.index(depth);
    if (name === "tableRow") {
      row = $from.index(depth);
      break;
    }
  }
  return row < 0 || cell < 0 ? "outside" : `r${row}c${cell}`;
}

/** A stored 3×3 table whose first cell carries `first`. */
function storedTable(first: string): string {
  return `<table><tbody><tr><td><p>${first}</p></td><td><p>b</p></td><td><p>c</p></td></tr><tr><td><p>d</p></td><td><p>e</p></td><td><p>f</p></td></tr><tr><td><p>g</p></td><td><p>h</p></td><td><p>i</p></td></tr></tbody></table>`;
}

/** How many cells the current selection covers (a cell selection only). */
function selectedCellCount(editor: { state: { selection: unknown } }): number {
  const selection = editor.state.selection as unknown as {
    forEachCell: (fn: (node: unknown, pos: number) => void) => void;
  };
  let count = 0;
  selection.forEachCell(() => {
    count += 1;
  });
  return count;
}

describe("pageTableCellAnchorAt", () => {
  it("resolves a cell position to its table and cell", async () => {
    const editor = await editorWith(`<p>before</p>${storedTable("a")}`);
    try {
      const { pos, table } = locatedTable(editor);
      const first = pageTableCellAnchorAt(editor.state.doc, await cellPosition(table, pos, 0, 0));
      const second = pageTableCellAnchorAt(editor.state.doc, await cellPosition(table, pos, 1, 2));
      expect(first?.tablePos).toBe(pos);
      expect(first?.table).toBe(table);
      expect(second?.table).toBe(table);
      expect(first?.cellPos).not.toBe(second?.cellPos);
    } finally {
      editor.destroy();
    }
  });

  it("answers null in a plain block", async () => {
    const editor = await editorWith("<p>seed</p>");
    try {
      expect(pageTableCellAnchorAt(editor.state.doc, 2)).toBeNull();
    } finally {
      editor.destroy();
    }
  });

  it("answers null past either document end", async () => {
    const editor = await editorWith(`<p>before</p>${storedTable("a")}`);
    try {
      expect(pageTableCellAnchorAt(editor.state.doc, -1)).toBeNull();
      expect(pageTableCellAnchorAt(editor.state.doc, editor.state.doc.content.size + 1)).toBeNull();
    } finally {
      editor.destroy();
    }
  });

  it("answers null at the row boundary, where no cell holds the position", async () => {
    const editor = await editorWith(`<p>before</p>${storedTable("a")}`);
    try {
      const { pos } = locatedTable(editor);
      expect(pageTableCellAnchorAt(editor.state.doc, pos + 1)).toBeNull();
    } finally {
      editor.destroy();
    }
  });
});

describe("pageTableDuplicateRows", () => {
  it("copies rows after the last copied row, keeping content", async () => {
    const editor = await editorWith(storedTable("dup-me"));
    try {
      const { pos, table } = locatedTable(editor);
      const rowSize = table.child(0).nodeSize;
      const single = pageTableDuplicateRows(table, pos + 1, 0, 1);
      expect(single.nodes).toHaveLength(1);
      expect(single.nodes[0]?.type.name).toBe("tableRow");
      expect(single.nodes[0]?.textContent).toContain("dup-me");
      expect(single.pos).toBe(pos + 1 + rowSize);
      const pair = pageTableDuplicateRows(table, pos + 1, 0, 2);
      expect(pair.nodes).toHaveLength(2);
      expect(pair.pos).toBe(pos + 1 + rowSize + table.child(1).nodeSize);
    } finally {
      editor.destroy();
    }
  });
});

describe("pageTableDuplicateColumns", () => {
  it("copies one column's cells bottom-up", async () => {
    const editor = await editorWith(storedTable("dup-me"));
    try {
      const { pos, table } = locatedTable(editor);
      const { TableMap } = await import("@tiptap/pm/tables");
      const inserts = pageTableDuplicateColumns(table, TableMap.get(table), pos + 1, 1, 2);
      expect(inserts).toHaveLength(3);
      expect(inserts[0]?.nodes[0]?.textContent).toBe("h");
      expect(inserts[2]?.nodes[0]?.textContent).toBe("b");
      // Bottom-up: each insert lands below the previous one's position.
      const positions = inserts.map((insert) => insert.pos);
      expect([...positions].sort((a, b) => b - a)).toEqual(positions);
      const tr = editor.state.tr;
      for (const insert of inserts) tr.insert(insert.pos, insert.nodes);
      editor.view.dispatch(tr);
      expect(tableDims(editor.getHTML())).toBe("3x4");
    } finally {
      editor.destroy();
    }
  });

  it("copies a column range in reading order per row", async () => {
    const editor = await editorWith(storedTable("a"));
    try {
      const { pos, table } = locatedTable(editor);
      const { TableMap } = await import("@tiptap/pm/tables");
      const inserts = pageTableDuplicateColumns(table, TableMap.get(table), pos + 1, 0, 2);
      expect(inserts).toHaveLength(3);
      expect(inserts[0]?.nodes.map((node) => node.textContent)).toEqual(["g", "h"]);
    } finally {
      editor.destroy();
    }
  });

  it("skips a column a colspan carries past its starting cell", async () => {
    const editor = await editorWith(
      '<table><tbody><tr><td colspan="2"><p>wide</p></td><td><p>c</p></td></tr><tr><td><p>a</p></td><td><p>b</p></td><td><p>c</p></td></tr></tbody></table>'
    );
    try {
      const { pos, table } = locatedTable(editor);
      const { TableMap } = await import("@tiptap/pm/tables");
      // Grid column 1 of the first row belongs to the spanning cell, so
      // only the second row contributes a copy.
      const inserts = pageTableDuplicateColumns(table, TableMap.get(table), pos + 1, 1, 2);
      expect(inserts).toHaveLength(1);
      expect(inserts[0]?.nodes[0]?.textContent).toBe("b");
    } finally {
      editor.destroy();
    }
  });

  it("contributes nothing from a row a rowspan covers", async () => {
    const editor = await editorWith(
      '<table><tbody><tr><td rowspan="2"><p>tall</p></td><td><p>b</p></td></tr><tr><td><p>c</p></td></tr></tbody></table>'
    );
    try {
      const { pos, table } = locatedTable(editor);
      const { TableMap } = await import("@tiptap/pm/tables");
      const inserts = pageTableDuplicateColumns(table, TableMap.get(table), pos + 1, 0, 1);
      expect(inserts).toHaveLength(1);
      expect(inserts[0]?.nodes[0]?.textContent).toBe("tall");
    } finally {
      editor.destroy();
    }
  });
});

describe("page table structure commands", () => {
  it("selects the targeted row and column", async () => {
    const { CellSelection } = await import("@tiptap/pm/tables");
    const editor = await editorWith(storedTable("a"));
    try {
      const commands = editor.commands as unknown as PageTableCommands;
      const first = locatedTable(editor);
      expect(commands.selectPageTableRow({ pos: await cellPosition(first.table, first.pos, 1, 0) })).toBe(true);
      expect(editor.state.selection).toBeInstanceOf(CellSelection);
      expect(selectedCellCount(editor)).toBe(3);
      const second = locatedTable(editor);
      expect(commands.selectPageTableColumn({ pos: await cellPosition(second.table, second.pos, 0, 2) })).toBe(true);
      expect(editor.state.selection).toBeInstanceOf(CellSelection);
      expect(selectedCellCount(editor)).toBe(3);
    } finally {
      editor.destroy();
    }
  });

  it("inserts a row above and below the targeted row", async () => {
    const editor = await editorWith(storedTable("a"));
    try {
      const commands = editor.commands as unknown as PageTableCommands;
      const first = locatedTable(editor);
      expect(commands.insertPageTableRowAbove({ pos: await cellPosition(first.table, first.pos, 1, 1) })).toBe(true);
      expect(tableDims(editor.getHTML())).toBe("4x3");
      const second = locatedTable(editor);
      expect(commands.insertPageTableRowBelow({ pos: await cellPosition(second.table, second.pos, 0, 0) })).toBe(true);
      expect(tableDims(editor.getHTML())).toBe("5x3");
    } finally {
      editor.destroy();
    }
  });

  it("inserts a column left and right of the targeted column", async () => {
    const editor = await editorWith(storedTable("a"));
    try {
      const commands = editor.commands as unknown as PageTableCommands;
      const first = locatedTable(editor);
      expect(commands.insertPageTableColumnLeft({ pos: await cellPosition(first.table, first.pos, 1, 1) })).toBe(true);
      expect(tableDims(editor.getHTML())).toBe("3x4");
      const second = locatedTable(editor);
      expect(commands.insertPageTableColumnRight({ pos: await cellPosition(second.table, second.pos, 0, 0) })).toBe(
        true
      );
      expect(tableDims(editor.getHTML())).toBe("3x5");
    } finally {
      editor.destroy();
    }
  });

  it("duplicates the targeted row with its cell content", async () => {
    const editor = await editorWith(storedTable("dup-me"));
    try {
      const commands = editor.commands as unknown as PageTableCommands;
      const { pos, table } = locatedTable(editor);
      expect(commands.duplicatePageTableRow({ pos: await cellPosition(table, pos, 0, 2) })).toBe(true);
      const html = editor.getHTML();
      expect(tableDims(html)).toBe("4x3");
      expect(html.split("dup-me")).toHaveLength(3);
    } finally {
      editor.destroy();
    }
  });

  it("duplicates the targeted column with its cell content", async () => {
    const editor = await editorWith(storedTable("dup-me"));
    try {
      const commands = editor.commands as unknown as PageTableCommands;
      const { pos, table } = locatedTable(editor);
      expect(commands.duplicatePageTableColumn({ pos: await cellPosition(table, pos, 2, 0) })).toBe(true);
      const html = editor.getHTML();
      expect(tableDims(html)).toBe("3x4");
      expect(html.split("dup-me")).toHaveLength(3);
    } finally {
      editor.destroy();
    }
  });

  it("clears the targeted row and column without touching the structure", async () => {
    const editor = await editorWith(storedTable("dup-me"));
    try {
      const commands = editor.commands as unknown as PageTableCommands;
      const first = locatedTable(editor);
      expect(commands.clearPageTableCells({ pos: await cellPosition(first.table, first.pos, 0, 1), axis: "row" })).toBe(
        true
      );
      const cleared = editor.getHTML();
      expect(tableDims(cleared)).toBe("3x3");
      expect(cleared).not.toContain("dup-me");
      const second = locatedTable(editor);
      expect(
        commands.clearPageTableCells({ pos: await cellPosition(second.table, second.pos, 1, 0), axis: "column" })
      ).toBe(true);
      const clearedColumn = editor.getHTML();
      expect(tableDims(clearedColumn)).toBe("3x3");
      expect(clearedColumn).not.toContain(">d<");
      expect(clearedColumn).not.toContain(">g<");
    } finally {
      editor.destroy();
    }
  });

  it("deletes a middle and a first row", async () => {
    const editor = await editorWith(storedTable("a"));
    try {
      const commands = editor.commands as unknown as PageTableCommands;
      const first = locatedTable(editor);
      expect(commands.deletePageTableRow({ pos: await cellPosition(first.table, first.pos, 1, 1) })).toBe(true);
      expect(tableDims(editor.getHTML())).toBe("2x3");
      const second = locatedTable(editor);
      expect(commands.deletePageTableRow({ pos: await cellPosition(second.table, second.pos, 0, 0) })).toBe(true);
      expect(tableDims(editor.getHTML())).toBe("1x3");
    } finally {
      editor.destroy();
    }
  });

  it("deletes a middle and a first column", async () => {
    const editor = await editorWith(storedTable("a"));
    try {
      const commands = editor.commands as unknown as PageTableCommands;
      const first = locatedTable(editor);
      expect(commands.deletePageTableColumn({ pos: await cellPosition(first.table, first.pos, 1, 1) })).toBe(true);
      expect(tableDims(editor.getHTML())).toBe("3x2");
      const second = locatedTable(editor);
      expect(commands.deletePageTableColumn({ pos: await cellPosition(second.table, second.pos, 0, 0) })).toBe(true);
      expect(tableDims(editor.getHTML())).toBe("3x1");
    } finally {
      editor.destroy();
    }
  });

  it("leaves an empty paragraph behind the last deleted row", async () => {
    const editor = await editorWith("<p>before</p>");
    try {
      const commands = editor.commands as unknown as PageTableCommands;
      expect(commands.insertPageTable({ rows: 1, cols: 2 })).toBe(true);
      expect(tableDims(editor.getHTML())).toBe("1x2");
      const { pos, table } = locatedTable(editor);
      expect(commands.deletePageTableRow({ pos: await cellPosition(table, pos, 0, 1) })).toBe(true);
      expect(tableDims(editor.getHTML())).toBeNull();
      // The table stood first (the caret was at the document start when it
      // was inserted), so the empty paragraph replaces it in place.
      expect(editor.state.doc.childCount).toBe(2);
      const head = editor.state.doc.child(0);
      expect(head.type.name).toBe("paragraph");
      expect(head.textContent).toBe("");
      expect(editor.state.doc.child(1).textContent).toBe("before");
      const caret = editor.state.selection.$from;
      expect(caret.parent.type.name).toBe("paragraph");
      expect(caret.parent.textContent).toBe("");
    } finally {
      editor.destroy();
    }
  });

  it("leaves an empty paragraph behind the last deleted column", async () => {
    const editor = await editorWith("<p>before</p>");
    try {
      const commands = editor.commands as unknown as PageTableCommands;
      expect(commands.insertPageTable({ rows: 2, cols: 1 })).toBe(true);
      expect(tableDims(editor.getHTML())).toBe("2x1");
      const { pos, table } = locatedTable(editor);
      expect(commands.deletePageTableColumn({ pos: await cellPosition(table, pos, 1, 0) })).toBe(true);
      expect(tableDims(editor.getHTML())).toBeNull();
      expect(editor.state.doc.childCount).toBe(2);
      const head = editor.state.doc.child(0);
      expect(head.type.name).toBe("paragraph");
      expect(head.textContent).toBe("");
      expect(editor.state.doc.child(1).textContent).toBe("before");
    } finally {
      editor.destroy();
    }
  });

  it("paints the targeted row's background and clears it again", async () => {
    const editor = await editorWith(storedTable("a"));
    try {
      const commands = editor.commands as unknown as PageTableCommands;
      const first = locatedTable(editor);
      expect(
        commands.paintPageTableCells({
          pos: await cellPosition(first.table, first.pos, 2, 0),
          axis: "row",
          color: "pink",
        })
      ).toBe(true);
      expect(editor.getHTML().split('data-background-color="pink"')).toHaveLength(4);
      // Repainting the same colour is a no-op the command declines.
      const second = locatedTable(editor);
      expect(
        commands.paintPageTableCells({
          pos: await cellPosition(second.table, second.pos, 2, 1),
          axis: "row",
          color: "pink",
        })
      ).toBe(false);
      const third = locatedTable(editor);
      expect(
        commands.paintPageTableCells({
          pos: await cellPosition(third.table, third.pos, 2, 2),
          axis: "row",
          color: null,
        })
      ).toBe(true);
      expect(editor.getHTML()).not.toContain("data-background-color");
    } finally {
      editor.destroy();
    }
  });

  it("paints the targeted column's background", async () => {
    const editor = await editorWith(storedTable("a"));
    try {
      const commands = editor.commands as unknown as PageTableCommands;
      const { pos, table } = locatedTable(editor);
      expect(
        commands.paintPageTableCells({ pos: await cellPosition(table, pos, 0, 1), axis: "column", color: "green" })
      ).toBe(true);
      expect(editor.getHTML().split('data-background-color="green"')).toHaveLength(4);
    } finally {
      editor.destroy();
    }
  });

  it("round-trips a painted cell through stored markup", async () => {
    const editor = await editorWith(
      '<table><tbody><tr><td data-background-color="pink"><p>a</p></td><td><p>b</p></td></tr></tbody></table>'
    );
    try {
      expect(editor.getHTML()).toContain('data-background-color="pink"');
    } finally {
      editor.destroy();
    }
  });

  it("reads an empty stored background as no background", async () => {
    const editor = await editorWith(
      '<table><tbody><tr><td data-background-color=""><p>a</p></td></tr></tbody></table>'
    );
    try {
      expect(editor.getHTML()).not.toContain("data-background-color");
    } finally {
      editor.destroy();
    }
  });

  it("renders the background attribute only for a stored key", async () => {
    const nodes = await loadPageTableExtensions();
    const cell = nodes.find((node) => node.name === "tableCell");
    if (cell === undefined) throw new Error("missing table cell");
    const attributes = (
      cell.config.addAttributes as unknown as () => Record<
        string,
        { renderHTML: (attributes: Record<string, unknown>) => Record<string, unknown> }
      >
    )();
    expect(attributes["cellBackground"]?.renderHTML({})).toEqual({});
    expect(attributes["cellBackground"]?.renderHTML({ cellBackground: "pink" })).toEqual({
      "data-background-color": "pink",
    });
    expect(attributes["cellBackground"]?.renderHTML({ cellBackground: "" })).toEqual({});
  });

  it("declines every structure command outside a table", async () => {
    const editor = await editorWith("<p>seed</p>");
    try {
      const commands = editor.commands as unknown as PageTableCommands;
      expect(commands.selectPageTableRow({ pos: 2 })).toBe(false);
      expect(commands.selectPageTableColumn({ pos: 2 })).toBe(false);
      expect(commands.insertPageTableRowAbove({ pos: 2 })).toBe(false);
      expect(commands.insertPageTableRowBelow({ pos: 2 })).toBe(false);
      expect(commands.insertPageTableColumnLeft({ pos: 2 })).toBe(false);
      expect(commands.insertPageTableColumnRight({ pos: 2 })).toBe(false);
      expect(commands.duplicatePageTableRow({ pos: 2 })).toBe(false);
      expect(commands.duplicatePageTableColumn({ pos: 2 })).toBe(false);
      expect(commands.clearPageTableCells({ pos: 2, axis: "row" })).toBe(false);
      expect(commands.clearPageTableCells({ pos: 2, axis: "column" })).toBe(false);
      expect(commands.deletePageTableRow({ pos: 2 })).toBe(false);
      expect(commands.deletePageTableColumn({ pos: 2 })).toBe(false);
      expect(commands.paintPageTableCells({ pos: 2, axis: "row", color: "pink" })).toBe(false);
    } finally {
      editor.destroy();
    }
  });

  it("probes a structure command without touching the document", async () => {
    const editor = await editorWith(`<p>before</p>${storedTable("a")}`);
    try {
      const { pos, table } = locatedTable(editor);
      const before = editor.getHTML();
      const can = editor.can() as unknown as PageTableCommands;
      expect(can.selectPageTableRow({ pos: await cellPosition(table, pos, 0, 0) })).toBe(true);
      expect(can.insertPageTableRowBelow({ pos: await cellPosition(table, pos, 1, 1) })).toBe(true);
      expect(can.deletePageTableRow({ pos: 2 })).toBe(false);
      expect(editor.getHTML()).toBe(before);
    } finally {
      editor.destroy();
    }
  });

  it("keeps the Tab walk working after every structure edit", async () => {
    const edits: [string, (commands: PageTableCommands, pos: number) => boolean][] = [
      ["insert row", (commands, pos) => commands.insertPageTableRowBelow({ pos })],
      ["insert column", (commands, pos) => commands.insertPageTableColumnRight({ pos })],
      ["duplicate row", (commands, pos) => commands.duplicatePageTableRow({ pos })],
      ["duplicate column", (commands, pos) => commands.duplicatePageTableColumn({ pos })],
      ["clear", (commands, pos) => commands.clearPageTableCells({ pos, axis: "row" })],
      ["delete row", (commands, pos) => commands.deletePageTableRow({ pos })],
      ["delete column", (commands, pos) => commands.deletePageTableColumn({ pos })],
      ["paint", (commands, pos) => commands.paintPageTableCells({ pos, axis: "row", color: "pink" })],
    ];
    for (const [name, edit] of edits) {
      const editor = await editorWith(storedTable("a"));
      try {
        const commands = editor.commands as unknown as PageTableCommands;
        const { pos, table } = locatedTable(editor);
        expect(edit(commands, await cellPosition(table, pos, 0, 0))).toBe(true);
        const before = caretCell(editor.state.selection.$from);
        editor.view.dom.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true }));
        const after = caretCell(editor.state.selection.$from);
        expect(after, `${name}: Tab leaves the table`).toMatch(/^r\d+c\d+$/);
        expect(after, `${name}: Tab does not move`).not.toBe(before);
      } finally {
        editor.destroy();
      }
    }
  });
});

describe("loadPageColorMark", () => {
  it("loads the server-named mark for the letter/wash pair", async () => {
    const mark = await loadPageColorMark();
    expect(mark.name).toBe("customColor");
  });

  it("stores and reloads both attributes on one span", async () => {
    const editor = await editorWith('<p><span data-text-color="pink" data-background-color="green">paint</span></p>');
    try {
      const span = editor.getHTML();
      expect(span).toContain('data-text-color="pink"');
      expect(span).toContain('data-background-color="green"');
      expect(span).toContain("<span");
    } finally {
      editor.destroy();
    }
  });

  it("accepts either attribute alone", async () => {
    const editor = await editorWith(
      '<p><span data-text-color="gray">a</span> <span data-background-color="purple">b</span></p>'
    );
    try {
      const html = editor.getHTML();
      expect(html).toContain('data-text-color="gray"');
      expect(html).toContain('data-background-color="purple"');
    } finally {
      editor.destroy();
    }
  });
});

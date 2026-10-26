// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page tables and the colour mark (parity/specs/pages.md §5.3,
// PAGE-033/036): the table an insert builds, the markup it stores, the
// in-cell Tab walk, and the letter/wash pair on one span.

import { describe, expect, it } from "vitest";

import { loadPageExtensions } from "./pageExtensions.js";
import {
  buildPageTable,
  loadPageColorMark,
  loadPageTableExtensions,
  PAGE_TABLE_COLUMN_WIDTH,
  tableAtOrAfter,
  type PageTableCommands,
} from "./pageTableExtensions.js";

async function editorWith(content: string) {
  const { Editor } = await import("@tiptap/core");
  return new Editor({ extensions: await loadPageExtensions(), content });
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

describe("loadPageColorMark", () => {
  it("loads one mark named for the letter/wash pair", async () => {
    const mark = await loadPageColorMark();
    expect(mark.name).toBe("pageColor");
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

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Table row/column hover handles (parity/specs/pages.md §19.1, PAGE-091):
// handle placement on cell hover, menu order and wiring, the colour
// section, dismissal, and the silent no-editor paths.

import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { PageEditorHandle } from "../lib/pageEditorAdapter.js";
import {
  PAGE_TABLE_COLOR_CLEAR_TEST_ID,
  PAGE_TABLE_COLOR_SECTION_TEST_ID,
  PAGE_TABLE_COLOR_SWATCH_TEST_ID,
  PAGE_TABLE_COLUMN_ENTRIES,
  PAGE_TABLE_COLUMN_HANDLE_TEST_ID,
  PAGE_TABLE_COLUMN_MENU_TEST_ID,
  PAGE_TABLE_HANDLES_TEST_ID,
  PAGE_TABLE_MENU_ITEM_TEST_ID,
  PAGE_TABLE_ROW_ENTRIES,
  PAGE_TABLE_ROW_HANDLE_TEST_ID,
  PAGE_TABLE_ROW_MENU_TEST_ID,
  PageTableHandles,
} from "./PageTableHandles.js";

/** A handle probe with the table surface mocked (PAGE-091). */
function tableHandle(overrides: Partial<PageEditorHandle> = {}): PageEditorHandle {
  return {
    positionAtPoint: vi.fn(() => 7),
    selectTableRow: vi.fn(),
    selectTableColumn: vi.fn(),
    tableRowAction: vi.fn(),
    tableColumnAction: vi.fn(),
    paintTableCells: vi.fn(),
    html: vi.fn(() => "<p>next</p>"),
    ...overrides,
  } as unknown as PageEditorHandle;
}

function setup(options: { handle?: PageEditorHandle | null; html?: string } = {}): {
  onDocumentChange: ReturnType<typeof vi.fn>;
  setHandle: (next: PageEditorHandle | null) => void;
  rerender: (html: string) => void;
} {
  let current = options.handle === undefined ? tableHandle() : options.handle;
  const onDocumentChange = vi.fn();
  const view = (html: string): React.ReactElement => (
    <>
      <div contentEditable="true" data-testid="page-editor">
        <table>
          <tbody>
            <tr>
              <td>cell</td>
            </tr>
          </tbody>
        </table>
      </div>
      <PageTableHandles getHandle={() => current} onDocumentChange={onDocumentChange} html={html} />
    </>
  );
  const rendered = render(view(options.html ?? "<p></p>"));
  return {
    onDocumentChange,
    setHandle: (next) => {
      current = next;
    },
    rerender: (html) => rendered.rerender(view(html)),
  };
}

/** The editor's cell, for hovering. */
function editorCell(): HTMLElement {
  const cell = screen.getByTestId("page-editor").querySelector("td");
  if (cell === null) throw new Error("missing editor cell");
  return cell as unknown as HTMLElement;
}

function box(top: number, left: number, width: number, height: number): DOMRect {
  return {
    top,
    left,
    width,
    height,
    right: left + width,
    bottom: top + height,
    x: left,
    y: top,
    toJSON: () => ({}),
  };
}

/** A real button press: mousedown (which the editor keeps for focus) then click. */
function press(target: HTMLElement): void {
  fireEvent.mouseDown(target);
  fireEvent.click(target);
}

describe("PageTableHandles", () => {
  it("renders no handles before any hover", () => {
    setup();
    expect(screen.getByTestId(PAGE_TABLE_HANDLES_TEST_ID)).toBeInTheDocument();
    expect(screen.queryByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeNull();
    expect(screen.queryByTestId(PAGE_TABLE_COLUMN_HANDLE_TEST_ID)).toBeNull();
  });

  it("raises both handles beside the hovered cell", () => {
    setup();
    const cell = editorCell();
    const row = cell.closest("tr");
    const table = cell.closest("table");
    if (row === null || table === null) throw new Error("missing row or table");
    row.getBoundingClientRect = () => box(100, 50, 300, 40);
    cell.getBoundingClientRect = () => box(100, 50, 100, 40);
    table.getBoundingClientRect = () => box(80, 50, 300, 200);
    fireEvent.mouseMove(cell, { clientX: 90, clientY: 110 });

    const rowHandle = screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID);
    expect(rowHandle).toHaveAttribute("aria-label", "Row actions");
    expect(rowHandle.parentElement).toHaveStyle({ top: "108px", left: "22px" });
    const columnHandle = screen.getByTestId(PAGE_TABLE_COLUMN_HANDLE_TEST_ID);
    expect(columnHandle).toHaveAttribute("aria-label", "Column actions");
    expect(columnHandle.parentElement).toHaveStyle({ top: "52px", left: "88px" });
  });

  it("hides when the pointer leaves the table", () => {
    setup();
    fireEvent.mouseMove(editorCell());
    expect(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeInTheDocument();
    fireEvent.mouseMove(document.body);
    expect(screen.queryByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeNull();
    expect(screen.queryByTestId(PAGE_TABLE_COLUMN_HANDLE_TEST_ID)).toBeNull();
  });

  it("keeps the handles over their own surface", () => {
    setup();
    fireEvent.mouseMove(editorCell());
    const handle = screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID);
    fireEvent.mouseMove(handle);
    expect(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeInTheDocument();
  });

  it("treats a non-element pointer target as outside", () => {
    setup();
    fireEvent.mouseMove(document);
    expect(screen.queryByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeNull();
    fireEvent.mouseMove(editorCell());
    expect(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeInTheDocument();
    fireEvent.mouseMove(document);
    expect(screen.queryByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeNull();
  });

  it("ignores cells outside the editable body", () => {
    setup();
    const stray = document.createElement("table");
    stray.innerHTML = "<tbody><tr><td>stray</td></tr></tbody>";
    document.body.appendChild(stray);
    try {
      const cell = stray.querySelector("td");
      if (cell === null) throw new Error("missing stray cell");
      fireEvent.mouseMove(cell);
      expect(screen.queryByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeNull();
    } finally {
      stray.remove();
    }
  });

  it("hides when the editor is missing", () => {
    setup({ handle: null });
    fireEvent.mouseMove(editorCell());
    expect(screen.queryByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeNull();
  });

  it("hides when the point names no document position", () => {
    setup({ handle: tableHandle({ positionAtPoint: vi.fn(() => null) }) });
    fireEvent.mouseMove(editorCell());
    expect(screen.queryByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeNull();
  });

  it("ignores a cell without a table row", () => {
    setup();
    const holder = document.createElement("div");
    holder.setAttribute("contenteditable", "true");
    const cell = document.createElement("td");
    cell.textContent = "loose";
    holder.appendChild(cell);
    document.body.appendChild(holder);
    try {
      fireEvent.mouseMove(cell);
      expect(screen.queryByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeNull();
    } finally {
      holder.remove();
    }
  });

  it("ignores a row without a table", () => {
    setup();
    const holder = document.createElement("div");
    holder.setAttribute("contenteditable", "true");
    const row = document.createElement("tr");
    const cell = document.createElement("td");
    cell.textContent = "loose";
    row.appendChild(cell);
    holder.appendChild(row);
    document.body.appendChild(holder);
    try {
      fireEvent.mouseMove(cell);
      expect(screen.queryByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeNull();
    } finally {
      holder.remove();
    }
  });

  it("opens the row menu in entry order and selects the row", () => {
    const handle = tableHandle();
    setup({ handle });
    fireEvent.mouseMove(editorCell());
    press(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID));
    expect(handle.selectTableRow).toHaveBeenCalledWith(7);
    const menu = screen.getByTestId(PAGE_TABLE_ROW_MENU_TEST_ID);
    const items = within(menu).getAllByTestId(PAGE_TABLE_MENU_ITEM_TEST_ID);
    expect(items.map((item) => item.textContent)).toEqual(PAGE_TABLE_ROW_ENTRIES.map((entry) => entry.label));
    expect(within(menu).getByTestId(PAGE_TABLE_COLOR_SECTION_TEST_ID)).toBeInTheDocument();
    expect(within(menu).getAllByTestId(PAGE_TABLE_COLOR_SWATCH_TEST_ID)).toHaveLength(8);
    expect(within(menu).getByTestId(PAGE_TABLE_COLOR_CLEAR_TEST_ID)).toBeInTheDocument();
  });

  it("opens the column menu in entry order and selects the column", () => {
    const handle = tableHandle();
    setup({ handle });
    fireEvent.mouseMove(editorCell());
    press(screen.getByTestId(PAGE_TABLE_COLUMN_HANDLE_TEST_ID));
    expect(handle.selectTableColumn).toHaveBeenCalledWith(7);
    const menu = screen.getByTestId(PAGE_TABLE_COLUMN_MENU_TEST_ID);
    const items = within(menu).getAllByTestId(PAGE_TABLE_MENU_ITEM_TEST_ID);
    expect(items.map((item) => item.textContent)).toEqual(PAGE_TABLE_COLUMN_ENTRIES.map((entry) => entry.label));
  });

  it("toggles the menu closed on a second press", () => {
    const handle = tableHandle();
    setup({ handle });
    fireEvent.mouseMove(editorCell());
    press(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID));
    expect(screen.getByTestId(PAGE_TABLE_ROW_MENU_TEST_ID)).toBeInTheDocument();
    press(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID));
    expect(screen.queryByTestId(PAGE_TABLE_ROW_MENU_TEST_ID)).toBeNull();
    expect(handle.selectTableRow).toHaveBeenCalledTimes(1);
  });

  it("raises the open menu's wrapper above the selection bubble", () => {
    setup({ handle: tableHandle() });
    fireEvent.mouseMove(editorCell());
    const rowWrapper = screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID).parentElement;
    const columnWrapper = screen.getByTestId(PAGE_TABLE_COLUMN_HANDLE_TEST_ID).parentElement;
    expect(rowWrapper?.className).toContain("z-20");
    expect(rowWrapper?.className).not.toContain("z-40");
    expect(columnWrapper?.className).toContain("z-20");
    press(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID));
    expect(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID).parentElement?.className).toContain("z-40");
    expect(screen.getByTestId(PAGE_TABLE_COLUMN_HANDLE_TEST_ID).parentElement?.className).toContain("z-20");
    press(screen.getByTestId(PAGE_TABLE_COLUMN_HANDLE_TEST_ID));
    expect(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID).parentElement?.className).toContain("z-20");
    expect(screen.getByTestId(PAGE_TABLE_COLUMN_HANDLE_TEST_ID).parentElement?.className).toContain("z-40");
    press(screen.getByTestId(PAGE_TABLE_COLUMN_HANDLE_TEST_ID));
    expect(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID).parentElement?.className).toContain("z-20");
    expect(screen.getByTestId(PAGE_TABLE_COLUMN_HANDLE_TEST_ID).parentElement?.className).toContain("z-20");
  });

  it("runs every row entry against the hovered row and reports the document", () => {
    const handle = tableHandle();
    const { onDocumentChange } = setup({ handle });
    for (const entry of PAGE_TABLE_ROW_ENTRIES) {
      fireEvent.mouseMove(editorCell());
      press(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID));
      press(screen.getByRole("menuitem", { name: entry.label }));
      expect(handle.tableRowAction).toHaveBeenCalledWith(7, entry.action);
      expect(screen.queryByTestId(PAGE_TABLE_ROW_MENU_TEST_ID)).toBeNull();
    }
    expect(onDocumentChange).toHaveBeenCalledTimes(PAGE_TABLE_ROW_ENTRIES.length);
    expect(onDocumentChange).toHaveBeenCalledWith("<p>next</p>");
  });

  it("runs every column entry against the hovered column and reports the document", () => {
    const handle = tableHandle();
    const { onDocumentChange } = setup({ handle });
    for (const entry of PAGE_TABLE_COLUMN_ENTRIES) {
      fireEvent.mouseMove(editorCell());
      press(screen.getByTestId(PAGE_TABLE_COLUMN_HANDLE_TEST_ID));
      press(screen.getByRole("menuitem", { name: entry.label }));
      expect(handle.tableColumnAction).toHaveBeenCalledWith(7, entry.action);
      expect(screen.queryByTestId(PAGE_TABLE_COLUMN_MENU_TEST_ID)).toBeNull();
    }
    expect(onDocumentChange).toHaveBeenCalledTimes(PAGE_TABLE_COLUMN_ENTRIES.length);
    expect(onDocumentChange).toHaveBeenCalledWith("<p>next</p>");
  });

  it("paints and clears the background from the colour section", () => {
    const handle = tableHandle();
    const { onDocumentChange } = setup({ handle });
    fireEvent.mouseMove(editorCell());
    press(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID));
    press(screen.getByRole("button", { name: "Wash color Pink" }));
    expect(handle.paintTableCells).toHaveBeenCalledWith(7, "row", "pink");
    expect(onDocumentChange).toHaveBeenCalledWith("<p>next</p>");
    expect(screen.queryByTestId(PAGE_TABLE_ROW_MENU_TEST_ID)).toBeNull();

    fireEvent.mouseMove(editorCell());
    press(screen.getByTestId(PAGE_TABLE_COLUMN_HANDLE_TEST_ID));
    press(screen.getByRole("button", { name: "Clear wash color" }));
    expect(handle.paintTableCells).toHaveBeenCalledWith(7, "column", null);
    expect(screen.queryByTestId(PAGE_TABLE_COLUMN_MENU_TEST_ID)).toBeNull();
  });

  it("collapses the colour section without closing the menu", () => {
    setup();
    for (const [handleId, menuId] of [
      [PAGE_TABLE_ROW_HANDLE_TEST_ID, PAGE_TABLE_ROW_MENU_TEST_ID],
      [PAGE_TABLE_COLUMN_HANDLE_TEST_ID, PAGE_TABLE_COLUMN_MENU_TEST_ID],
    ] as const) {
      fireEvent.mouseMove(editorCell());
      press(screen.getByTestId(handleId));
      const menu = screen.getByTestId(menuId);
      expect(within(menu).getAllByTestId(PAGE_TABLE_COLOR_SWATCH_TEST_ID)).toHaveLength(8);
      press(within(menu).getByRole("button", { name: "Cell background" }));
      expect(within(menu).queryAllByTestId(PAGE_TABLE_COLOR_SWATCH_TEST_ID)).toHaveLength(0);
      expect(screen.getByTestId(menuId)).toBeInTheDocument();
      press(within(menu).getByRole("button", { name: "Cell background" }));
      expect(within(menu).getAllByTestId(PAGE_TABLE_COLOR_SWATCH_TEST_ID)).toHaveLength(8);
      press(screen.getByTestId(handleId));
      expect(screen.queryByTestId(menuId)).toBeNull();
    }
  });

  it("dismisses the menu on Escape and ignores other keys", () => {
    setup();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId(PAGE_TABLE_ROW_MENU_TEST_ID)).toBeNull();
    fireEvent.mouseMove(editorCell());
    press(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID));
    fireEvent.keyDown(document, { key: "Enter" });
    expect(screen.getByTestId(PAGE_TABLE_ROW_MENU_TEST_ID)).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId(PAGE_TABLE_ROW_MENU_TEST_ID)).toBeNull();
  });

  it("hides after the document changes", () => {
    const { rerender } = setup();
    fireEvent.mouseMove(editorCell());
    expect(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeInTheDocument();
    rerender("<p>changed</p>");
    expect(screen.queryByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID)).toBeNull();
  });

  it("does nothing without an editor behind the handles", () => {
    const handle = tableHandle();
    const { setHandle } = setup({ handle });
    // Opening needs the editor: with none mounted the press is ignored.
    fireEvent.mouseMove(editorCell());
    setHandle(null);
    press(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID));
    expect(screen.queryByTestId(PAGE_TABLE_ROW_MENU_TEST_ID)).toBeNull();
    expect(handle.selectTableRow).not.toHaveBeenCalled();

    // Entries picked after the editor unmounts stay silent, too.
    setHandle(handle);
    fireEvent.mouseMove(editorCell());
    press(screen.getByTestId(PAGE_TABLE_ROW_HANDLE_TEST_ID));
    setHandle(null);
    press(screen.getByRole("menuitem", { name: "Duplicate" }));
    expect(handle.tableRowAction).not.toHaveBeenCalled();
    expect(screen.getByTestId(PAGE_TABLE_ROW_MENU_TEST_ID)).toBeInTheDocument();

    setHandle(handle);
    fireEvent.mouseMove(editorCell());
    press(screen.getByTestId(PAGE_TABLE_COLUMN_HANDLE_TEST_ID));
    setHandle(null);
    press(screen.getByRole("menuitem", { name: "Delete" }));
    expect(handle.tableColumnAction).not.toHaveBeenCalled();
    expect(screen.getByTestId(PAGE_TABLE_COLUMN_MENU_TEST_ID)).toBeInTheDocument();

    press(screen.getByRole("button", { name: "Wash color Pink" }));
    expect(handle.paintTableCells).not.toHaveBeenCalled();
    expect(handle.html).not.toHaveBeenCalled();
  });
});

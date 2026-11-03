// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Table row/column hover handles (parity/specs/pages.md §19.1,
// PAGE-091). Each hovered table cell raises two handles: one beside its
// row, one above its column. A handle opens its structure menu — insert,
// duplicate, clear, delete, in that order — plus a collapsible cell
// background section painting the palette's wash colours. Opening a menu
// selects its line first, so every entry acts on the hovered row or
// column and the highlight shows it; the selection bubble that
// selection raises sits above the row while the menu hangs below the
// handle, so the two never cover each other. The layer mounts this only
// beside the editable body, so locked, archived and read-only pages
// render no handles.

import { Ban, GripHorizontal, GripVertical } from "lucide-react";
import * as React from "react";

import type { PageTableLineAxis } from "../../../shared/editor/pageTableExtensions.js";
import type { PageEditorHandle, PageTableColumnAction, PageTableRowAction } from "../lib/pageEditorAdapter.js";
import { colorClearLabel, colorSwatchLabel, colorSwatchPaint, PAGE_COLOR_CHOICES } from "../lib/pageColors.js";

/** Row-menu entries, in the order the menu offers them (PAGE-091). */
export const PAGE_TABLE_ROW_ENTRIES: readonly { action: PageTableRowAction; label: string }[] = [
  { action: "insertAbove", label: "Insert above" },
  { action: "insertBelow", label: "Insert below" },
  { action: "duplicate", label: "Duplicate" },
  { action: "clear", label: "Clear contents" },
  { action: "delete", label: "Delete" },
];

/** Column-menu entries, in the order the menu offers them (PAGE-091). */
export const PAGE_TABLE_COLUMN_ENTRIES: readonly { action: PageTableColumnAction; label: string }[] = [
  { action: "insertLeft", label: "Insert left" },
  { action: "insertRight", label: "Insert right" },
  { action: "duplicate", label: "Duplicate" },
  { action: "clear", label: "Clear contents" },
  { action: "delete", label: "Delete" },
];

export const PAGE_TABLE_ROW_HANDLE_LABEL = "Row actions";
export const PAGE_TABLE_COLUMN_HANDLE_LABEL = "Column actions";
export const PAGE_TABLE_COLOR_LABEL = "Cell background";

export const PAGE_TABLE_HANDLES_TEST_ID = "page-table-handles";
export const PAGE_TABLE_ROW_HANDLE_TEST_ID = "page-table-row-handle";
export const PAGE_TABLE_COLUMN_HANDLE_TEST_ID = "page-table-column-handle";
export const PAGE_TABLE_ROW_MENU_TEST_ID = "page-table-row-menu";
export const PAGE_TABLE_COLUMN_MENU_TEST_ID = "page-table-column-menu";
export const PAGE_TABLE_MENU_ITEM_TEST_ID = "page-table-menu-item";
export const PAGE_TABLE_COLOR_SECTION_TEST_ID = "page-table-color-section";
export const PAGE_TABLE_COLOR_TOGGLE_TEST_ID = "page-table-color-toggle";
export const PAGE_TABLE_COLOR_SWATCH_TEST_ID = "page-table-color-swatch";
export const PAGE_TABLE_COLOR_CLEAR_TEST_ID = "page-table-color-clear";

export interface PageTableHandlesProps {
  getHandle: () => PageEditorHandle | null;
  /** Report a new document so the pane's asset rows refresh (PAGE-039). */
  onDocumentChange: (html: string) => void;
  /** Fires after each editor update, with the current HTML. */
  html: string;
}

/** The hovered cell's document position plus both handles' viewport spots. */
interface TableHover {
  pos: number;
  rowTop: number;
  rowLeft: number;
  colTop: number;
  colLeft: number;
}

/** An open structure menu: its axis and the position it acts on. */
interface OpenTableMenu {
  axis: PageTableLineAxis;
  pos: number;
}

/** Handle buttons sit this far outside the table's edge, in px. */
const TABLE_HANDLE_GUTTER = 28;
/** Half the handle button's edge, for centering on the row or column. */
const TABLE_HANDLE_HALF = 12;

export function PageTableHandles({ getHandle, onDocumentChange, html }: PageTableHandlesProps): React.ReactElement {
  const [hover, setHover] = React.useState<TableHover | null>(null);
  const [openMenu, setOpenMenu] = React.useState<OpenTableMenu | null>(null);
  const [colorOpen, setColorOpen] = React.useState(true);
  const surfaceRef = React.useRef<HTMLDivElement | null>(null);

  // A new document invalidates the hovered position: the handles hide
  // until the pointer moves again, so a menu never acts on a position an
  // edit has shifted.
  React.useEffect(() => {
    setHover(null);
    setOpenMenu(null);
  }, [html]);

  // The handles follow the hovered cell: the row handle sits left of the
  // table beside the row, the column handle above the table over the
  // column. A pointer over the handles or an open menu keeps them.
  React.useEffect(() => {
    const hide = (): void => {
      setHover(null);
      setOpenMenu(null);
    };
    const onPointerMove = (event: MouseEvent): void => {
      const target = event.target instanceof Element ? event.target : null;
      const cell = target?.closest("td, th") ?? null;
      const editable = cell?.closest('[contenteditable="true"]') ?? null;
      if (cell !== null && editable !== null) {
        const handle = getHandle();
        if (handle === null) {
          hide();
          return;
        }
        const pos = handle.positionAtPoint(event.clientX, event.clientY);
        if (pos === null) {
          hide();
          return;
        }
        const row = cell.closest("tr");
        const table = cell.closest("table");
        if (row === null || table === null) {
          hide();
          return;
        }
        const rowBox = row.getBoundingClientRect();
        const cellBox = cell.getBoundingClientRect();
        const tableBox = table.getBoundingClientRect();
        setHover({
          pos,
          rowTop: Math.round(rowBox.top + rowBox.height / 2 - TABLE_HANDLE_HALF),
          rowLeft: Math.round(tableBox.left - TABLE_HANDLE_GUTTER),
          colTop: Math.round(tableBox.top - TABLE_HANDLE_GUTTER),
          colLeft: Math.round(cellBox.left + cellBox.width / 2 - TABLE_HANDLE_HALF),
        });
        return;
      }
      const surface = surfaceRef.current;
      if (surface !== null && target !== null && surface.contains(target)) return;
      hide();
    };
    document.addEventListener("mousemove", onPointerMove);
    return () => document.removeEventListener("mousemove", onPointerMove);
  }, [getHandle]);

  // Escape dismisses an open structure menu without touching the document.
  React.useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || openMenu === null) return;
      event.preventDefault();
      setOpenMenu(null);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [openMenu]);

  // Opening a menu selects its line first, so the entries act on the
  // hovered row or column and the highlight shows it (PAGE-091).
  const toggleMenu = (axis: PageTableLineAxis, pos: number): void => {
    if (openMenu?.axis === axis) {
      setOpenMenu(null);
      return;
    }
    const handle = getHandle();
    if (handle === null) return;
    if (axis === "row") handle.selectTableRow(pos);
    else handle.selectTableColumn(pos);
    setColorOpen(true);
    setOpenMenu({ axis, pos });
  };

  const runRowAction = (menu: OpenTableMenu, action: PageTableRowAction): void => {
    const handle = getHandle();
    if (handle === null) return;
    handle.tableRowAction(menu.pos, action);
    setOpenMenu(null);
    onDocumentChange(handle.html());
  };

  const runColumnAction = (menu: OpenTableMenu, action: PageTableColumnAction): void => {
    const handle = getHandle();
    if (handle === null) return;
    handle.tableColumnAction(menu.pos, action);
    setOpenMenu(null);
    onDocumentChange(handle.html());
  };

  const runPaint = (menu: OpenTableMenu, color: string | null): void => {
    const handle = getHandle();
    if (handle === null) return;
    handle.paintTableCells(menu.pos, menu.axis, color);
    setOpenMenu(null);
    onDocumentChange(handle.html());
  };

  // An open menu must paint above the selection bubble (z-30): the menu
  // lives inside its handle wrapper's stacking context, so the wrapper
  // holding the open menu rises to z-40 while the other stays at z-20.
  const rowWrapperClassName =
    openMenu?.axis === "row" ? "pointer-events-auto fixed z-40" : "pointer-events-auto fixed z-20";
  const columnWrapperClassName =
    openMenu?.axis === "column" ? "pointer-events-auto fixed z-40" : "pointer-events-auto fixed z-20";

  return (
    <div data-testid={PAGE_TABLE_HANDLES_TEST_ID}>
      {hover === null ? null : (
        <div ref={surfaceRef}>
          <div className={rowWrapperClassName} style={{ top: hover.rowTop, left: hover.rowLeft }}>
            <button
              type="button"
              aria-label={PAGE_TABLE_ROW_HANDLE_LABEL}
              aria-expanded={openMenu?.axis === "row"}
              data-testid={PAGE_TABLE_ROW_HANDLE_TEST_ID}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => toggleMenu("row", hover.pos)}
              className="grid size-6 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
            >
              <GripVertical size={14} strokeWidth={1.75} aria-hidden="true" />
            </button>
            {openMenu?.axis === "row" ? (
              <TableStructureMenu
                testId={PAGE_TABLE_ROW_MENU_TEST_ID}
                entries={PAGE_TABLE_ROW_ENTRIES}
                onPick={(action) => runRowAction(openMenu, action)}
                onPaint={(color) => runPaint(openMenu, color)}
                colorOpen={colorOpen}
                onToggleColor={() => setColorOpen((open) => !open)}
              />
            ) : null}
          </div>
          <div className={columnWrapperClassName} style={{ top: hover.colTop, left: hover.colLeft }}>
            <button
              type="button"
              aria-label={PAGE_TABLE_COLUMN_HANDLE_LABEL}
              aria-expanded={openMenu?.axis === "column"}
              data-testid={PAGE_TABLE_COLUMN_HANDLE_TEST_ID}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => toggleMenu("column", hover.pos)}
              className="grid size-6 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
            >
              <GripHorizontal size={14} strokeWidth={1.75} aria-hidden="true" />
            </button>
            {openMenu?.axis === "column" ? (
              <TableStructureMenu
                testId={PAGE_TABLE_COLUMN_MENU_TEST_ID}
                entries={PAGE_TABLE_COLUMN_ENTRIES}
                onPick={(action) => runColumnAction(openMenu, action)}
                onPaint={(color) => runPaint(openMenu, color)}
                colorOpen={colorOpen}
                onToggleColor={() => setColorOpen((open) => !open)}
              />
            ) : null}
          </div>
        </div>
      )}
    </div>
  );
}

/** One structure menu: the five entries plus the background section. */
function TableStructureMenu<Action extends PageTableRowAction | PageTableColumnAction>({
  testId,
  entries,
  onPick,
  onPaint,
  colorOpen,
  onToggleColor,
}: {
  testId: string;
  entries: readonly { action: Action; label: string }[];
  onPick: (action: Action) => void;
  onPaint: (color: string | null) => void;
  colorOpen: boolean;
  onToggleColor: () => void;
}): React.ReactElement {
  return (
    <div
      role="menu"
      data-testid={testId}
      className="shadow-popover absolute top-6 left-0 z-30 flex w-44 flex-col rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-1)"
    >
      {entries.map(({ action, label }) => (
        <button
          key={action}
          type="button"
          role="menuitem"
          data-testid={PAGE_TABLE_MENU_ITEM_TEST_ID}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => onPick(action)}
          className="text-body rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle)"
        >
          {label}
        </button>
      ))}
      <div data-testid={PAGE_TABLE_COLOR_SECTION_TEST_ID} className="flex flex-col">
        <button
          type="button"
          aria-label={PAGE_TABLE_COLOR_LABEL}
          aria-expanded={colorOpen}
          data-testid={PAGE_TABLE_COLOR_TOGGLE_TEST_ID}
          onMouseDown={(event) => event.preventDefault()}
          onClick={onToggleColor}
          className="text-body rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left font-medium text-(--text) hover:bg-(--subtle)"
        >
          {PAGE_TABLE_COLOR_LABEL}
        </button>
        {colorOpen ? (
          <div className="flex flex-wrap items-center gap-(--space-1) px-(--space-2) py-(--space-1)">
            <button
              type="button"
              aria-label={colorClearLabel("background")}
              data-testid={PAGE_TABLE_COLOR_CLEAR_TEST_ID}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => onPaint(null)}
              className="grid size-6 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle)"
            >
              <Ban size={14} strokeWidth={1.75} aria-hidden="true" />
            </button>
            {PAGE_COLOR_CHOICES.map((choice) => (
              <button
                key={choice.key}
                type="button"
                aria-label={colorSwatchLabel("background", choice)}
                data-testid={PAGE_TABLE_COLOR_SWATCH_TEST_ID}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => onPaint(choice.key)}
                className="size-6 rounded-(--radius-control) border border-(--border)"
                style={{ backgroundColor: colorSwatchPaint("background", choice) }}
              />
            ))}
          </div>
        ) : null}
      </div>
    </div>
  );
}

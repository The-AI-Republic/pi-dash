// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page table nodes and the colour mark (parity/specs/pages.md §5.3,
// PAGE-033/036) plus the row/column structure editing the hover handles
// drive (§19.1, PAGE-091). Tiptap serves tables from `@tiptap/pm/tables`,
// which the page editor already carries through `@tiptap/pm`, so the
// page set gains tables without a new runtime package. Everything is
// built inside the lazy loaders (the boundary check allows ProseMirror
// only through import()), and each node serialises to the markup the
// API's content validator already accepts: `<table>/<tr>/<th>/<td>` for
// tables, a `<span>` carrying `data-text-color`/`data-background-color`
// for colour, and the same `data-background-color` palette key on a
// painted cell.

type Core = typeof import("@tiptap/core");
type MergeAttributes = Core["mergeAttributes"];
type CommandProps = import("@tiptap/core").CommandProps;
type RawCommands = import("@tiptap/core").RawCommands;
type PmModel = typeof import("@tiptap/pm/model");
type PmNode = InstanceType<PmModel["Node"]>;
type Schema = InstanceType<PmModel["Schema"]>;
type State = typeof import("@tiptap/pm/state");
type EditorState = InstanceType<State["EditorState"]>;
type Transaction = InstanceType<State["Transaction"]>;
type TextSelectionClass = State["TextSelection"];
type TablesModule = typeof import("@tiptap/pm/tables");
type TableMap = TablesModule["TableMap"];
type TableMapInstance = InstanceType<TableMap>;

/**
 * The tables package reads `tableRole` from the ProseMirror node spec, so
 * every table node publishes it through `extendNodeSchema`. That hook runs
 * for the whole extension list, so it reports the role only for its own
 * node; without it the map, cell walk and Tab shortcut decline the node.
 */
function tableRole(name: string, role: string) {
  return (node: { name: string }) => (node.name === name ? { tableRole: role } : {});
}

/**
 * The commands the table extension registers. Tiptap's command types are
 * a closed interface each extension augments; the page tree cannot import
 * `@tiptap/core` statically (the boundary check only allows it through
 * `import()`), so the two commands are named here and cast at the call
 * sites instead of widening the shared type.
 */
export interface PageTableCommands {
  insertPageTable: (attributes?: { rows?: number; cols?: number; withHeaderRow?: boolean }) => boolean;
  /** Select the row holding `pos` (PAGE-091 handle highlight). */
  selectPageTableRow: (attributes: { pos: number }) => boolean;
  /** Select the column holding `pos` (PAGE-091 handle highlight). */
  selectPageTableColumn: (attributes: { pos: number }) => boolean;
  /** Insert a row above/below the row holding `pos` (PAGE-091). */
  insertPageTableRowAbove: (attributes: { pos: number }) => boolean;
  insertPageTableRowBelow: (attributes: { pos: number }) => boolean;
  /** Insert a column left/right of the column holding `pos` (PAGE-091). */
  insertPageTableColumnLeft: (attributes: { pos: number }) => boolean;
  insertPageTableColumnRight: (attributes: { pos: number }) => boolean;
  /** Duplicate the row/column holding `pos` with its content (PAGE-091). */
  duplicatePageTableRow: (attributes: { pos: number }) => boolean;
  duplicatePageTableColumn: (attributes: { pos: number }) => boolean;
  /** Empty the cells of the row/column holding `pos` (PAGE-091). */
  clearPageTableCells: (attributes: { pos: number; axis: PageTableLineAxis }) => boolean;
  /** Delete the row/column holding `pos` (PAGE-091). */
  deletePageTableRow: (attributes: { pos: number }) => boolean;
  deletePageTableColumn: (attributes: { pos: number }) => boolean;
  /** Paint the row/column holding `pos` with a palette key, or clear it (PAGE-091). */
  paintPageTableCells: (attributes: { pos: number; axis: PageTableLineAxis; color: string | null }) => boolean;
}

/** The line a table structure command selects before it acts. */
export type PageTableLineAxis = "row" | "column";

/** Column width an inserted cell records, in CSS pixels. */
export const PAGE_TABLE_COLUMN_WIDTH = 150;

/**
 * The colour mark. One mark carries both attributes, so a ranged text can
 * hold a letter colour and a wash colour at once and clearing one leaves
 * the other in place (PAGE-036). The adapter reads the current pair and
 * writes both attributes back, so neither write erases the other. The
 * data attributes are the stored form; the CSS paints them from the
 * palette tokens. The mark and attribute names are the live server's own
 * (NEWFRONT-929): only those names survive the Yjs round trip the server
 * serializes back to HTML.
 */
function createPageColorMark(Mark: Core["Mark"], mergeAttributes: MergeAttributes) {
  return Mark.create({
    name: "customColor",
    addAttributes() {
      return {
        color: {
          default: null,
          parseHTML: (element: HTMLElement) => element.getAttribute("data-text-color"),
          renderHTML: (attributes: Record<string, unknown>) => {
            const value = attributes.color;
            return typeof value === "string" && value !== "" ? { "data-text-color": value } : {};
          },
        },
        backgroundColor: {
          default: null,
          parseHTML: (element: HTMLElement) => element.getAttribute("data-background-color"),
          renderHTML: (attributes: Record<string, unknown>) => {
            const value = attributes.backgroundColor;
            return typeof value === "string" && value !== "" ? { "data-background-color": value } : {};
          },
        },
      };
    },
    parseHTML() {
      return [{ tag: "span[data-text-color]" }, { tag: "span[data-background-color]" }];
    },
    renderHTML({ HTMLAttributes }: { HTMLAttributes: Record<string, unknown> }) {
      return ["span", mergeAttributes(HTMLAttributes), 0];
    },
  });
}

/** One cell's shared attributes: spans and the stored column widths. */
function cellAttributes() {
  return {
    colspan: {
      default: 1,
      parseHTML: (element: HTMLElement) => Number(element.getAttribute("colspan") ?? 1),
      renderHTML: (attributes: Record<string, unknown>) =>
        Number(attributes.colspan ?? 1) === 1 ? {} : { colspan: String(attributes.colspan) },
    },
    rowspan: {
      default: 1,
      parseHTML: (element: HTMLElement) => Number(element.getAttribute("rowspan") ?? 1),
      renderHTML: (attributes: Record<string, unknown>) =>
        Number(attributes.rowspan ?? 1) === 1 ? {} : { rowspan: String(attributes.rowspan) },
    },
    colwidth: {
      default: null as number[] | null,
      parseHTML: (element: HTMLElement) => {
        const stored = element.getAttribute("data-colwidth");
        if (stored === null || stored === "") return null;
        return stored.split(",").map((part) => Number(part));
      },
      renderHTML: (attributes: Record<string, unknown>) => {
        const value = attributes.colwidth;
        return Array.isArray(value) ? { "data-colwidth": value.join(",") } : {};
      },
    },
    cellBackground: {
      default: null as string | null,
      parseHTML: (element: HTMLElement) => {
        const stored = element.getAttribute("data-background-color");
        return stored === null || stored === "" ? null : stored;
      },
      renderHTML: (attributes: Record<string, unknown>) => {
        const value = attributes.cellBackground;
        return typeof value === "string" && value !== "" ? { "data-background-color": value } : {};
      },
    },
  };
}

/** Build the table node an insert drops at the caret. */
export function buildPageTable(schema: Schema, rows: number, cols: number, withHeaderRow: boolean): PmNode | null {
  const paragraph = schema.nodes["paragraph"];
  const rowType = schema.nodes["tableRow"];
  const tableType = schema.nodes["table"];
  if (paragraph === undefined || rowType === undefined || tableType === undefined) return null;

  const rowNodes: PmNode[] = [];
  for (let row = 0; row < rows; row += 1) {
    const type = withHeaderRow && row === 0 ? schema.nodes["tableHeader"] : schema.nodes["tableCell"];
    if (type === undefined) return null;
    const cells: PmNode[] = [];
    for (let col = 0; col < cols; col += 1) {
      cells.push(type.create({ colwidth: [PAGE_TABLE_COLUMN_WIDTH] }, paragraph.create()));
    }
    rowNodes.push(rowType.create(null, cells));
  }
  return tableType.create(null, rowNodes);
}

/**
 * The first table that starts at or after `from`, with its document
 * position. The just-inserted table is the nearest one by construction;
 * walking for it avoids depending on where a mapped boundary lands.
 */
export function tableAtOrAfter(doc: PmNode, from: number): { pos: number; table: PmNode } | null {
  let found: { pos: number; table: PmNode } | null = null;
  doc.descendants((node, pos) => {
    if (found !== null) return false;
    if (pos + node.nodeSize < from) return true;
    if (node.type.spec["tableRole"] === "table") {
      found = { pos, table: node };
      return false;
    }
    return true;
  });
  return found;
}

/** A document position resolved to the table cell holding it. */
export interface PageTableCellAnchor {
  /** Document position of the table node. */
  tablePos: number;
  /** The table node itself. */
  table: PmNode;
  /** Document position of the cell node holding the position. */
  cellPos: number;
}

/**
 * The table cell holding `pos`, for the row/column structure commands
 * (PAGE-091). Answers null outside a table cell — past the document end
 * or in a plain block — so the commands decline rather than act on a
 * table the handle never targeted.
 */
export function pageTableCellAnchorAt(doc: PmNode, pos: number): PageTableCellAnchor | null {
  if (pos < 0 || pos > doc.content.size) return null;
  const $pos = doc.resolve(pos);
  let tableDepth = -1;
  for (let depth = $pos.depth; depth > 0; depth -= 1) {
    if ($pos.node(depth).type.spec["tableRole"] === "table") {
      tableDepth = depth;
      break;
    }
  }
  if (tableDepth < 0) return null;
  // The cell sits two levels below its table (table, row, cell); a
  // position at the row boundary or above names no cell.
  if ($pos.depth < tableDepth + 2) return null;
  return {
    tablePos: $pos.before(tableDepth),
    table: $pos.node(tableDepth),
    cellPos: $pos.before(tableDepth + 2),
  };
}

/** Copies of rows [top, bottom) plus the document position they land at. */
export interface PageTableRowCopies {
  pos: number;
  nodes: PmNode[];
}

/**
 * Copies of the table's rows [top, bottom), landing after row bottom - 1
 * (PAGE-091 duplicate). The indices come from the row selection's own
 * rectangle, so they always name real rows. Nodes are immutable values,
 * so the originals are shared rather than rebuilt; each copy keeps its
 * cells' content and styling.
 */
export function pageTableDuplicateRows(
  table: PmNode,
  tableStart: number,
  top: number,
  bottom: number
): PageTableRowCopies {
  const nodes: PmNode[] = [];
  for (let row = top; row < bottom; row += 1) nodes.push(table.child(row));
  let pos = tableStart;
  for (let row = 0; row < bottom; row += 1) pos += table.child(row).nodeSize;
  return { pos, nodes };
}

/** One column-copy insert: the cells one row contributes, and where. */
export interface PageTableColumnCopy {
  pos: number;
  nodes: PmNode[];
}

/**
 * Copies of the cells starting in grid columns [left, right), one insert
 * per row holding such a cell, in bottom-up document order so each insert
 * lands at a position no earlier insert has shifted (PAGE-091
 * duplicate). A cell a span carries into the range is left to the row its
 * span starts in; a row no in-range cell starts in contributes nothing.
 */
export function pageTableDuplicateColumns(
  table: PmNode,
  map: TableMapInstance,
  tableStart: number,
  left: number,
  right: number
): PageTableColumnCopy[] {
  const inserts: PageTableColumnCopy[] = [];
  let rowPos = tableStart;
  for (let row = 0; row < map.height; row += 1) {
    const rowNode = table.child(row);
    const copies: PmNode[] = [];
    // The insert lands after the last cell at or left of the range, so
    // the copies sit beside their originals rather than past a neighbour.
    let insertAt = -1;
    let childPos = rowPos + 1;
    for (let index = 0; index < rowNode.childCount; index += 1) {
      const child = rowNode.child(index);
      const end = childPos + child.nodeSize;
      const gridLeft = map.findCell(childPos - tableStart).left;
      if (gridLeft < right) insertAt = end;
      if (gridLeft >= left && gridLeft < right) copies.push(child);
      childPos = end;
    }
    rowPos += rowNode.nodeSize;
    if (copies.length > 0) inserts.unshift({ pos: insertAt, nodes: copies });
  }
  return inserts;
}

/** What the structure commands need from the editor: state and dispatch. */
interface TableCommandEditor {
  state: EditorState;
  view: { dispatch: (tr: Transaction) => void };
  schema: Schema;
}

/**
 * Select the row or column holding the anchored cell. The cell position
 * resolves at the row boundary, which is where the line selections read
 * their table from.
 */
function selectTableLine(
  tables: TablesModule,
  state: EditorState,
  anchor: PageTableCellAnchor,
  axis: PageTableLineAxis
): Transaction {
  const tr = state.tr;
  const $cell = tr.doc.resolve(anchor.cellPos);
  tr.setSelection(axis === "row" ? tables.CellSelection.rowSelection($cell) : tables.CellSelection.colSelection($cell));
  return tr;
}

/**
 * Run one structure command against the row or column holding `pos`: the
 * line is selected first, so the menu always acts on its own handle's
 * line and the highlight shows it, then the edit runs on that selection.
 * A probe only asks whether the position names a table cell. Both steps
 * dispatch through the view — the tables package builds its own
 * transactions, which the chain's transaction would not match — so the
 * chain's own trailing dispatch is suppressed first.
 */
function runTableLineCommand(
  tables: TablesModule,
  chain: Transaction,
  editor: TableCommandEditor,
  dispatch: ((tr: Transaction) => void) | undefined,
  pos: number,
  axis: PageTableLineAxis,
  edit: (state: EditorState, dispatch: (tr: Transaction) => void) => boolean
): boolean {
  const anchor = pageTableCellAnchorAt(editor.state.doc, pos);
  if (anchor === null) return false;
  if (dispatch === undefined) return true;
  chain.setMeta("preventDispatch", true);
  editor.view.dispatch(selectTableLine(tables, editor.state, anchor, axis));
  return edit(editor.state, editor.view.dispatch.bind(editor.view));
}

/**
 * Delete the selected rows — or the whole table when they are all of
 * them. The last row out leaves an empty paragraph with the caret behind
 * (PAGE-091); the tables package itself refuses a full-height delete.
 */
function deleteTableRows(
  tables: TablesModule,
  schema: Schema,
  near: TextSelectionClass["near"],
  state: EditorState,
  dispatch: (tr: Transaction) => void
): boolean {
  const rect = tables.selectedRect(state);
  if (rect.top === 0 && rect.bottom === rect.map.height) {
    const tablePos = rect.tableStart - 1;
    const tr = state.tr;
    tr.replaceWith(tablePos, tablePos + rect.table.nodeSize, schema.node("paragraph"));
    tr.setSelection(near(tr.doc.resolve(rect.tableStart)));
    dispatch(tr);
    return true;
  }
  return tables.deleteRow(state, dispatch);
}

/**
 * Delete the selected columns — or the whole table when they are all of
 * them. A table with no columns is not a table, so the last column out
 * leaves the same empty paragraph the last row does.
 */
function deleteTableColumns(
  tables: TablesModule,
  schema: Schema,
  near: TextSelectionClass["near"],
  state: EditorState,
  dispatch: (tr: Transaction) => void
): boolean {
  const rect = tables.selectedRect(state);
  if (rect.left === 0 && rect.right === rect.map.width) {
    const tablePos = rect.tableStart - 1;
    const tr = state.tr;
    tr.replaceWith(tablePos, tablePos + rect.table.nodeSize, schema.node("paragraph"));
    tr.setSelection(near(tr.doc.resolve(rect.tableStart)));
    dispatch(tr);
    return true;
  }
  return tables.deleteColumn(state, dispatch);
}

/**
 * The table node family. `tableEditing()` supplies the in-cell keyboard
 * behaviour the row requires — Tab and Shift-Tab walk the cells and the
 * arrow keys stay inside the table — so the nodes themselves stay plain.
 * The structure commands (PAGE-091) each take a document position in the
 * targeted cell, select its row or column, and run the edit on that
 * selection.
 */
function createTableNodes(
  Node: Core["Node"],
  mergeAttributes: MergeAttributes,
  tables: TablesModule,
  model: Pick<PmModel, "Fragment" | "Slice">,
  near: TextSelectionClass["near"]
) {
  const Table = Node.create({
    name: "table",
    group: "block",
    content: "tableRow+",
    isolating: true,
    extendNodeSchema: tableRole("table", "table"),
    parseHTML() {
      return [{ tag: "table" }];
    },
    renderHTML({ HTMLAttributes }: { HTMLAttributes: Record<string, unknown> }) {
      return ["table", mergeAttributes(HTMLAttributes), ["tbody", 0]];
    },
    addCommands() {
      // One menu entry, one command: each selects its own handle's line
      // first, so acting never depends on where the caret sits (PAGE-091).
      const line =
        (
          axis: PageTableLineAxis,
          edit: (state: EditorState, dispatch: (tr: Transaction) => void) => boolean
        ): ((attributes: { pos: number }) => (props: CommandProps) => boolean) =>
        ({ pos }: { pos: number }) =>
        ({ tr, dispatch, editor }: CommandProps) =>
          runTableLineCommand(tables, tr, editor, dispatch, pos, axis, edit);
      const selectLine =
        (axis: PageTableLineAxis): ((attributes: { pos: number }) => (props: CommandProps) => boolean) =>
        ({ pos }: { pos: number }) =>
        ({ tr, dispatch, editor }: CommandProps) => {
          const anchor = pageTableCellAnchorAt(editor.state.doc, pos);
          if (anchor === null) return false;
          if (dispatch === undefined) return true;
          tr.setMeta("preventDispatch", true);
          editor.view.dispatch(selectTableLine(tables, editor.state, anchor, axis));
          return true;
        };
      return {
        selectPageTableRow: selectLine("row"),
        selectPageTableColumn: selectLine("column"),
        insertPageTableRowAbove: line("row", (state, dispatch) => tables.addRowBefore(state, dispatch)),
        insertPageTableRowBelow: line("row", (state, dispatch) => tables.addRowAfter(state, dispatch)),
        insertPageTableColumnLeft: line("column", (state, dispatch) => tables.addColumnBefore(state, dispatch)),
        insertPageTableColumnRight: line("column", (state, dispatch) => tables.addColumnAfter(state, dispatch)),
        duplicatePageTableRow: line("row", (state, dispatch) => {
          const rect = tables.selectedRect(state);
          const copies = pageTableDuplicateRows(rect.table, rect.tableStart, rect.top, rect.bottom);
          const tr = state.tr;
          tr.insert(copies.pos, copies.nodes);
          dispatch(tr);
          return true;
        }),
        duplicatePageTableColumn: line("column", (state, dispatch) => {
          const rect = tables.selectedRect(state);
          const inserts = pageTableDuplicateColumns(rect.table, rect.map, rect.tableStart, rect.left, rect.right);
          const tr = state.tr;
          for (const insert of inserts) tr.insert(insert.pos, insert.nodes);
          dispatch(tr);
          return true;
        }),
        clearPageTableCells:
          ({ pos, axis }: { pos: number; axis: PageTableLineAxis }) =>
          ({ tr, dispatch, editor }: CommandProps) =>
            runTableLineCommand(tables, tr, editor, dispatch, pos, axis, (state, innerDispatch) =>
              tables.deleteCellSelection(state, innerDispatch)
            ),
        deletePageTableRow: line("row", (state, dispatch) =>
          deleteTableRows(tables, state.schema, near, state, dispatch)
        ),
        deletePageTableColumn: line("column", (state, dispatch) =>
          deleteTableColumns(tables, state.schema, near, state, dispatch)
        ),
        paintPageTableCells:
          ({ pos, axis, color }: { pos: number; axis: PageTableLineAxis; color: string | null }) =>
          ({ tr, dispatch, editor }: CommandProps) =>
            runTableLineCommand(tables, tr, editor, dispatch, pos, axis, (state, innerDispatch) =>
              tables.setCellAttr("cellBackground", color)(state, innerDispatch)
            ),
        insertPageTable:
          ({
            rows = 3,
            cols = 3,
            withHeaderRow = false,
          }: { rows?: number; cols?: number; withHeaderRow?: boolean } = {}) =>
          ({ tr, dispatch, editor }: CommandProps) => {
            const node = buildPageTable(editor.schema, rows, cols, withHeaderRow);
            if (node === null) return false;
            if (dispatch !== undefined) {
              // Insert the table as its own block at the caret's boundary;
              // `replaceRange` returns the position the table starts at, so
              // the first cell's location is read from its own table map
              // rather than guessed from a mapped selection (PAGE-033).
              const from = tr.selection.from;
              tr.replaceRange(from, from, new model.Slice(model.Fragment.from(node), 0, 0));
              // The insert does not move anything before its own position,
              // so the new table begins exactly at the old caret; the walk
              // cannot miss the node this transaction just placed.
              const landed = tableAtOrAfter(tr.doc, from) as { pos: number; table: PmNode };
              const cellPos = landed.pos + 1 + tables.TableMap.get(landed.table).positionAt(0, 0, landed.table);
              tr.setSelection(near(tr.doc.resolve(cellPos)));
              tr.scrollIntoView();
              dispatch(tr);
            }
            return true;
          },
      } as unknown as Partial<RawCommands>;
    },
    addProseMirrorPlugins() {
      return [tables.tableEditing({ allowTableNodeSelection: true })];
    },
    addKeyboardShortcuts() {
      // Tab and Shift-Tab walk the cells while the caret is inside a
      // table (PAGE-033); outside one the browser's own focus move is
      // left alone, so the walk declines. The tables package builds its
      // own transaction, so it dispatches through the view rather than
      // through the command manager (whose transaction it would not match).
      const walk = (direction: 1 | -1): boolean => {
        const { state, view } = this.editor;
        return tables.goToNextCell(direction)(state, view.dispatch.bind(view));
      };
      return {
        Tab: () => walk(1),
        "Shift-Tab": () => walk(-1),
      };
    },
  });

  const TableRow = Node.create({
    name: "tableRow",
    content: "(tableCell | tableHeader)*",
    extendNodeSchema: tableRole("tableRow", "row"),
    parseHTML() {
      return [{ tag: "tr" }];
    },
    renderHTML({ HTMLAttributes }: { HTMLAttributes: Record<string, unknown> }) {
      return ["tr", mergeAttributes(HTMLAttributes), 0];
    },
  });

  const TableCell = Node.create({
    name: "tableCell",
    content: "block+",
    isolating: true,
    extendNodeSchema: tableRole("tableCell", "cell"),
    addAttributes: cellAttributes,
    parseHTML() {
      return [{ tag: "td" }];
    },
    renderHTML({ HTMLAttributes }: { HTMLAttributes: Record<string, unknown> }) {
      return ["td", mergeAttributes(HTMLAttributes), 0];
    },
  });

  const TableHeader = Node.create({
    name: "tableHeader",
    content: "block+",
    isolating: true,
    extendNodeSchema: tableRole("tableHeader", "header_cell"),
    addAttributes: cellAttributes,
    parseHTML() {
      return [{ tag: "th" }];
    },
    renderHTML({ HTMLAttributes }: { HTMLAttributes: Record<string, unknown> }) {
      return ["th", mergeAttributes(HTMLAttributes), 0];
    },
  });

  return [Table, TableRow, TableCell, TableHeader];
}

/** Load the page's table node family. */
export async function loadPageTableExtensions() {
  const [{ Node, mergeAttributes }, tables, state] = await Promise.all([
    import("@tiptap/core"),
    import("@tiptap/pm/tables"),
    import("@tiptap/pm/state"),
  ]);
  // `Selection.near` is a static that calls `this.findFrom`, so it must
  // keep its receiver when handed to the command.
  const near = state.TextSelection.near.bind(state.TextSelection) as typeof state.TextSelection.near;
  const model = await import("@tiptap/pm/model");
  return createTableNodes(Node, mergeAttributes, tables, model, near);
}

/** Load the page's colour mark. */
export async function loadPageColorMark() {
  const { Mark, mergeAttributes } = await import("@tiptap/core");
  return createPageColorMark(Mark, mergeAttributes);
}

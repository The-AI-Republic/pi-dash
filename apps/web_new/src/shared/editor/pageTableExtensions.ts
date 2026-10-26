// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page table nodes and the colour mark (parity/specs/pages.md §5.3,
// PAGE-033/036). Tiptap serves tables from `@tiptap/pm/tables`, which the
// page editor already carries through `@tiptap/pm`, so the page set gains
// tables without a new runtime package. Everything is built inside the
// lazy loaders (the boundary check allows ProseMirror only through
// import()), and each node serialises to the markup the API's content
// validator already accepts: `<table>/<tr>/<th>/<td>` for tables and a
// `<span>` carrying `data-text-color`/`data-background-color` for colour.

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
type TableEditing = (options?: { allowTableNodeSelection?: boolean }) => InstanceType<State["Plugin"]>;
type GoToNextCell = (direction: 1 | -1) => (state: EditorState, dispatch?: (tr: Transaction) => void) => boolean;
type TableMap = typeof import("@tiptap/pm/tables").TableMap;

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
}

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

/**
 * The table node family. `tableEditing()` supplies the in-cell keyboard
 * behaviour the row requires — Tab and Shift-Tab walk the cells and the
 * arrow keys stay inside the table — so the nodes themselves stay plain.
 */
function createTableNodes(
  Node: Core["Node"],
  mergeAttributes: MergeAttributes,
  tableEditing: TableEditing,
  TableMap: TableMap,
  model: Pick<PmModel, "Fragment" | "Slice">,
  goToNextCell: GoToNextCell,
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
      return {
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
              const cellPos = landed.pos + 1 + TableMap.get(landed.table).positionAt(0, 0, landed.table);
              tr.setSelection(near(tr.doc.resolve(cellPos)));
              tr.scrollIntoView();
              dispatch(tr);
            }
            return true;
          },
      } as unknown as Partial<RawCommands>;
    },
    addProseMirrorPlugins() {
      return [tableEditing({ allowTableNodeSelection: true })];
    },
    addKeyboardShortcuts() {
      // Tab and Shift-Tab walk the cells while the caret is inside a
      // table (PAGE-033); outside one the browser's own focus move is
      // left alone, so the walk declines. The tables package builds its
      // own transaction, so it dispatches through the view rather than
      // through the command manager (whose transaction it would not match).
      const walk = (direction: 1 | -1): boolean => {
        const { state, view } = this.editor;
        return goToNextCell(direction)(state, view.dispatch.bind(view));
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
  return createTableNodes(
    Node,
    mergeAttributes,
    tables.tableEditing,
    tables.TableMap,
    model,
    tables.goToNextCell,
    near
  );
}

/** Load the page's colour mark. */
export async function loadPageColorMark() {
  const { Mark, mergeAttributes } = await import("@tiptap/core");
  return createPageColorMark(Mark, mergeAttributes);
}

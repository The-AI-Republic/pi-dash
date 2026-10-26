// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page editor command adapter (parity/specs/pages.md §5.3,
// PAGE-028–032). The editor itself lives in the lazy component; this
// module is the seam between it and the toolbar, written against a small
// structural interface so each command's chain call, the block restyle
// and the caret jump are unit-testable without a browser or Tiptap.
// Keeping the switch here means the component has no per-command
// branches to cover twice.

import {
  headingLevelOf,
  readFormattingState,
  type PageBlockKind,
  type PageFormatCommand,
  type PageFormattingState,
} from "../../../shared/editor/pageFormatting.js";
import { emojiHtml, embedHtml, type PageCatalogBlockKind, type PageEmbedAttributes } from "./pageBlocks.js";
import type { PageEntityReferenceAttributes } from "../../../shared/editor/pageBlockExtensions.js";

/** The chain methods this adapter calls, by the command that calls them. */
export interface PageEditorChain {
  focus: () => PageEditorChain;
  toggleBold: () => PageEditorChain;
  toggleItalic: () => PageEditorChain;
  toggleUnderline: () => PageEditorChain;
  toggleStrike: () => PageEditorChain;
  setTextAlign: (alignment: string) => PageEditorChain;
  toggleOrderedList: () => PageEditorChain;
  toggleBulletList: () => PageEditorChain;
  toggleTaskList: () => PageEditorChain;
  toggleBlockquote: () => PageEditorChain;
  toggleCode: () => PageEditorChain;
  setParagraph: () => PageEditorChain;
  toggleHeading: (attributes: { level: number }) => PageEditorChain;
  setContent: (content: string, emitUpdate?: boolean) => PageEditorChain;
  /** Drop a 3×3 table at the caret (PAGE-033). */
  insertPageTable: (attributes?: { rows?: number; cols?: number; withHeaderRow?: boolean }) => PageEditorChain;
  /** Apply one mark to the range (the colour pair rides on one mark). */
  setMark: (name: string, attributes: Record<string, unknown>) => PageEditorChain;
  /** Remove one mark from the range (PAGE-036 clear). */
  unsetMark: (name: string) => PageEditorChain;
  /** Point the selection at a document range (the Ctrl+A repair, PAGE-031). */
  setTextSelection: (range: { from: number; to: number }) => PageEditorChain;
  /** Insert one node or HTML string at the caret (PAGE-034). */
  insertContent: (content: unknown) => PageEditorChain;
  /** Write content at a position or over a range (PAGE-037 block moves). */
  insertContentAt: (position: number | "end" | { from: number; to: number }, content: unknown) => PageEditorChain;
  /** Delete one document range (drag-handle Delete). */
  deleteRange: (range: { from: number; to: number }) => PageEditorChain;
  /** Set or replace the link mark on the selection (PAGE-043). */
  setLink: (attributes: { href: string }) => PageEditorChain;
  /** Remove the link mark (PAGE-043). */
  unsetLink: () => PageEditorChain;
  run: () => boolean;
}

/**
 * Where a focus command puts the caret: a document position, or the
 * editor's own `start`/`end` keywords. The keywords matter because
 * position 0 is outside the document's text range and Tiptap treats it
 * as "no position given", leaving the caret where it was.
 */
export type PageFocusPosition = number | "start" | "end";

/** A resolved document position, narrowed to what the block reads use. */
export interface PageResolvedPosition {
  depth?: number;
  before?: (depth: number) => number;
  after?: (depth: number) => number;
  node?: (depth: number) => { nodeSize: number; toJSON: () => unknown };
}

/** One document node, narrowed to the reads the block geometry makes. */
export interface PageEditorNode {
  nodeSize: number;
  toJSON: () => unknown;
  type: { name: string };
  /** How many children the node holds. */
  childCount?: number;
  /** One child by index, for the endpoint walk. */
  child?: (index: number) => PageEditorNode;
  /** True when the node is a text container (a paragraph, a heading…). */
  isTextblock?: boolean;
  /** The node's own inline size, without its own open/close tokens. */
  content?: { size: number };
}

/** The document half of the editor view the block geometry reads. */
export interface PageEditorDocument extends PageEditorNode {
  childCount: number;
  child: (index: number) => PageEditorNode;
  resolve: (position: number) => PageResolvedPosition;
}

/** The view methods the hovered-block resolver needs. */
export interface PageEditorView {
  dom: {
    getBoundingClientRect: () => { top: number; left?: number; width?: number; height?: number };
    contains: (node: Node | null) => boolean;
  };
  posAtCoords: (coords: { left: number; top: number }) => { pos: number } | null;
  coordsAtPos: (position: number) => { top: number; left?: number };
  state: { doc: PageEditorDocument };
}

/** What the adapter needs from a Tiptap editor. */
export interface PageEditorRef {
  chain: () => PageEditorChain;
  isActive: (name: string, attributes?: Record<string, unknown>) => boolean;
  getHTML: () => string;
  /** Tiptap's focus command: no argument focuses the current caret. */
  commands: { focus: (position?: PageFocusPosition) => void };
  /** Tiptap's mark-attribute read; optional so simple probes still work. */
  getAttributes?: (name: string) => Record<string, unknown>;
  /** Present on a live editor; absent on simple probes and in unit tests. */
  view?: PageEditorView;
  state: {
    selection: {
      from?: number;
      to?: number;
      $from: {
        depth?: number;
        parent: {
          type: { name: string };
          attrs: Record<string, unknown>;
          textContent: string;
          textBetween?: (from: number, to: number, block?: string, leaf?: string) => string;
        };
        doc?: { textBetween: (from: number, to: number, block?: string, leaf?: string) => string };
        start?: () => number;
        parentOffset?: number;
        before?: (depth: number) => number;
        after?: (depth: number) => number;
        node?: (depth: number) => { nodeSize: number; toJSON: () => unknown };
        textBetween?: (from: number, to: number, block?: string, leaf?: string) => string;
      };
      $to?: { parentOffset?: number };
    };
  };
}

/** What the title, the bar and later slices drive on the live editor. */
export interface PageEditorHandle {
  /** Run one formatting command at the caret (bold, lists, quote, code…). */
  applyCommand: (command: PageFormatCommand) => void;
  /** Insert a 3×3 table at the caret (PAGE-033). */
  insertTable: () => void;
  /** Apply one letter or wash colour to the current range (PAGE-036). */
  applyColor: (kind: "text" | "background", key: string) => void;
  /** Clear one colour kind back to plain text/wash (PAGE-036). */
  clearColor: (kind: "text" | "background") => void;
  /** Insert an uploaded picture at the caret (PAGE-034). `alt` names its row. */
  insertImage: (src: string, storedSrc: string, alt?: string) => void;
  /** Restyle the block under the caret as the given typography entry. */
  applyBlockKind: (kind: PageBlockKind) => void;
  /** Run one slash-catalogue entry (PAGE-037). */
  applyCatalogBlock: (block: PageCatalogBlockKind) => void;
  /** Insert an inline body emoji at the caret (PAGE-042). */
  insertEmoji: (glyph: string) => void;
  /** Insert a person mention at the caret, replacing a live @query (PAGE-041). */
  insertMention: (target: { id: string; name: string }) => void;
  /** Insert a work-item embed as an atomic block (PAGE-040). */
  insertEmbed: (attributes: PageEmbedAttributes) => void;
  /** Insert a normalised entity reference inline (PAGE-043). */
  insertEntityReference: (attributes: PageEntityReferenceAttributes) => void;
  /** Insert plain text at the caret (the slash Mention command's trigger). */
  insertText: (text: string) => void;
  /** Duplicate or delete the block that owns the caret (PAGE-037). */
  duplicateBlock: (target?: PageCaretBlock | null) => void;
  deleteBlock: (target?: PageCaretBlock | null) => void;
  /** Move a block one slot up or down past its sibling (PAGE-037). */
  moveBlock: (target: PageCaretBlock, direction: "up" | "down") => void;
  /** The top-level block under a viewport point, for the hover handle. */
  blockAtPoint: (clientX: number, clientY: number) => PageHoveredBlock | null;
  /**
   * Where the caret sits in viewport coordinates. The transient surfaces
   * anchor here so they open beside the caret rather than at the end of
   * the document (PAGE-037/042).
   */
  caretAnchor: () => { top: number; left: number } | null;
  /** Set the link on the selection in place (PAGE-043). */
  setLink: (href: string) => void;
  /** Remove the link mark from the selection (PAGE-043). */
  unsetLink: () => void;
  /** The live document HTML, for the copy-markup entry (PAGE-044). */
  html: () => string;
  /** The link mark's address under the caret, or null (PAGE-043). */
  linkHref: () => string | null;
  /** Whether the editor currently has a non-empty text selection (PAGE-037). */
  hasSelection: () => boolean;
  /** The selection's document range, or null when nothing is selected (PAGE-075). */
  selectionRange: () => PageTextRange | null;
  /** The caret's document position, or null when the selection is missing (PAGE-078). */
  caretPosition: () => number | null;
  /**
   * Write markup at a position or over a range: a number inserts at the
   * caret-side position, a range replaces it (PAGE-075 assistant
   * apply/append, PAGE-078 registry snippets).
   */
  insertAt: (at: number | PageTextRange, html: string) => void;
  /** Text between the block start and the caret, for slash/@ queries. */
  textBeforeCaret: () => string;
  /** Delete the typed slash/@ trigger immediately before the caret. */
  deleteBeforeCaret: (length: number) => void;
  /** The selected text, for the link bubble prefill. */
  selectedText: () => string;
  /** Collapse the caret to the end of the document. */
  focusEnd: () => void;
  /** Put the caret at the very start of the body (title Enter, PAGE-026). */
  focusStart: () => void;
  /** The formatting state right now, for a bar that mounted after it. */
  readState: () => PageFormattingState;
  /**
   * Replace the whole body with stored markup, in place (PAGE-053).
   * Answers false when the editor refused the command, so the caller
   * reports the refusal instead of a change that never happened.
   */
  replaceBody: (html: string) => boolean;
}

/**
 * The inline endpoints of the document, or null when it has no text
 * block to select. A whole-document selection starts at the document's
 * first valid position and ends at its content end — both *outside* the
 * blocks the command is meant to restyle, so ProseMirror's block range
 * resolves at the document and a list lift has nothing to act on. The
 * endpoints here are the first text-block position and the last text
 * block's inline end, which is the range a native Ctrl+A over the text
 * produces (PAGE-031).
 */
export function pageTextEndpoints(doc: PageEditorDocument): { from: number; to: number } | null {
  const firstTextblock = (node: PageEditorNode, position: number, root: boolean): number | null => {
    if (node.isTextblock === true) return position + 1;
    const childCount = node.childCount ?? 0;
    let offset = root ? position : position + 1;
    for (let index = 0; index < childCount; index++) {
      const child = node.child?.(index);
      if (child === undefined) return null;
      const found = firstTextblock(child, offset, false);
      if (found !== null) return found;
      offset += child.nodeSize;
    }
    return null;
  };
  const lastTextblock = (node: PageEditorNode, position: number, root: boolean): number | null => {
    if (node.isTextblock === true) return position + 1 + (node.content?.size ?? 0);
    const childCount = node.childCount ?? 0;
    let offset = root ? position : position + 1;
    let last: number | null = null;
    for (let index = 0; index < childCount; index++) {
      const child = node.child?.(index);
      if (child === undefined) return null;
      const found = lastTextblock(child, offset, false);
      if (found !== null) last = found;
      offset += child.nodeSize;
    }
    return last;
  };
  const from = firstTextblock(doc, 0, true);
  const to = lastTextblock(doc, 0, true);
  if (from === null || to === null) return null;
  return { from, to };
}

/**
 * The selection repaired to the document's text endpoints when it runs
 * past them, or null when it already points inside the text (a partial
 * selection, a caret) or repair is not possible. Only the ends that sit
 * on a document boundary are pulled inward: a selection that starts
 * before the first text block keeps its start at that block's first
 * position, and one that ends at the document's content end keeps its
 * end at the last text block's inline end. That is what makes a Ctrl+A
 * whole-document selection behave like the same selection made with the
 * mouse (PAGE-031).
 */
function textEndpointRange(editor: PageEditorRef): { from: number; to: number } | null {
  const doc = editor.view?.state.doc;
  if (doc === undefined) return null;
  const { from, to } = editor.state.selection;
  // A caret restyles the block it sits in already; expanding it into the
  // whole document would change what the command means.
  if (from === undefined || to === undefined || from === to) return null;
  const endpoints = pageTextEndpoints(doc);
  if (endpoints === null) return null;
  const start = from <= endpoints.from ? endpoints.from : from;
  const end = to >= endpoints.to ? endpoints.to : to;
  if (start === from && end === to) return null;
  if (start > end) return null;
  return { from: start, to: end };
}

/**
 * Run one formatting command: focus the editor first so a click on a
 * toolbar button never drops the selection it is meant to restyle.
 */
export function runPageCommand(editor: PageEditorRef, command: PageFormatCommand): void {
  const repair = textEndpointRange(editor);
  const chain = repair === null ? editor.chain().focus() : editor.chain().focus().setTextSelection(repair);
  switch (command) {
    case "bold":
      chain.toggleBold().run();
      return;
    case "italic":
      chain.toggleItalic().run();
      return;
    case "underline":
      chain.toggleUnderline().run();
      return;
    case "strike":
      chain.toggleStrike().run();
      return;
    case "alignLeft":
      chain.setTextAlign("left").run();
      return;
    case "alignCenter":
      chain.setTextAlign("center").run();
      return;
    case "alignRight":
      chain.setTextAlign("right").run();
      return;
    case "orderedList":
      chain.toggleOrderedList().run();
      return;
    case "bulletList":
      chain.toggleBulletList().run();
      return;
    case "taskList":
      chain.toggleTaskList().run();
      return;
    case "blockquote":
      chain.toggleBlockquote().run();
      return;
    case "code":
      chain.toggleCode().run();
  }
}

/**
 * Insert the toolbar's table at the caret (PAGE-033). The table nodes'
 * own command lands the caret in the first cell, so the next Tab walks
 * the cells rather than leaving the document.
 */
export function insertPageTable(editor: PageEditorRef): void {
  editor.chain().focus().insertPageTable({ rows: 3, cols: 3, withHeaderRow: false }).run();
}

/**
 * The mark attribute one colour kind writes (PAGE-036). The mark
 * attribute for letters is the live server's `color`; the adapter's own
 * pair keeps the `textColor` name, so only this table maps the two.
 */
const COLOR_ATTRIBUTE = { text: "color", background: "backgroundColor" } as const;
/** The mark name the letter/wash pair lives on: the live server's own (NEWFRONT-929). */
export const PAGE_COLOR_MARK = "customColor";

/** The pair the current range carries, read from the mark's attributes. */
function currentPageColor(editor: PageEditorRef): {
  color: string | null;
  backgroundColor: string | null;
} {
  const attributes = editor.getAttributes?.(PAGE_COLOR_MARK) ?? {};
  return {
    color: typeof attributes.color === "string" ? attributes.color : null,
    backgroundColor: typeof attributes.backgroundColor === "string" ? attributes.backgroundColor : null,
  };
}

/** Apply one palette key to the current range (PAGE-036). */
export function applyPageColor(editor: PageEditorRef, kind: "text" | "background", key: string): void {
  const current = currentPageColor(editor);
  // Both attributes are written on every change so the untouched kind
  // survives: a wash applied to coloured letters keeps the letters.
  editor
    .chain()
    .focus()
    .setMark(PAGE_COLOR_MARK, { ...current, [COLOR_ATTRIBUTE[kind]]: key })
    .run();
}

/** Remove one colour kind from the current range (PAGE-036). */
export function clearPageColor(editor: PageEditorRef, kind: "text" | "background"): void {
  const current = currentPageColor(editor);
  const next = { ...current, [COLOR_ATTRIBUTE[kind]]: null };
  const chain = editor.chain().focus();
  // Clearing the last remaining kind drops the mark itself, so the text
  // returns to plain styling rather than an empty span.
  if (next.color === null && next.backgroundColor === null) {
    chain.unsetMark(PAGE_COLOR_MARK).run();
    return;
  }
  chain.setMark(PAGE_COLOR_MARK, next).run();
}

/**
 * Insert an uploaded picture at the caret (PAGE-034). The node is the
 * shared stored-image shape: the stored asset id rides in
 * `data-stored-src` (and `src`, as every client stores it) with the
 * upload status the oracle reads back, while the editable surface paints
 * the upload's served URL in `src`.
 */
export function runInsertImage(editor: PageEditorRef, src: string, storedSrc: string, alt?: string): void {
  editor
    .chain()
    .focus()
    .insertContent({
      type: "imageComponent",
      attrs: {
        src,
        "data-stored-src": storedSrc,
        status: "uploaded",
        ...(alt === undefined ? {} : { alt }),
      },
    })
    .run();
}

/** Restyle the caret's block as plain text or one heading level. */
export function runBlockKind(editor: PageEditorRef, kind: PageBlockKind): void {
  const level = headingLevelOf(kind);
  if (level === null) {
    editor.chain().focus().setParagraph().run();
    return;
  }
  editor.chain().focus().toggleHeading({ level }).run();
}

/**
 * Swap the body for another document without leaving the page (PAGE-053).
 * The update is emitted, so every listener the editor already has — the
 * draft seam the save slice persists from, the live document the pane
 * reads — sees the restored body exactly as it would see a typed one.
 * Answers the command's own result: a refused write must not be
 * reported as a restore.
 */
export function replacePageBody(editor: PageEditorRef, html: string): boolean {
  return editor.chain().setContent(html, true).run();
}

/** The text block holding the caret, in the shape the reader expects. */
export function caretBlockOf(editor: PageEditorRef): { type: string; attrs: Record<string, unknown> } {
  const parent = editor.state.selection.$from.parent;
  return { type: parent.type.name, attrs: parent.attrs };
}

/** The formatting state the editor reports for the caret/selection. */
export function editorFormattingState(editor: PageEditorRef): PageFormattingState {
  return readFormattingState({
    isActive: (name, attributes) => editor.isActive(name, attributes),
    caretBlock: () => caretBlockOf(editor),
    inTaskList: () => editor.isActive("taskList"),
    colorAttrs: () => pageColorAttrs(editor),
  });
}

/**
 * The colour mark's attributes over the caret, or null when the range
 * holds no colour. Tiptap reports a mark's attributes through
 * `getAttributes`; the reader stays optional so older probes keep working.
 */
function pageColorAttrs(editor: PageEditorRef): { textColor?: unknown; backgroundColor?: unknown } | null {
  if (editor.getAttributes === undefined) return null;
  const attrs = editor.getAttributes(PAGE_COLOR_MARK) as { color?: unknown; backgroundColor?: unknown };
  if (attrs.color === undefined && attrs.backgroundColor === undefined) return null;
  // The formatting state names the letter kind `textColor`; the mark
  // attribute is the server's `color`, mapped here at the boundary.
  return { textColor: attrs.color, backgroundColor: attrs.backgroundColor };
}

/** True when the document holds no text at all (placeholder decision). */
export function pageDocumentEmpty(editor: PageEditorRef): boolean {
  return (
    editor
      .getHTML()
      .replace(/<[^>]*>/g, "")
      .trim() === ""
  );
}

/**
 * The body's first caret position. The editor's `start` keyword is the
 * document's own first valid position (before the first block's text),
 * which is what pressing Enter in the title hands the caret to.
 */
export const PAGE_BODY_START: PageFocusPosition = "start";

/** Build the handle the toolbar and title hold. */
export function pageEditorHandle(editor: PageEditorRef): PageEditorHandle {
  return {
    applyCommand: (command) => runPageCommand(editor, command),
    applyBlockKind: (kind) => runBlockKind(editor, kind),
    insertTable: () => insertPageTable(editor),
    applyColor: (kind, key) => applyPageColor(editor, kind, key),
    clearColor: (kind) => clearPageColor(editor, kind),
    insertImage: (src, storedSrc, alt) => runInsertImage(editor, src, storedSrc, alt),
    applyCatalogBlock: (block) => runCatalogBlock(editor, block),
    insertEmoji: (glyph) => insertPageEmoji(editor, glyph),
    insertMention: (target) =>
      editor
        .chain()
        .focus()
        .insertContent({
          type: "mention",
          attrs: { id: target.id, entity_identifier: target.id, entity_name: "user_mention", "data-name": target.name },
        })
        .run(),
    insertEmbed: (attributes) => insertPageEmbed(editor, attributes),
    insertEntityReference: (attributes) => insertPageEntityReference(editor, attributes),
    insertText: (text) => {
      editor.chain().focus().insertContent(text).run();
    },
    duplicateBlock: (target) => duplicatePageBlock(editor, target),
    deleteBlock: (target) => deletePageBlock(editor, target),
    moveBlock: (target, direction) => movePageBlock(editor, target, direction),
    blockAtPoint: (clientX, clientY) => pageBlockAtPoint(editor, clientX, clientY),
    caretAnchor: () => pageCaretAnchor(editor),
    setLink: (href) => linkSelection(editor, href),
    unsetLink: () => unlinkSelection(editor),
    html: () => editor.getHTML(),
    linkHref: () => {
      if (!editor.isActive("link") || editor.getAttributes === undefined) return null;
      const href = editor.getAttributes("link")["href"];
      return typeof href === "string" && href !== "" ? href : null;
    },
    hasSelection: () => pageSelectionRange(editor) !== null,
    selectionRange: () => pageSelectionRange(editor),
    caretPosition: () => pageCaretPosition(editor),
    insertAt: (at, html) => {
      editor.chain().focus().insertContentAt(at, html).run();
    },
    textBeforeCaret: () => pageTextBeforeCaret(editor),
    deleteBeforeCaret: (length) => deleteContentBeforeCaret(editor, length),
    selectedText: () => selectedPageText(editor),
    focusEnd: () => editor.commands.focus("end"),
    focusStart: () => editor.commands.focus(PAGE_BODY_START),
    readState: () => editorFormattingState(editor),
    replaceBody: (html) => replacePageBody(editor, html),
  };
}

/** One top-level block around the caret: its document range and JSON. */
export interface PageCaretBlock {
  from: number;
  to: number;
  json: unknown;
}

/**
 * The top-level block the caret sits in. Depth 1 is the document's own
 * child list, so a paragraph inside a callout resolves at its outer
 * container when the selection is deeper than one level.
 */
export function caretBlockRange(editor: PageEditorRef): PageCaretBlock | null {
  const { $from } = editor.state.selection;
  const depth = $from.depth ?? 0;
  if (depth < 1 || $from.before === undefined || $from.after === undefined || $from.node === undefined) return null;
  return { from: $from.before(1), to: $from.after(1), json: $from.node(1).toJSON() };
}

/** Duplicate one block, immediately after itself (default: the caret's). */
export function duplicatePageBlock(editor: PageEditorRef, target?: PageCaretBlock | null): void {
  const block = target ?? caretBlockRange(editor);
  if (block === null) return;
  editor.chain().focus().insertContentAt(block.to, block.json).run();
}

/** Remove one block (default: the caret's), never the surrounding document. */
export function deletePageBlock(editor: PageEditorRef, target?: PageCaretBlock | null): void {
  const block = target ?? caretBlockRange(editor);
  if (block === null) return;
  editor.chain().focus().deleteRange({ from: block.from, to: block.to }).run();
}

/** Every top-level block with its document range, in reading order. */
export function pageTopLevelBlocks(editor: PageEditorRef): PageCaretBlock[] {
  const doc = editor.view?.state.doc;
  if (doc === undefined) return [];
  const blocks: PageCaretBlock[] = [];
  let position = 0;
  for (let index = 0; index < doc.childCount; index++) {
    const child = doc.child(index);
    blocks.push({ from: position, to: position + child.nodeSize, json: child.toJSON() });
    position += child.nodeSize;
  }
  return blocks;
}

/**
 * Swap one block with its sibling above or below (PAGE-037). The later
 * range is rewritten first so replacing the earlier one never shifts the
 * range still to be written.
 */
export function movePageBlock(editor: PageEditorRef, target: PageCaretBlock, direction: "up" | "down"): void {
  const blocks = pageTopLevelBlocks(editor);
  const index = blocks.findIndex((block) => block.from === target.from && block.to === target.to);
  if (index === -1) return;
  if (direction === "up") {
    const previous = blocks[index - 1];
    if (previous === undefined) return;
    editor.chain().focus().insertContentAt({ from: target.from, to: target.to }, previous.json).run();
    editor.chain().focus().insertContentAt({ from: previous.from, to: previous.to }, target.json).run();
    return;
  }
  const next = blocks[index + 1];
  if (next === undefined) return;
  editor.chain().focus().insertContentAt({ from: next.from, to: next.to }, target.json).run();
  editor.chain().focus().insertContentAt({ from: target.from, to: target.to }, next.json).run();
}

/**
 * The caret's position in viewport coordinates, which is the space the
 * transient surfaces are positioned in. A view without geometry answers
 * null and the surfaces fall back to the editor's own top.
 */
export function pageCaretAnchor(editor: PageEditorRef): { top: number; left: number } | null {
  const view = editor.view;
  if (view === undefined) return null;
  const from = editor.state.selection.from;
  const position = typeof from === "number" ? from : 0;
  const coords = view.coordsAtPos(position);
  return { top: coords.top, left: coords.left ?? 0 };
}

/** One top-level block plus where the hover handle should sit beside it. */
export interface PageHoveredBlock extends PageCaretBlock {
  /** The block's top edge in viewport coordinates, in px. */
  top: number;
  /** The block's left edge in viewport coordinates, in px. */
  left: number;
}

/**
 * The top-level block under a viewport point (PAGE-037). The drag handle
 * is driven by where the pointer actually is, so its menu always acts on
 * the block that was hovered rather than whichever block holds the caret.
 * A point outside the editor, or one ProseMirror cannot place, answers
 * null and the handle hides.
 */
export function pageBlockAtPoint(editor: PageEditorRef, clientX: number, clientY: number): PageHoveredBlock | null {
  const view = editor.view;
  if (view === undefined) return null;
  const found = view.posAtCoords({ left: clientX, top: clientY });
  if (found === null) return null;
  const blocks = pageTopLevelBlocks(editor);
  if (blocks.length === 0) return null;
  // The block whose half-open range holds the point; the last block also
  // takes the document's final position.
  const inside = blocks.find((block) => found.pos >= block.from && found.pos < block.to) ?? blocks[blocks.length - 1];
  if (inside === undefined) return null;
  const blockCoords = view.coordsAtPos(inside.from);
  return { ...inside, top: Math.round(blockCoords.top), left: Math.round(blockCoords.left ?? 0) };
}

/** Insert one slash-catalogue entry at the caret (PAGE-037). */
export function runCatalogBlock(editor: PageEditorRef, block: PageCatalogBlockKind): void {
  switch (block) {
    case "embed":
    case "entity":
      // These rows open their picker in the interaction layer; reaching
      // the adapter without a choice is a no-op.
      return;
    case "text":
      runBlockKind(editor, "paragraph");
      return;
    case "heading1":
    case "heading2":
    case "heading3":
    case "heading4":
    case "heading5":
    case "heading6":
      runBlockKind(editor, block);
      return;
    case "numbered":
      runPageCommand(editor, "orderedList");
      return;
    case "bulleted":
      runPageCommand(editor, "bulletList");
      return;
    case "todo":
      runPageCommand(editor, "taskList");
      return;
    case "table":
      insertPageTable(editor);
      return;
    case "quote":
      runPageCommand(editor, "blockquote");
      return;
    case "code":
      editor.chain().focus().insertContent({ type: "codeBlock" }).run();
      return;
    case "callout":
      editor
        .chain()
        .focus()
        .insertContent({ type: "pageCallout", content: [{ type: "paragraph" }] })
        .run();
      return;
    case "divider":
      editor.chain().focus().insertContent({ type: "horizontalRule" }).run();
      return;
    case "emoji":
      // The palette row opens the inline picker instead of inserting a
      // block; the surface handles this sentinel before reaching here.
      return;
    case "image":
      return;
  }
}

/** Insert one body emoji inline at the caret (PAGE-042). */
export function insertPageEmoji(editor: PageEditorRef, glyph: string): void {
  editor.chain().focus().insertContent(emojiHtml(glyph)).run();
}

/**
 * Insert a fresh asset picture, cloning a foreign asset first when the
 * source belongs to another page (PAGE-039). The clone result is the new
 * stored id; the caller supplies the served address.
 */
export function insertPageEmbed(editor: PageEditorRef, attributes: PageEmbedAttributes): void {
  editor.chain().focus().insertContent(embedHtml(attributes)).run();
}

/** Insert one stored entity reference inline at the caret (PAGE-043). */
export function insertPageEntityReference(editor: PageEditorRef, attributes: PageEntityReferenceAttributes): void {
  editor
    .chain()
    .focus()
    .insertContent({
      type: "pageEntityReference",
      attrs: attributes,
    })
    .run();
}

/** Set the link on the current selection in place (PAGE-043). */
export function linkSelection(editor: PageEditorRef, href: string): void {
  editor.chain().focus().setLink({ href }).run();
}

/** Remove the link mark from the selection (PAGE-043). */
export function unlinkSelection(editor: PageEditorRef): void {
  editor.chain().focus().unsetLink().run();
}

/** A captured text range, for writes after the selection is gone (PAGE-075). */
export interface PageTextRange {
  from: number;
  to: number;
}

/**
 * The selection's document range, or null for a collapsed or missing
 * selection. The single predicate behind `hasSelection`, so the two can
 * never disagree.
 */
export function pageSelectionRange(editor: PageEditorRef): PageTextRange | null {
  const { from, to } = editor.state.selection;
  if (from === undefined || to === undefined || to <= from) return null;
  return { from, to };
}

/**
 * The caret's document position: the selection head, which is where a
 * collapsed caret sits. A slash palette only exists at a collapsed
 * caret, so the registry snippet lands here rather than on the
 * selection range, which a collapsed caret never yields (PAGE-078).
 */
export function pageCaretPosition(editor: PageEditorRef): number | null {
  const { from, to } = editor.state.selection;
  if (to !== undefined) return to;
  if (from !== undefined) return from;
  return null;
}

/** The selected text, or the text block under the caret (link prefill). */
export function selectedPageText(editor: PageEditorRef): string {
  const { $from } = editor.state.selection;
  const selection = editor.state.selection;
  if (selection.from !== undefined && selection.to !== undefined && selection.to > selection.from) {
    // ProseMirror puts `textBetween` on the document, not on a resolved
    // position, so a range's text comes from the document it belongs to.
    const doc = $from.doc;
    if (doc !== undefined) return doc.textBetween(selection.from, selection.to, " ");
  }
  return $from.parent.textContent ?? "";
}

/** Delete `length` characters immediately before the caret. */
export function deleteContentBeforeCaret(editor: PageEditorRef, length: number): void {
  const { $from } = editor.state.selection;
  if (length <= 0 || $from.start === undefined || $from.parentOffset === undefined) return;
  const caret = $from.start() + $from.parentOffset;
  editor
    .chain()
    .focus()
    .deleteRange({ from: caret - length, to: caret })
    .run();
}

/** Text before the caret in its own block, for slash and @ queries. */
export function pageTextBeforeCaret(editor: PageEditorRef): string {
  const { $from } = editor.state.selection;
  // The text comes from the caret's own block: a resolved position has no
  // `textBetween`, only the node does (PAGE-037/041 trigger detection).
  const block = $from.parent;
  if (typeof block.textBetween !== "function" || $from.parentOffset === undefined) return "";
  return block.textBetween(0, $from.parentOffset, undefined, "\ufffc");
}

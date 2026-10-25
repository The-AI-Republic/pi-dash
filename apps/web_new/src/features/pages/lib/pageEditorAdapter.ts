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
  /** Insert one node or HTML string at the caret (PAGE-034). */
  insertContent: (content: unknown) => PageEditorChain;
  run: () => boolean;
}

/**
 * Where a focus command puts the caret: a document position, or the
 * editor's own `start`/`end` keywords. The keywords matter because
 * position 0 is outside the document's text range and Tiptap treats it
 * as "no position given", leaving the caret where it was.
 */
export type PageFocusPosition = number | "start" | "end";

/** What the adapter needs from a Tiptap editor. */
export interface PageEditorRef {
  chain: () => PageEditorChain;
  isActive: (name: string, attributes?: Record<string, unknown>) => boolean;
  getHTML: () => string;
  /** Tiptap's focus command: no argument focuses the current caret. */
  commands: { focus: (position?: PageFocusPosition) => void };
  /** Tiptap's mark-attribute read; optional so simple probes still work. */
  getAttributes?: (name: string) => Record<string, unknown>;
  state: {
    selection: {
      $from: {
        parent: { type: { name: string }; attrs: Record<string, unknown>; textContent: string };
      };
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
  /** Insert an uploaded picture at the caret (PAGE-034). */
  insertImage: (src: string, storedSrc: string) => void;
  /** Restyle the block under the caret as the given typography entry. */
  applyBlockKind: (kind: PageBlockKind) => void;
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
 * Run one formatting command: focus the editor first so a click on a
 * toolbar button never drops the selection it is meant to restyle.
 */
export function runPageCommand(editor: PageEditorRef, command: PageFormatCommand): void {
  const chain = editor.chain().focus();
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

/** The mark attribute one colour kind writes (PAGE-036). */
const COLOR_ATTRIBUTE = { text: "textColor", background: "backgroundColor" } as const;
/** The mark name the letter/wash pair lives on. */
export const PAGE_COLOR_MARK = "pageColor";

/** The pair the current range carries, read from the mark's attributes. */
function currentPageColor(editor: PageEditorRef): { textColor: string | null; backgroundColor: string | null } {
  const attributes = editor.getAttributes?.(PAGE_COLOR_MARK) ?? {};
  return {
    textColor: typeof attributes.textColor === "string" ? attributes.textColor : null,
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
  if (next.textColor === null && next.backgroundColor === null) {
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
export function runInsertImage(editor: PageEditorRef, src: string, storedSrc: string): void {
  editor
    .chain()
    .focus()
    .insertContent({
      type: "image",
      attrs: { src, "data-stored-src": storedSrc, status: "uploaded" },
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
  const attrs = editor.getAttributes("pageColor") as { textColor?: unknown; backgroundColor?: unknown };
  if (attrs.textColor === undefined && attrs.backgroundColor === undefined) return null;
  return attrs;
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
    insertImage: (src, storedSrc) => runInsertImage(editor, src, storedSrc),
    focusStart: () => editor.commands.focus(PAGE_BODY_START),
    readState: () => editorFormattingState(editor),
    replaceBody: (html) => replacePageBody(editor, html),
  };
}

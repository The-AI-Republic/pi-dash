// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page editor formatting state (UI kit and editor; parity/specs/pages.md
// §5.3, PAGE-028–032). Tiptap is reached only through import(), so this
// module knows the editor by the small surface it exposes: `isActive`
// plus the caret's block. Reading the state and applying a command both
// work against that surface, which keeps the toolbar's rules testable
// without a browser and keeps the heavy editor out of every importer.

/** The block kinds the typography selector covers. */
export type PageBlockKind = "paragraph" | `heading${1 | 2 | 3 | 4 | 5 | 6}`;

/** Every formatting command the page bar can fire. */
export type PageFormatCommand =
  | "bold"
  | "italic"
  | "underline"
  | "strike"
  | "alignLeft"
  | "alignCenter"
  | "alignRight"
  | "orderedList"
  | "bulletList"
  | "taskList"
  | "blockquote"
  | "code";

/** Alignment an alignment command (or the caret's block) stands for. */
export type PageAlignment = "left" | "center" | "right";

/** Coverable list kinds; inline code and quotes are not lists. */
export type PageListKind = "ordered" | "bullet" | "task";

/** One formatting state at the caret or over the selection. */
export interface PageFormattingState {
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strike: boolean;
  align: PageAlignment;
  list: PageListKind | null;
  blockquote: boolean;
  code: boolean;
  /** Block kind under the caret; null when the selection spans blocks. */
  block: PageBlockKind | null;
  /** Stored letter colour key of the range, or null for plain text. */
  textColor: string | null;
  /** Stored wash colour key of the range, or null for no highlight. */
  backgroundColor: string | null;
}

/** The state a bar holds before any editor reports one. */
export const EMPTY_FORMATTING_STATE: PageFormattingState = {
  bold: false,
  italic: false,
  underline: false,
  strike: false,
  align: "left",
  list: null,
  blockquote: false,
  code: false,
  block: "paragraph",
  textColor: null,
  backgroundColor: null,
};

/** What the reader needs from the editor: attribute probing plus the caret. */
export interface PageEditorProbe {
  /** Tiptap's `isActive`, by mark or node name and optional attributes. */
  isActive: (name: string, attributes?: Record<string, unknown>) => boolean;
  /** The text block holding the caret (start of the selection). */
  caretBlock: () => { type: string; attrs: Record<string, unknown> } | null;
  /** True while the caret is inside a task list item. */
  inTaskList: () => boolean;
  /** The colour mark's attributes over the caret/selection, if any. */
  colorAttrs?: () => { textColor?: unknown; backgroundColor?: unknown } | null;
}

/** A stored heading level clamped to the six the selector offers. */
export function blockKindForHeading(level: number): PageBlockKind {
  const clamped = Math.min(6, Math.max(1, Math.trunc(level)));
  return `heading${clamped}` as PageBlockKind;
}

/** The heading level a block kind stands for, or null for plain text. */
export function headingLevelOf(kind: PageBlockKind): number | null {
  return kind === "paragraph" ? null : Number(kind.slice("heading".length));
}

/** Alignment from a stored text-align attribute, defaulting to left. */
export function alignmentOf(value: unknown): PageAlignment {
  return value === "center" || value === "right" ? value : "left";
}

/** The state the editor reports for the caret or selection. */
export function readFormattingState(probe: PageEditorProbe): PageFormattingState {
  const block = probe.caretBlock();
  const alignValue = block?.attrs["textAlign"];
  return {
    bold: probe.isActive("bold"),
    italic: probe.isActive("italic"),
    underline: probe.isActive("underline"),
    strike: probe.isActive("strike"),
    align: alignmentOf(alignValue),
    list: probe.inTaskList()
      ? "task"
      : probe.isActive("bulletList")
        ? "bullet"
        : probe.isActive("orderedList")
          ? "ordered"
          : null,
    blockquote: probe.isActive("blockquote"),
    code: probe.isActive("code"),
    block: block?.type === "heading" ? blockKindForHeading(Number(block.attrs["level"] ?? 1)) : "paragraph",
    textColor: colorKey(probe.colorAttrs?.()?.textColor),
    backgroundColor: colorKey(probe.colorAttrs?.()?.backgroundColor),
  };
}

/** A stored colour attribute read as a key, or null when it carries none. */
function colorKey(value: unknown): string | null {
  return typeof value === "string" && value !== "" ? value : null;
}

/** Two states are the same; the bar skips a repaint when they match. */
export function formattingStatesEqual(left: PageFormattingState, right: PageFormattingState): boolean {
  return (
    left.bold === right.bold &&
    left.italic === right.italic &&
    left.underline === right.underline &&
    left.strike === right.strike &&
    left.align === right.align &&
    left.list === right.list &&
    left.blockquote === right.blockquote &&
    left.code === right.code &&
    left.block === right.block &&
    left.textColor === right.textColor &&
    left.backgroundColor === right.backgroundColor
  );
}

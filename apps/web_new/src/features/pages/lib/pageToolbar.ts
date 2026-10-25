// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page formatting bar model (parity/specs/pages.md §5.3, §9.2,
// PAGE-028–032). The bar is described once, as data: every control's
// order, label, key hint and the document command it fires. The React
// component renders this list and the editor adapter runs a command, so
// the order the oracle counts — four marks, three alignments, three
// lists, quote, code — lives in one tested place. The media slice
// (NEWFRONT-841, PAGE-033/034/036) appends its table, image and colour
// entries after these twelve.

import type {
  PageAlignment,
  PageBlockKind,
  PageFormatCommand,
  PageFormattingState,
  PageListKind,
} from "../../../shared/editor/index.js";

/** The plain-text entry; a mixed or empty selection reads as this. */
export const PAGE_BLOCK_FALLBACK = { value: "paragraph" as PageBlockKind, label: "Text" };

/** The block kinds the typography selector offers, in reading order. */
export const PAGE_BLOCK_KINDS: readonly { value: PageBlockKind; label: string }[] = [
  { value: "paragraph", label: "Text" },
  { value: "heading1", label: "Heading 1" },
  { value: "heading2", label: "Heading 2" },
  { value: "heading3", label: "Heading 3" },
  { value: "heading4", label: "Heading 4" },
  { value: "heading5", label: "Heading 5" },
  { value: "heading6", label: "Heading 6" },
];

/** One formatting control: a name, its hint, and the command it fires. */
export interface PageToolbarControl {
  command: PageFormatCommand;
  /** Tooltip name; the oracle reads exactly this. */
  label: string;
  /** Key hint advertised beside the name, or null when there is none. */
  hint: string | null;
}

/**
 * The three media controls this slice appends after the core twelve, in
 * reading order (spec §5.3 items 6–8). They are not all plain format
 * commands — table inserts a node, image opens a picker, colour opens a
 * popover — so the bar renders them through its own media slot rather
 * than the command list above.
 */
export const PAGE_MEDIA_CONTROL_LABELS = {
  table: "Table",
  image: "Image",
  color: "Color",
} as const;

/** The bar's icon-only controls, in reading order (spec §5.3). */
export const PAGE_TOOLBAR_CONTROLS: readonly PageToolbarControl[] = [
  { command: "bold", label: "Bold", hint: "Cmd + B" },
  { command: "italic", label: "Italic", hint: "Cmd + I" },
  { command: "underline", label: "Underline", hint: "Cmd + U" },
  { command: "strike", label: "Strikethrough", hint: "Cmd + Shift + S" },
  { command: "alignLeft", label: "Left align", hint: "Cmd + Shift + L" },
  { command: "alignCenter", label: "Center align", hint: "Cmd + Shift + E" },
  { command: "alignRight", label: "Right align", hint: "Cmd + Shift + R" },
  { command: "orderedList", label: "Numbered list", hint: "Cmd + Shift + 7" },
  { command: "bulletList", label: "Bulleted list", hint: "Cmd + Shift + 8" },
  { command: "taskList", label: "To-do list", hint: "Cmd + Shift + 9" },
  { command: "blockquote", label: "Quote", hint: null },
  { command: "code", label: "Code", hint: null },
];

/** The full tooltip text for one control: its name, plus the hint if any. */
export function toolbarTooltip(control: PageToolbarControl): string {
  return control.hint === null ? control.label : `${control.label} (${control.hint})`;
}

/**
 * The hint as the keys it names, in press order ("Cmd + Shift + 7" ->
 * ["Cmd", "Shift", "7"]), or null for a control with no shortcut. The
 * tooltip draws each key as its own key cap, so the hint reads as keys
 * rather than as prose.
 */
export function toolbarHintKeys(control: PageToolbarControl): string[] | null {
  if (control.hint === null) return null;
  return control.hint.split(" + ");
}

const ALIGNMENT_COMMANDS: Record<PageAlignment, PageFormatCommand> = {
  left: "alignLeft",
  center: "alignCenter",
  right: "alignRight",
};

const LIST_COMMANDS: Record<PageListKind, PageFormatCommand> = {
  ordered: "orderedList",
  bullet: "bulletList",
  task: "taskList",
};

/**
 * Whether one control reads as active for a formatting state. Alignment
 * and lists are exclusive (exactly the current one lights); marks, quote
 * and code light on their own.
 */
export function controlActive(command: PageFormatCommand, state: PageFormattingState): boolean {
  switch (command) {
    case "bold":
      return state.bold;
    case "italic":
      return state.italic;
    case "underline":
      return state.underline;
    case "strike":
      return state.strike;
    case "alignLeft":
    case "alignCenter":
    case "alignRight":
      return ALIGNMENT_COMMANDS[state.align] === command;
    case "orderedList":
    case "bulletList":
    case "taskList":
      return state.list !== null && LIST_COMMANDS[state.list] === command;
    case "blockquote":
      return state.blockquote;
    case "code":
      return state.code;
  }
}

/**
 * One advertised shortcut row of the ten-command map (spec §9.2).
 * `native` says the editor's own keymap already runs this command, so the
 * page's handler must leave the event alone instead of toggling twice.
 */
export interface PageToolbarShortcut {
  key: string;
  shift: boolean;
  command: PageFormatCommand;
  native: boolean;
}

export const PAGE_TOOLBAR_SHORTCUTS: readonly PageToolbarShortcut[] = [
  { key: "b", shift: false, command: "bold", native: true },
  { key: "i", shift: false, command: "italic", native: true },
  { key: "u", shift: false, command: "underline", native: true },
  { key: "s", shift: true, command: "strike", native: true },
  { key: "l", shift: true, command: "alignLeft", native: false },
  { key: "e", shift: true, command: "alignCenter", native: false },
  { key: "r", shift: true, command: "alignRight", native: false },
  { key: "7", shift: true, command: "orderedList", native: true },
  { key: "8", shift: true, command: "bulletList", native: true },
  { key: "9", shift: true, command: "taskList", native: true },
];

/** The shortcut row a keyboard event names, if any. */
export function matchShortcut(input: {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}): PageToolbarShortcut | null {
  if (input.altKey) return null;
  if (!input.metaKey && !input.ctrlKey) return null;
  const key = input.key.toLowerCase();
  return PAGE_TOOLBAR_SHORTCUTS.find((row) => row.key === key && row.shift === input.shiftKey) ?? null;
}

/**
 * The command a keyboard event means inside the body editor. Only the
 * primary modifier (meta on macOS, control elsewhere) counts, the letter
 * matches case-insensitively so a shifted key hits its own row, and an
 * alt chord belongs to the platform. Returns null for anything the
 * editor handles itself.
 */
export function shortcutCommand(input: {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}): PageFormatCommand | null {
  return matchShortcut(input)?.command ?? null;
}

/**
 * The command the page's own key handler should run, or null to leave the
 * event to the editor. Rows the editor already binds return null: running
 * them here as well would toggle the mark straight back off.
 */
export function pageKeyCommand(input: {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}): PageFormatCommand | null {
  const row = matchShortcut(input);
  if (row === null || row.native) return null;
  return row.command;
}

/** Typography selector label for the block under the caret. */
export function blockKindLabel(kind: PageBlockKind | null): string {
  return blockKindOption(kind).label;
}

/** The selector entry for a block kind, or the plain-text entry. */
export function blockKindOption(kind: PageBlockKind | null): { value: PageBlockKind; label: string } {
  if (kind === null) return PAGE_BLOCK_FALLBACK;
  return PAGE_BLOCK_KINDS.find((entry) => entry.value === kind) ?? PAGE_BLOCK_FALLBACK;
}

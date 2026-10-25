// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Formatting bar model (parity/specs/pages.md §5.3, §9.2, PAGE-028–032):
// the control order and tooltips the oracle reads, the active-highlight
// rules, the shortcut map and the typography labels.

import { describe, expect, it } from "vitest";

import { EMPTY_FORMATTING_STATE, type PageFormattingState } from "../../../shared/editor/index.js";
import {
  blockKindLabel,
  blockKindOption,
  controlActive,
  PAGE_BLOCK_KINDS,
  PAGE_TOOLBAR_CONTROLS,
  PAGE_TOOLBAR_SHORTCUTS,
  pageKeyCommand,
  shortcutCommand,
  toolbarHintKeys,
  toolbarTooltip,
} from "./pageToolbar.js";

function state(overrides: Partial<PageFormattingState> = {}): PageFormattingState {
  return { ...EMPTY_FORMATTING_STATE, ...overrides };
}

describe("toolbar controls", () => {
  it("lists the twelve core controls in the spec's reading order", () => {
    expect(PAGE_TOOLBAR_CONTROLS.map((control) => control.command)).toEqual([
      "bold",
      "italic",
      "underline",
      "strike",
      "alignLeft",
      "alignCenter",
      "alignRight",
      "orderedList",
      "bulletList",
      "taskList",
      "blockquote",
      "code",
    ]);
  });

  it("names every control and its key hint exactly as the oracle reads it", () => {
    expect(PAGE_TOOLBAR_CONTROLS.map((control) => [control.label, control.hint])).toEqual([
      ["Bold", "Cmd + B"],
      ["Italic", "Cmd + I"],
      ["Underline", "Cmd + U"],
      ["Strikethrough", "Cmd + Shift + S"],
      ["Left align", "Cmd + Shift + L"],
      ["Center align", "Cmd + Shift + E"],
      ["Right align", "Cmd + Shift + R"],
      ["Numbered list", "Cmd + Shift + 7"],
      ["Bulleted list", "Cmd + Shift + 8"],
      ["To-do list", "Cmd + Shift + 9"],
      ["Quote", null],
      ["Code", null],
    ]);
  });

  it("appends the hint to the tooltip text only when there is one", () => {
    const control = (command: string) => {
      const found = PAGE_TOOLBAR_CONTROLS.find((entry) => entry.command === command);
      if (found === undefined) throw new Error(`missing ${command}`);
      return found;
    };
    expect(toolbarTooltip(control("bold"))).toBe("Bold (Cmd + B)");
    expect(toolbarTooltip(control("blockquote"))).toBe("Quote");
  });

  it("splits a hint into the keys it names, and has none for quote or code", () => {
    const control = (command: string) => {
      const found = PAGE_TOOLBAR_CONTROLS.find((entry) => entry.command === command);
      if (found === undefined) throw new Error(`missing ${command}`);
      return found;
    };
    expect(toolbarHintKeys(control("bold"))).toEqual(["Cmd", "B"]);
    expect(toolbarHintKeys(control("strike"))).toEqual(["Cmd", "Shift", "S"]);
    expect(toolbarHintKeys(control("code"))).toBeNull();
  });
});

describe("controlActive", () => {
  it("lights each mark on its own", () => {
    const active = controlActive;
    expect(active("bold", state({ bold: true }))).toBe(true);
    expect(active("bold", state({ italic: true }))).toBe(false);
    expect(active("italic", state({ italic: true }))).toBe(true);
    expect(active("underline", state({ underline: true }))).toBe(true);
    expect(active("strike", state({ strike: true }))).toBe(true);
  });

  it("lights exactly one alignment", () => {
    expect(controlActive("alignLeft", state({ align: "left" }))).toBe(true);
    expect(controlActive("alignCenter", state({ align: "left" }))).toBe(false);
    expect(controlActive("alignCenter", state({ align: "center" }))).toBe(true);
    expect(controlActive("alignRight", state({ align: "center" }))).toBe(false);
    expect(controlActive("alignRight", state({ align: "right" }))).toBe(true);
    expect(controlActive("alignLeft", state({ align: "right" }))).toBe(false);
  });

  it("lights exactly one list kind, and none outside a list", () => {
    expect(controlActive("orderedList", state({ list: "ordered" }))).toBe(true);
    expect(controlActive("bulletList", state({ list: "ordered" }))).toBe(false);
    expect(controlActive("bulletList", state({ list: "bullet" }))).toBe(true);
    expect(controlActive("taskList", state({ list: "task" }))).toBe(true);
    expect(controlActive("orderedList", state({ list: null }))).toBe(false);
    expect(controlActive("taskList", state({ list: "bullet" }))).toBe(false);
  });

  it("lights quote and code independently", () => {
    expect(controlActive("blockquote", state({ blockquote: true }))).toBe(true);
    expect(controlActive("blockquote", state({ code: true }))).toBe(false);
    expect(controlActive("code", state({ code: true }))).toBe(true);
    expect(controlActive("code", state({ blockquote: true }))).toBe(false);
  });
});

describe("shortcut map", () => {
  it("advertises the ten commands of the spec's map", () => {
    expect(PAGE_TOOLBAR_SHORTCUTS.map((row) => row.command)).toEqual([
      "bold",
      "italic",
      "underline",
      "strike",
      "alignLeft",
      "alignCenter",
      "alignRight",
      "orderedList",
      "bulletList",
      "taskList",
    ]);
  });

  it("maps the primary modifier and the shift variant", () => {
    expect(shortcutCommand({ key: "b", metaKey: true, ctrlKey: false, altKey: false, shiftKey: false })).toBe("bold");
    expect(shortcutCommand({ key: "B", metaKey: false, ctrlKey: true, altKey: false, shiftKey: false })).toBe("bold");
    expect(shortcutCommand({ key: "S", metaKey: true, ctrlKey: false, altKey: false, shiftKey: true })).toBe("strike");
    expect(shortcutCommand({ key: "7", metaKey: true, ctrlKey: false, altKey: false, shiftKey: true })).toBe(
      "orderedList"
    );
  });

  it("ignores bare keys, alt chords and unlisted combinations", () => {
    const base = { metaKey: false, ctrlKey: false, altKey: false, shiftKey: false };
    expect(shortcutCommand({ ...base, key: "b" })).toBe(null);
    expect(shortcutCommand({ ...base, key: "b", metaKey: true, altKey: true })).toBe(null);
    expect(shortcutCommand({ ...base, key: "q", metaKey: true })).toBe(null);
    // Shift+B is not the bold chord; shift-less S is not strike.
    expect(shortcutCommand({ ...base, key: "b", metaKey: true, shiftKey: true })).toBe(null);
    expect(shortcutCommand({ ...base, key: "s", metaKey: true })).toBe(null);
  });

  it("leaves chords the editor already binds to the editor", () => {
    const base = { metaKey: true, ctrlKey: false, altKey: false, shiftKey: false };
    expect(pageKeyCommand({ ...base, key: "b" })).toBe(null);
    expect(pageKeyCommand({ ...base, key: "i" })).toBe(null);
    expect(pageKeyCommand({ ...base, key: "u" })).toBe(null);
    expect(pageKeyCommand({ ...base, key: "7", shiftKey: true })).toBe(null);
    expect(pageKeyCommand({ ...base, key: "9", shiftKey: true })).toBe(null);
  });

  it("runs the alignment chords itself", () => {
    const base = { metaKey: true, ctrlKey: false, altKey: false, shiftKey: true };
    expect(pageKeyCommand({ ...base, key: "l" })).toBe("alignLeft");
    expect(pageKeyCommand({ ...base, key: "e" })).toBe("alignCenter");
    expect(pageKeyCommand({ ...base, key: "r" })).toBe("alignRight");
    expect(pageKeyCommand({ ...base, key: "q" })).toBe(null);
  });
});

describe("typography", () => {
  it("offers plain text plus six heading levels, in order", () => {
    expect(PAGE_BLOCK_KINDS.map((entry) => entry.label)).toEqual([
      "Text",
      "Heading 1",
      "Heading 2",
      "Heading 3",
      "Heading 4",
      "Heading 5",
      "Heading 6",
    ]);
  });

  it("labels a known block and falls back to Text for a mixed selection", () => {
    expect(blockKindLabel("heading2")).toBe("Heading 2");
    expect(blockKindLabel("paragraph")).toBe("Text");
    expect(blockKindLabel(null)).toBe("Text");
    expect(blockKindOption(null).value).toBe("paragraph");
    expect(blockKindOption("heading6")).toEqual({ value: "heading6", label: "Heading 6" });
  });
});

describe("blockKindOption fallbacks", () => {
  it("returns the plain-text entry for an unknown block value", () => {
    expect(blockKindOption("heading9" as never)).toEqual({ value: "paragraph", label: "Text" });
  });
});

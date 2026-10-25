// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page formatting state (parity/specs/pages.md §5.3, PAGE-028–032): the
// block kind under the caret, its alignment, the list kind and every mark,
// read from the small editor probe the lazy editor exposes.

import { describe, expect, it } from "vitest";

import {
  alignmentOf,
  blockKindForHeading,
  EMPTY_FORMATTING_STATE,
  formattingStatesEqual,
  headingLevelOf,
  readFormattingState,
  type PageEditorProbe,
  type PageFormattingState,
} from "./pageFormatting.js";

function probe(
  overrides: Partial<PageEditorProbe> & {
    block?: { type: string; attrs: Record<string, unknown> } | null;
    active?: string[];
  } = {}
): PageEditorProbe {
  const active = new Set(overrides.active ?? []);
  return {
    isActive: (name, attributes) => {
      if (attributes !== undefined) return false;
      return active.has(name);
    },
    caretBlock: () => (overrides.block === undefined ? { type: "paragraph", attrs: {} } : overrides.block),
    inTaskList: overrides.inTaskList ?? (() => false),
  };
}

describe("heading helpers", () => {
  it("clamps a stored level to the six the selector offers", () => {
    expect(blockKindForHeading(1)).toBe("heading1");
    expect(blockKindForHeading(6)).toBe("heading6");
    expect(blockKindForHeading(0)).toBe("heading1");
    expect(blockKindForHeading(-3)).toBe("heading1");
    expect(blockKindForHeading(9)).toBe("heading6");
    expect(blockKindForHeading(2.9)).toBe("heading2");
  });

  it("reads a level back out of a block kind", () => {
    expect(headingLevelOf("paragraph")).toBe(null);
    expect(headingLevelOf("heading1")).toBe(1);
    expect(headingLevelOf("heading6")).toBe(6);
  });

  it("reads alignment from a stored attribute, defaulting to left", () => {
    expect(alignmentOf("center")).toBe("center");
    expect(alignmentOf("right")).toBe("right");
    expect(alignmentOf("left")).toBe("left");
    expect(alignmentOf(undefined)).toBe("left");
    expect(alignmentOf("justify")).toBe("left");
    expect(alignmentOf(3)).toBe("left");
  });
});

describe("readFormattingState", () => {
  it("starts from a plain paragraph with every mark off", () => {
    expect(readFormattingState(probe())).toEqual(EMPTY_FORMATTING_STATE);
  });

  it("reports each mark the probe has active", () => {
    const state = readFormattingState(probe({ active: ["bold", "italic", "underline", "strike", "code"] }));
    expect(state).toMatchObject({
      bold: true,
      italic: true,
      underline: true,
      strike: true,
      code: true,
      blockquote: false,
    });
  });

  it("reads the caret's heading level and alignment", () => {
    const state = readFormattingState(probe({ block: { type: "heading", attrs: { level: 3, textAlign: "center" } } }));
    expect(state.block).toBe("heading3");
    expect(state.align).toBe("center");
  });

  it("clamps an out-of-range or missing heading level", () => {
    expect(readFormattingState(probe({ block: { type: "heading", attrs: { level: 12 } } })).block).toBe("heading6");
    expect(readFormattingState(probe({ block: { type: "heading", attrs: {} } })).block).toBe("heading1");
  });

  it("treats any non-heading block, or no block, as plain text", () => {
    expect(readFormattingState(probe({ block: { type: "listItem", attrs: {} } })).block).toBe("paragraph");
    expect(readFormattingState(probe({ block: null })).block).toBe("paragraph");
    expect(readFormattingState(probe({ block: null })).align).toBe("left");
  });

  it("prefers the task list, then bullet, then ordered", () => {
    expect(readFormattingState(probe({ active: ["orderedList"] })).list).toBe("ordered");
    expect(readFormattingState(probe({ active: ["orderedList", "bulletList"] })).list).toBe("bullet");
    expect(readFormattingState(probe({ active: ["orderedList", "bulletList"], inTaskList: () => true })).list).toBe(
      "task"
    );
    expect(readFormattingState(probe()).list).toBe(null);
  });

  it("reads a quote from the blockquote mark", () => {
    expect(readFormattingState(probe({ active: ["blockquote"] })).blockquote).toBe(true);
  });
});

describe("formattingStatesEqual", () => {
  const base: PageFormattingState = { ...EMPTY_FORMATTING_STATE };

  it("holds for identical states", () => {
    expect(formattingStatesEqual(base, { ...base })).toBe(true);
  });

  it("notices every field", () => {
    const changes: Partial<PageFormattingState>[] = [
      { bold: true },
      { italic: true },
      { underline: true },
      { strike: true },
      { align: "right" },
      { list: "task" },
      { blockquote: true },
      { code: true },
      { block: "heading4" },
    ];
    for (const change of changes) {
      expect(formattingStatesEqual(base, { ...base, ...change })).toBe(false);
    }
  });
});

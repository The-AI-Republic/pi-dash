// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { chatInputDoc, chatInputKeyAction } from "./chatInputText.js";

const plain = { shiftKey: false, ctrlKey: false, metaKey: false, altKey: false };

describe("chatInputDoc", () => {
  it("holds a draft as one paragraph with hard breaks for newlines", () => {
    expect(chatInputDoc("a\nb")).toEqual({
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [{ type: "text", text: "a" }, { type: "hardBreak" }, { type: "text", text: "b" }],
        },
      ],
    });
  });

  it("never emits empty text nodes", () => {
    expect(chatInputDoc("")).toEqual({ type: "doc", content: [{ type: "paragraph", content: [] }] });
    expect(chatInputDoc("\n\nx\n")[`content`][0].content).toEqual([
      { type: "hardBreak" },
      { type: "hardBreak" },
      { type: "text", text: "x" },
      { type: "hardBreak" },
    ]);
  });
});

describe("chatInputKeyAction", () => {
  it("submits on an unmodified Enter only", () => {
    expect(chatInputKeyAction({ key: "Enter", ...plain })).toBe("submit");
    expect(chatInputKeyAction({ key: "Enter", ...plain, shiftKey: true })).toBe("newline");
    expect(chatInputKeyAction({ key: "a", ...plain })).toBe("pass");
  });

  it("swallows every Enter chord, with or without Shift", () => {
    for (const chord of [{ ctrlKey: true }, { metaKey: true }, { altKey: true }]) {
      expect(chatInputKeyAction({ key: "Enter", ...plain, ...chord })).toBe("swallow");
      expect(chatInputKeyAction({ key: "Enter", ...plain, ...chord, shiftKey: true })).toBe("swallow");
    }
  });

  it("leaves Enter to an open IME composition", () => {
    expect(chatInputKeyAction({ key: "Enter", ...plain, isComposing: true })).toBe("pass");
  });
});

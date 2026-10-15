// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { formatSequenceKeys, isTypingTarget } from "./shortcuts.js";

describe("shortcut presentation", () => {
  it("renders sequences as uppercase chips", () => {
    expect(formatSequenceKeys("op")).toEqual(["O", "P"]);
  });
});

describe("typing guard", () => {
  it("silences global bindings while typing", () => {
    expect(isTypingTarget(document.createElement("input"))).toBe(true);
    expect(isTypingTarget(document.createElement("textarea"))).toBe(true);
    expect(isTypingTarget(document.createElement("select"))).toBe(true);
    const editor = document.createElement("div");
    editor.classList.add("ProseMirror");
    expect(isTypingTarget(editor)).toBe(true);
    const editable = document.createElement("div");
    editable.setAttribute("contenteditable", "true");
    expect(isTypingTarget(editable)).toBe(true);
  });

  it("lets bindings fire outside typing surfaces", () => {
    expect(isTypingTarget(null)).toBe(false);
    expect(isTypingTarget(document.createElement("div"))).toBe(false);
    expect(isTypingTarget(document.createElement("button"))).toBe(false);
    expect(isTypingTarget("not-an-element" as unknown as EventTarget)).toBe(false);
    expect(isTypingTarget({} as EventTarget)).toBe(false);
  });

  it("honours the contenteditable attribute without the DOM flag", () => {
    expect(isTypingTarget({ getAttribute: () => "true" } as unknown as EventTarget)).toBe(true);
    expect(isTypingTarget({ tagName: "DIV", getAttribute: () => null } as unknown as EventTarget)).toBe(false);
  });
});

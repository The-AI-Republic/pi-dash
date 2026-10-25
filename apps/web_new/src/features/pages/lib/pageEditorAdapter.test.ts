// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editor command adapter (parity/specs/pages.md §5.3, PAGE-026–032): each
// toolbar command's chain call, the typography restyle, the caret jump to
// the body start, the formatting state the bar reads back, and the whole
// body replacement the version restore writes (PAGE-053).

import { describe, expect, it } from "vitest";

import {
  caretBlockOf,
  editorFormattingState,
  pageDocumentEmpty,
  pageEditorHandle,
  PAGE_BODY_START,
  replacePageBody,
  runBlockKind,
  runPageCommand,
  type PageEditorChain,
  type PageEditorRef,
  type PageFocusPosition,
} from "./pageEditorAdapter.js";

/** A chain that records every call and returns itself. */
function chainSpy(): { chain: PageEditorChain; calls: string[] } {
  const calls: string[] = [];
  const chain = new Proxy({} as PageEditorChain, {
    get: (_target, property: string) => {
      if (property === "run") {
        return () => {
          calls.push("run");
          return true;
        };
      }
      return (...args: unknown[]) => {
        const arg = args[0];
        const shown =
          typeof arg === "object" && arg !== null && "level" in arg
            ? String((arg as { level: unknown }).level)
            : String(arg);
        // A second argument is the emit-update switch of setContent; it
        // is part of what the command promises, so it is recorded too.
        const extra = args.length > 1 ? `, ${String(args[1])}` : "";
        calls.push(args.length === 0 ? property : `${property}(${shown}${extra})`);
        return chain;
      };
    },
  });
  return { chain, calls };
}

function editor(
  options: {
    html?: string;
    active?: string[];
    block?: { type: string; attrs: Record<string, unknown> };
    focus?: (position?: PageFocusPosition) => void;
  } = {}
): {
  ref: PageEditorRef;
  calls: string[];
  focused: (PageFocusPosition | -1)[];
} {
  const { chain, calls } = chainSpy();
  const active = new Set(options.active ?? []);
  const focused: (PageFocusPosition | -1)[] = [];
  const ref: PageEditorRef = {
    chain: () => chain,
    isActive: (name, attributes) => attributes === undefined && active.has(name),
    getHTML: () => options.html ?? "<p></p>",
    commands: {
      focus: (position) => {
        focused.push(position ?? -1);
        options.focus?.(position);
      },
    },
    state: {
      selection: {
        $from: {
          parent: {
            type: { name: options.block?.type ?? "paragraph" },
            attrs: options.block?.attrs ?? {},
            textContent: "",
          },
        },
      },
    },
  };
  return { ref, calls, focused };
}

describe("runPageCommand", () => {
  const cases: { command: Parameters<typeof runPageCommand>[1]; expected: string }[] = [
    { command: "bold", expected: "toggleBold" },
    { command: "italic", expected: "toggleItalic" },
    { command: "underline", expected: "toggleUnderline" },
    { command: "strike", expected: "toggleStrike" },
    { command: "blockquote", expected: "toggleBlockquote" },
    { command: "code", expected: "toggleCode" },
  ];

  it.each(cases)("focuses first and runs $command through $expected", ({ command, expected }) => {
    const { ref, calls } = editor();
    runPageCommand(ref, command);
    expect(calls).toEqual(["focus", expected, "run"]);
  });

  it.each([
    ["alignLeft", "setTextAlign(left)"],
    ["alignCenter", "setTextAlign(center)"],
    ["alignRight", "setTextAlign(right)"],
  ] as const)("aligns with %s", (command, expected) => {
    const { ref, calls } = editor();
    runPageCommand(ref, command);
    expect(calls).toEqual(["focus", expected, "run"]);
  });

  it.each([
    ["orderedList", "toggleOrderedList"],
    ["bulletList", "toggleBulletList"],
    ["taskList", "toggleTaskList"],
  ] as const)("converts to the %s list kind", (command, expected) => {
    const { ref, calls } = editor();
    runPageCommand(ref, command);
    expect(calls).toEqual(["focus", expected, "run"]);
  });
});

describe("runBlockKind", () => {
  it("returns a heading to plain text", () => {
    const { ref, calls } = editor();
    runBlockKind(ref, "paragraph");
    expect(calls).toEqual(["focus", "setParagraph", "run"]);
  });

  it("restyles the block as the chosen heading level", () => {
    const { ref, calls } = editor();
    runBlockKind(ref, "heading4");
    expect(calls).toEqual(["focus", "toggleHeading(4)", "run"]);
  });
});

describe("editor reads", () => {
  it("reports the text block under the caret", () => {
    const { ref } = editor({ block: { type: "heading", attrs: { level: 2 } } });
    expect(caretBlockOf(ref)).toEqual({ type: "heading", attrs: { level: 2 } });
  });

  it("reports the formatting state at the caret", () => {
    const { ref } = editor({ active: ["bold", "taskList"], block: { type: "heading", attrs: { level: 2 } } });
    expect(editorFormattingState(ref)).toMatchObject({ bold: true, list: "task", block: "heading2" });
  });

  it("calls the document empty only when it holds no text", () => {
    expect(pageDocumentEmpty(editor({ html: "<p></p>" }).ref)).toBe(true);
    expect(pageDocumentEmpty(editor({ html: "<p>   </p>" }).ref)).toBe(true);
    expect(pageDocumentEmpty(editor({ html: "<h2>Text</h2>" }).ref)).toBe(false);
  });
});

describe("pageEditorHandle", () => {
  it("runs commands and block kinds through the live editor", () => {
    const { ref, calls } = editor();
    const handle = pageEditorHandle(ref);
    handle.applyCommand("bold");
    handle.applyBlockKind("heading1");
    expect(calls).toEqual(["focus", "toggleBold", "run", "focus", "toggleHeading(1)", "run"]);
  });

  it("puts the caret at the body's very start", () => {
    const { ref, focused } = editor();
    pageEditorHandle(ref).focusStart();
    // The editor's own `start` keyword, never position 0: Tiptap reads a
    // falsy position as "none given" and would keep the old caret.
    expect(PAGE_BODY_START).toBe("start");
    expect(focused).toEqual(["start"]);
  });

  it("writes a whole body through the chain, emitting the update", () => {
    const { ref, calls } = editor();
    replacePageBody(ref, "<p>Restored</p>");
    expect(calls).toEqual(["setContent(<p>Restored</p>, true)", "run"]);
  });

  it("exposes the body replacement on the handle the surface holds", () => {
    const { ref, calls } = editor();
    pageEditorHandle(ref).replaceBody("<p>From the snapshot</p>");
    expect(calls).toEqual(["setContent(<p>From the snapshot</p>, true)", "run"]);
  });

  it("reads the live formatting state", () => {
    const { ref } = editor({ active: ["italic"] });
    expect(pageEditorHandle(ref).readState()).toMatchObject({ italic: true });
  });
});

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editor command adapter (parity/specs/pages.md §5.3, PAGE-026–032): each
// toolbar command's chain call, the typography restyle, the caret jump to
// the body start, the formatting state the bar reads back, and the whole
// body replacement the version restore writes (PAGE-053).

import { describe, expect, it } from "vitest";

import {
  applyPageColor,
  caretBlockOf,
  deleteContentBeforeCaret,
  deletePageBlock,
  duplicatePageBlock,
  movePageBlock,
  pageBlockAtPoint,
  insertPageEmoji,
  insertPageEmbed,
  insertPageEntityReference,
  linkSelection,
  pageTextBeforeCaret,
  selectedPageText,
  runCatalogBlock,
  unlinkSelection,
  clearPageColor,
  editorFormattingState,
  insertPageTable,
  PAGE_COLOR_MARK,
  pageDocumentEmpty,
  pageEditorHandle,
  PAGE_BODY_START,
  replacePageBody,
  runBlockKind,
  runInsertImage,
  runPageCommand,
  type PageEditorChain,
  type PageEditorRef,
  type PageFocusPosition,
} from "./pageEditorAdapter.js";

/** A chain that records every call and returns itself. */
function chainSpy(runResult = true): { chain: PageEditorChain; calls: string[] } {
  const calls: string[] = [];
  const chain = new Proxy({} as PageEditorChain, {
    get: (_target, property: string) => {
      if (property === "run") {
        return () => {
          calls.push("run");
          return runResult;
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
    runResult?: boolean;
    markAttrs?: Record<string, unknown>;
  } = {}
): {
  ref: PageEditorRef;
  calls: string[];
  focused: (PageFocusPosition | -1)[];
} {
  const { chain, calls } = chainSpy(options.runResult ?? true);
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
    ...(options.markAttrs === undefined ? {} : { getAttributes: () => options.markAttrs ?? {} }),
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
    expect(replacePageBody(ref, "<p>Restored</p>")).toBe(true);
    expect(calls).toEqual(["setContent(<p>Restored</p>, true)", "run"]);
  });

  it("exposes the body replacement on the handle the surface holds", () => {
    const { ref, calls } = editor();
    expect(pageEditorHandle(ref).replaceBody("<p>From the snapshot</p>")).toBe(true);
    expect(calls).toEqual(["setContent(<p>From the snapshot</p>, true)", "run"]);
  });

  it("answers the command's refusal instead of a write that never happened", () => {
    const { ref } = editor({ runResult: false });
    expect(pageEditorHandle(ref).replaceBody("<p>From the snapshot</p>")).toBe(false);
  });

  it("forwards catalogue blocks, emoji, mentions, embeds and blocks through the handle", () => {
    const { ref, calls } = editor();
    const handle = pageEditorHandle(ref);
    handle.applyCatalogBlock("code");
    handle.insertEmoji("🎉");
    handle.insertMention({ id: "u-1", name: "Ada" });
    handle.insertEmbed({
      id: "a",
      entity_identifier: "ISS-1",
      project_identifier: "P",
      workspace_identifier: "ws",
      entity_name: "Named",
    });
    handle.duplicateBlock();
    handle.deleteBlock();
    expect(calls.some((call) => call.startsWith("insertContent("))).toBe(true);
    expect(calls).toContain("run");
  });

  it("forwards link writes, the body HTML and the caret jumps", () => {
    const { ref, calls, focused } = editor();
    const handle = pageEditorHandle(ref);
    handle.setLink("https://example.com");
    handle.unsetLink();
    expect(handle.html()).toBe("<p></p>");
    handle.focusEnd();
    handle.deleteBeforeCaret(2);
    handle.selectedText();
    expect(focused).toEqual(["end"]);
    expect(calls).toContain("setLink([object Object])");
    expect(calls).toContain("unsetLink");
  });

  it("reports a selection only when the range is non-empty", () => {
    const base = editor();
    const empty = pageEditorHandle(base.ref);
    expect(empty.hasSelection()).toBe(false);
    const selected = {
      ...base.ref,
      state: { selection: { from: 2, to: 5, $from: base.ref.state.selection.$from } },
    };
    expect(pageEditorHandle(selected).hasSelection()).toBe(true);
  });

  it("reads the live formatting state", () => {
    const { ref } = editor({ active: ["italic"] });
    expect(pageEditorHandle(ref).readState()).toMatchObject({ italic: true });
  });
});

describe("table and image commands", () => {
  it("focuses first and drops the toolbar's 3×3 table", () => {
    const { ref, calls } = editor();
    insertPageTable(ref);
    expect(calls).toEqual(["focus", "insertPageTable([object Object])", "run"]);
  });

  it("inserts an uploaded picture with its stored id and status", () => {
    const { ref, calls } = editor();
    runInsertImage(ref, "/api/assets/v2/workspaces/acme/projects/p/a/", "a");
    expect(calls[0]).toBe("focus");
    expect(calls[1]).toBe("insertContent([object Object])");
    expect(calls[2]).toBe("run");
  });

  it("exposes table and image insertion on the handle", () => {
    const { ref, calls } = editor();
    const handle = pageEditorHandle(ref);
    handle.insertTable();
    handle.insertImage("/api/assets/a/", "a");
    expect(calls).toEqual([
      "focus",
      "insertPageTable([object Object])",
      "run",
      "focus",
      "insertContent([object Object])",
      "run",
    ]);
  });

  it("inserts plain text and an entity reference through the handle", () => {
    const { ref, calls } = editor();
    const handle = pageEditorHandle(ref);
    handle.insertText("@");
    handle.insertEntityReference({
      entity_name: "issue",
      entity_identifier: "ISS-2",
      project_identifier: "P",
      workspace_identifier: "ws",
      "data-label": "Fix login",
    });
    expect(calls).toEqual(["focus", "insertContent(@)", "run", "focus", "insertContent([object Object])", "run"]);
  });
});

describe("colour commands", () => {
  it("writes both attributes so the untouched kind survives", () => {
    const { ref, calls } = editor({ markAttrs: { textColor: "pink", backgroundColor: null } });
    applyPageColor(ref, "background", "green");
    expect(calls[0]).toBe("focus");
    expect(calls[1]).toBe(`setMark(${PAGE_COLOR_MARK}, [object Object])`);
    expect(calls[2]).toBe("run");
  });

  it("clears one kind and keeps the other", () => {
    const { ref, calls } = editor({ markAttrs: { textColor: "pink", backgroundColor: "green" } });
    clearPageColor(ref, "text");
    expect(calls).toEqual(["focus", `setMark(${PAGE_COLOR_MARK}, [object Object])`, "run"]);
  });

  it("drops the mark when the last kind is cleared", () => {
    const { ref, calls } = editor({ markAttrs: { textColor: "pink", backgroundColor: null } });
    clearPageColor(ref, "text");
    expect(calls).toEqual(["focus", `unsetMark(${PAGE_COLOR_MARK})`, "run"]);
  });

  it("exposes both colour actions on the handle", () => {
    const { ref, calls } = editor({ markAttrs: { textColor: null, backgroundColor: null } });
    const handle = pageEditorHandle(ref);
    handle.applyColor("text", "gray");
    handle.clearColor("text");
    expect(calls).toEqual([
      "focus",
      `setMark(${PAGE_COLOR_MARK}, [object Object])`,
      "run",
      "focus",
      `unsetMark(${PAGE_COLOR_MARK})`,
      "run",
    ]);
  });

  it("reads the stored colour pair into the formatting state", () => {
    const { ref } = editor({ markAttrs: { textColor: "pink", backgroundColor: "green" } });
    expect(editorFormattingState(ref)).toMatchObject({ textColor: "pink", backgroundColor: "green" });
  });

  it("reports no colour when the probe has no attributes", () => {
    const { ref } = editor();
    expect(editorFormattingState(ref)).toMatchObject({ textColor: null, backgroundColor: null });
  });

  it("writes a colour even when the editor cannot report its attributes", () => {
    const { ref, calls } = editor();
    applyPageColor(ref, "text", "gray");
    expect(calls).toEqual(["focus", `setMark(${PAGE_COLOR_MARK}, [object Object])`, "run"]);
  });
});

describe("page block catalogue commands", () => {
  it("restyles typography entries through the block path", () => {
    const { ref, calls } = editor();
    runCatalogBlock(ref, "heading3");
    expect(calls).toEqual(["focus", "toggleHeading(3)", "run"]);
    const text = editor();
    runCatalogBlock(text.ref, "text");
    expect(text.calls).toEqual(["focus", "setParagraph", "run"]);
  });

  it.each([
    ["numbered", "toggleOrderedList"],
    ["bulleted", "toggleBulletList"],
    ["todo", "toggleTaskList"],
    ["quote", "toggleBlockquote"],
  ] as const)("runs %s through its own command", (block, command) => {
    const { ref, calls } = editor();
    runCatalogBlock(ref, block);
    expect(calls).toEqual(["focus", command, "run"]);
  });

  it("inserts a table, a fenced code block, a callout and a divider", () => {
    const table = editor();
    runCatalogBlock(table.ref, "table");
    expect(table.calls).toEqual(["focus", "insertPageTable([object Object])", "run"]);

    const code = editor();
    runCatalogBlock(code.ref, "code");
    expect(code.calls).toEqual(["focus", "insertContent([object Object])", "run"]);

    const callout = editor();
    runCatalogBlock(callout.ref, "callout");
    expect(callout.calls).toEqual(["focus", "insertContent([object Object])", "run"]);

    const divider = editor();
    runCatalogBlock(divider.ref, "divider");
    expect(divider.calls[1]).toBe("insertContent([object Object])");
  });

  it("leaves the image and emoji entries to the interaction layer", () => {
    for (const block of ["image", "emoji", "embed", "entity"] as const) {
      const { ref, calls } = editor();
      runCatalogBlock(ref, block);
      expect(calls).toEqual([]);
    }
  });

  it("inserts a body emoji as marked inline markup", () => {
    const { ref, calls } = editor();
    insertPageEmoji(ref, "🎉");
    expect(calls).toEqual(["focus", 'insertContent(<span data-type="emoji">🎉</span>)', "run"]);
  });

  it("inserts an embed as the stored element", () => {
    const { ref, calls } = editor();
    insertPageEmbed(ref, {
      id: "a",
      entity_identifier: "ISS-1",
      project_identifier: "P",
      workspace_identifier: "ws",
      entity_name: "Named",
    });
    expect(calls[0]).toBe("focus");
    expect(calls[1]).toContain("<issue-embed-component");
    expect(calls[1]).toContain('entity_identifier="ISS-1"');
    expect(calls[2]).toBe("run");
  });

  it("inserts a normalised entity reference node", () => {
    const { ref, calls } = editor();
    insertPageEntityReference(ref, {
      entity_name: "issue",
      entity_identifier: "ISS-2",
      project_identifier: "P",
      workspace_identifier: "ws",
      project_id: "p-1",
      "data-label": "Fix login",
    });
    expect(calls[0]).toBe("focus");
    expect(calls[1]).toBe("insertContent([object Object])");
    expect(calls[2]).toBe("run");
  });

  it("sets and removes a link on the selection", () => {
    const link = editor();
    linkSelection(link.ref, "https://example.com");
    expect(link.calls).toEqual(["focus", "setLink([object Object])", "run"]);
    const unlink = editor();
    unlinkSelection(unlink.ref);
    expect(unlink.calls).toEqual(["focus", "unsetLink", "run"]);
  });

  it("deletes the typed trigger immediately before the caret", () => {
    const { ref, calls } = editorAt({ from: 6, parentOffset: 4, start: 2 });
    deleteContentBeforeCaret(ref, 3);
    expect(calls).toEqual(["focus", "deleteRange([object Object])", "run"]);
  });
});

/** An editor whose caret sits at a known document position. */
function editorAt(selection: { from: number; start: number; parentOffset: number }): {
  ref: PageEditorRef;
  calls: string[];
} {
  const base = editor();
  const ref: PageEditorRef = {
    ...base.ref,
    state: {
      selection: {
        from: selection.from,
        to: selection.from,
        $from: {
          parent: { type: { name: "paragraph" }, attrs: {}, textContent: "abcd" },
          start: () => selection.start,
          parentOffset: selection.parentOffset,
        },
      },
    },
  };
  return { ref, calls: base.calls };
}

describe("block duplication and deletion", () => {
  it("duplicates the top-level block after itself", () => {
    const { ref, calls } = editorBlock();
    duplicatePageBlock(ref);
    expect(calls).toEqual(["focus", "insertContentAt(12, [object Object])", "run"]);
  });

  it("deletes the top-level block's range", () => {
    const { ref, calls } = editorBlock();
    deletePageBlock(ref);
    expect(calls).toEqual(["focus", "deleteRange([object Object])", "run"]);
  });

  it("declines when the caret is not inside a block", () => {
    const { ref, calls } = editor();
    duplicatePageBlock(ref);
    deletePageBlock(ref);
    expect(calls).toEqual([]);
  });
});

function editorBlock(): { ref: PageEditorRef; calls: string[] } {
  const base = editor();
  const ref: PageEditorRef = {
    ...base.ref,
    state: {
      selection: {
        $from: {
          depth: 1,
          parent: { type: { name: "paragraph" }, attrs: {}, textContent: "block" },
          before: () => 4,
          after: () => 12,
          node: () => ({ nodeSize: 8, toJSON: () => ({ type: "paragraph" }) }),
        },
      },
    },
  };
  return { ref, calls: base.calls };
}

describe("selection read fallbacks", () => {
  it("falls back to an empty block text when the block has none", () => {
    const base = editor();
    const ref: PageEditorRef = {
      ...base.ref,
      state: {
        selection: {
          from: 1,
          to: 1,
          $from: { parent: { type: { name: "paragraph" }, attrs: {}, textContent: "" } },
        },
      },
    };
    expect(selectedPageText(ref)).toBe("");
  });

  it("falls back when a selection has no textBetween probe", () => {
    const base = editor();
    const ref: PageEditorRef = {
      ...base.ref,
      state: {
        selection: {
          from: 2,
          to: 4,
          $from: { parent: { type: { name: "paragraph" }, attrs: {}, textContent: "whole" } },
        },
      },
    };
    expect(selectedPageText(ref)).toBe("whole");
  });
});

describe("link and selection reads", () => {
  it("reads the link address from the active mark", () => {
    const { ref } = editor({ active: ["link"], markAttrs: { href: "https://example.com" } });
    expect(pageEditorHandle(ref).linkHref()).toBe("https://example.com");
  });

  it("answers null without a link mark or without attribute support", () => {
    expect(pageEditorHandle(editor().ref).linkHref()).toBeNull();
    const { ref } = editor({ active: ["link"] });
    expect(pageEditorHandle(ref).linkHref()).toBeNull();
    const { ref: withEmpty } = editor({ active: ["link"], markAttrs: { href: "" } });
    expect(pageEditorHandle(withEmpty).linkHref()).toBeNull();
  });

  it("reads only the selected range for the prefill", () => {
    const base = editor();
    const ref: PageEditorRef = {
      ...base.ref,
      state: {
        selection: {
          from: 2,
          to: 5,
          $from: {
            parent: { type: { name: "paragraph" }, attrs: {}, textContent: "abcdef" },
            textBetween: () => "bcd",
          },
        },
      },
    };
    expect(selectedPageText(ref)).toBe("bcd");
  });

  it("falls back to the caret's block text when nothing is selected", () => {
    const base = editor();
    const ref: PageEditorRef = {
      ...base.ref,
      state: {
        selection: {
          from: 2,
          to: 2,
          $from: { parent: { type: { name: "paragraph" }, attrs: {}, textContent: "block text" } },
        },
      },
    };
    expect(selectedPageText(ref)).toBe("block text");
  });

  it("reads text before the caret and an empty string without a range", () => {
    const base = editor();
    const ref: PageEditorRef = {
      ...base.ref,
      state: {
        selection: {
          $from: {
            parent: { type: { name: "paragraph" }, attrs: {}, textContent: "ab/cd" },
            parentOffset: 5,
            textBetween: () => "ab/cd",
          },
        },
      },
    };
    expect(pageTextBeforeCaret(ref)).toBe("ab/cd");
    expect(pageTextBeforeCaret(editor().ref)).toBe("");
  });

  it("inserts a picture with the alt text the assets pane reads (PAGE-039)", () => {
    const { ref, calls } = editor();
    runInsertImage(ref, "/api/assets/a/", "a", "image-a");
    expect(calls).toEqual(["focus", "insertContent([object Object])", "run"]);
  });
});

/** A document half-probe over a small fixed block list (PAGE-037 moves). */
function documentEditor(blocks: { type: string; text?: string }[]) {
  const { chain, calls } = chainSpy();
  const children = blocks.map((block) => ({
    nodeSize: 12,
    type: { name: block.type },
    toJSON: () => ({ type: block.type, content: [{ type: "text", text: block.text ?? "" }] }),
  }));
  const doc = {
    childCount: children.length,
    child: (index: number) => {
      const child = children[index];
      if (child === undefined) throw new Error("no such block");
      return child;
    },
    resolve: () => ({}),
  };
  const ref: PageEditorRef = {
    chain: () => chain,
    isActive: () => false,
    getHTML: () => "",
    commands: { focus: () => undefined },
    view: {
      dom: { getBoundingClientRect: () => ({ top: 0 }), contains: () => true },
      posAtCoords: ({ left }) => ({ pos: left }),
      coordsAtPos: (position) => ({ top: position }),
      state: { doc },
    },
    state: {
      selection: {
        from: 0,
        to: 0,
        $from: { depth: 0, parent: { type: { name: "doc" }, attrs: {}, textContent: "" } },
      },
    },
  };
  return { ref, calls };
}

describe("page block actions (PAGE-037)", () => {
  it("resolves the hovered top-level block and its offset from the editor top", () => {
    const { ref } = documentEditor([
      { type: "paragraph", text: "first" },
      { type: "paragraph", text: "last" },
    ]);
    // Blocks occupy 0–12 and 12–24; a point at 15 lands in the second.
    expect(pageBlockAtPoint(ref, 15, 300)).toEqual({
      from: 12,
      to: 24,
      top: 12,
      json: { type: "paragraph", content: [{ type: "text", text: "last" }] },
    });
  });

  it("answers null when the editor cannot place the point", () => {
    const { ref } = documentEditor([{ type: "paragraph" }]);
    const view = ref.view;
    if (view === undefined) throw new Error("no view");
    view.posAtCoords = () => null;
    expect(pageBlockAtPoint(ref, 0, 0)).toBeNull();
    const { ref: bare } = editor();
    expect(pageBlockAtPoint(bare, 0, 0)).toBeNull();
  });

  it("moves a block up or down by rewriting it over its sibling", () => {
    const up = documentEditor([
      { type: "paragraph", text: "first" },
      { type: "paragraph", text: "last" },
    ]);
    movePageBlock(up.ref, { from: 12, to: 24, json: { type: "paragraph" } }, "up");
    expect(up.calls).toEqual([
      "focus",
      "insertContentAt([object Object], [object Object])",
      "run",
      "focus",
      "insertContentAt([object Object], [object Object])",
      "run",
    ]);

    const down = documentEditor([
      { type: "paragraph", text: "first" },
      { type: "paragraph", text: "last" },
    ]);
    movePageBlock(down.ref, { from: 0, to: 12, json: { type: "paragraph" } }, "down");
    expect(down.calls.filter((call) => call === "run")).toHaveLength(2);
  });

  it("ignores a move past the first or last block, or an unknown target", () => {
    const first = documentEditor([{ type: "paragraph" }, { type: "paragraph" }]);
    movePageBlock(first.ref, { from: 0, to: 12, json: { type: "paragraph" } }, "up");
    expect(first.calls).toEqual([]);

    const last = documentEditor([{ type: "paragraph" }, { type: "paragraph" }]);
    movePageBlock(last.ref, { from: 12, to: 24, json: { type: "paragraph" } }, "down");
    expect(last.calls).toEqual([]);

    const stray = documentEditor([{ type: "paragraph" }]);
    movePageBlock(stray.ref, { from: 99, to: 111, json: "elsewhere" }, "up");
    expect(stray.calls).toEqual([]);
  });

  it("duplicates and deletes an explicitly passed block instead of the caret's", () => {
    const duplicate = editor();
    duplicatePageBlock(duplicate.ref, { from: 3, to: 9, json: "block" });
    expect(duplicate.calls).toEqual(["focus", "insertContentAt(9, block)", "run"]);

    const remove = editor();
    deletePageBlock(remove.ref, { from: 3, to: 9, json: "block" });
    expect(remove.calls).toEqual(["focus", "deleteRange([object Object])", "run"]);
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List and quote toggle-off under the editor's own select-all
// (parity/specs/pages.md §5.3, PAGE-031). Ctrl/Mod+A with the editor
// focused selects the document's first and last positions, which sit
// outside the text blocks a formatting command restyles. Every case
// here drives a real editor over the page extension set and asserts the
// document the command leaves behind.

import { describe, expect, it } from "vitest";

import { loadPageExtensions } from "../../../shared/editor/pageExtensions.js";
import type { PageFormatCommand } from "../../../shared/editor/pageFormatting.js";
import { runPageCommand, type PageEditorRef } from "./pageEditorAdapter.js";

/** A live editor over the page extension set, cleaned up by the caller. */
async function liveEditor(html: string) {
  const { Editor } = await import("@tiptap/core");
  return new Editor({ extensions: await loadPageExtensions(), content: html });
}

/** The editor as the adapter's own structural surface. */
function adapter(editor: Awaited<ReturnType<typeof liveEditor>>): PageEditorRef {
  return editor as unknown as PageEditorRef;
}

/**
 * Run one command with either a whole-document selection (the editor's
 * select-all) or a native in-text drag from the first to the last
 * character, and answer the resulting body.
 */
async function toggle(html: string, command: PageFormatCommand, select: "all" | "native"): Promise<string> {
  const editor = await liveEditor(html);
  try {
    if (select === "all") {
      editor.commands.selectAll();
    } else {
      const { TextSelection } = await import("@tiptap/pm/state");
      const doc = editor.state.doc;
      editor.view.dispatch(editor.state.tr.setSelection(TextSelection.create(doc, 1, doc.content.size - 1)));
    }
    runPageCommand(adapter(editor), command);
    return editor.getHTML();
  } finally {
    editor.destroy();
  }
}

describe("whole-document selection and list toggles (PAGE-031)", () => {
  it.each(["orderedList", "bulletList", "taskList"] as const)(
    "wraps %s and lifts the whole document back to paragraphs",
    async (command) => {
      const wrapped = await toggle("<p>items</p>", command, "all");
      expect(wrapped).toContain("<li");
      expect(wrapped).not.toBe("<p>items</p>");
      expect(await toggle(wrapped, command, "all")).toBe("<p>items</p>");
    }
  );

  it("lifts a multi-item list to one paragraph per item", async () => {
    const wrapped = await toggle("<p>one</p><p>two</p>", "orderedList", "all");
    expect(wrapped).toBe("<ol><li><p>one</p></li><li><p>two</p></li></ol>");
    expect(await toggle(wrapped, "orderedList", "all")).toBe("<p>one</p><p>two</p>");
  });

  it("reaches the document a native in-text selection reaches", async () => {
    const html = "<ol><li><p>one</p></li><li><p>two</p></li></ol>";
    for (const command of ["orderedList", "bulletList", "taskList", "blockquote"] as const) {
      expect(await toggle(html, command, "all")).toBe(await toggle(html, command, "native"));
    }
  });

  it("lifts a whole-document quote back to paragraphs", async () => {
    expect(await toggle("<blockquote><p>q</p></blockquote>", "blockquote", "all")).toBe("<p>q</p>");
  });

  it("keeps converting a plain document into a list", async () => {
    expect(await toggle("<p>one</p><p>two</p>", "bulletList", "all")).toBe(
      "<ul><li><p>one</p></li><li><p>two</p></li></ul>"
    );
  });

  it("leaves a partial selection alone", async () => {
    const editor = await liveEditor("<p>keep</p><p>restyle</p>");
    try {
      const { TextSelection } = await import("@tiptap/pm/state");
      const from = editor.state.doc.child(0).nodeSize + 1;
      editor.view.dispatch(editor.state.tr.setSelection(TextSelection.create(editor.state.doc, from, from + 7)));
      runPageCommand(adapter(editor), "orderedList");
      expect(editor.getHTML()).toBe("<p>keep</p><ol><li><p>restyle</p></li></ol>");
    } finally {
      editor.destroy();
    }
  });

  it("does not expand a collapsed caret at the document start", async () => {
    const editor = await liveEditor("<p>first</p><p>second</p>");
    try {
      editor.commands.setTextSelection(1);
      runPageCommand(adapter(editor), "orderedList");
      expect(editor.getHTML()).toBe("<ol><li><p>first</p></li></ol><p>second</p>");
    } finally {
      editor.destroy();
    }
  });

  it("keeps a picture after the list out of the lifted paragraphs", async () => {
    // The picture is its own top-level block; repair stops at the last
    // text block so the lift never drags it into a list item.
    const html = '<ol><li><p>a</p></li></ol><image-component src="asset-1" status="uploaded"></image-component>';
    const lifted = await toggle(html, "orderedList", "all");
    expect(lifted).toContain("<p>a</p>");
    expect(lifted).toContain("<img");
    expect(lifted).not.toContain("<li>");
  });

  it("selects a document with no text block unchanged", async () => {
    // A picture-only body has no text endpoints to repair to; the command
    // runs on the selection the editor gave it.
    const html = '<image-component src="asset-1" status="uploaded"></image-component>';
    expect(await toggle(html, "orderedList", "all")).toBe(await toggle(html, "bulletList", "all"));
  });

  it("leaves a selection that would invert against the text endpoints alone", async () => {
    // A divider before the first text block, selected as a node: its end
    // sits before the first text block, so repairing would invert the
    // range. The command must then run on the untouched selection.
    const editor = await liveEditor("<hr><p>after</p>");
    try {
      const { NodeSelection } = await import("@tiptap/pm/state");
      editor.view.dispatch(editor.state.tr.setSelection(NodeSelection.create(editor.state.doc, 0)));
      const before = editor.getHTML();
      runPageCommand(adapter(editor), "orderedList");
      expect(editor.getHTML()).toBe(before);
    } finally {
      editor.destroy();
    }
  });
});

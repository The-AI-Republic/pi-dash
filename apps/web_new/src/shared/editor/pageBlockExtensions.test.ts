// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page block nodes (parity/specs/pages.md §6.1–§6.4, PAGE-037–044):
// callouts, inline emoji, atomic work-item embeds and entity references
// must parse their stored elements and serialize them back unchanged.

import type { AnyExtension } from "@tiptap/core";
import { afterEach, describe, expect, it, vi } from "vitest";

import { loadPageExtensions } from "./pageExtensions.js";
import { resetEntityPeek, subscribeEntityPeek } from "../shell/entityPeek.js";

afterEach(() => {
  resetEntityPeek();
});

async function editorFor(html: string) {
  const { Editor } = await import("@tiptap/core");
  return new Editor({ extensions: (await loadPageExtensions()) as unknown as AnyExtension[], content: html });
}

describe("page block nodes", () => {
  it("registers callout, emoji, embed, entity reference and mention", async () => {
    const names = (await loadPageExtensions()).map((extension) => extension.name);
    expect(names).toContain("pageCallout");
    expect(names).toContain("pageEmoji");
    expect(names).toContain("pageEmbed");
    expect(names).toContain("pageEntityReference");
    expect(names).toContain("mention");
  });

  it("round-trips a callout", async () => {
    const editor = await editorFor('<callout-component data-type="callout"><p>notice</p></callout-component>');
    try {
      const html = editor.getHTML();
      expect(html).toContain("<callout-component");
      expect(html).toContain("notice");
    } finally {
      editor.destroy();
    }
  });

  it("round-trips an inline body emoji", async () => {
    const editor = await editorFor('<p>hi <span data-type="emoji">🎉</span></p>');
    try {
      const html = editor.getHTML();
      expect(html).toContain('data-type="emoji"');
      expect(html).toContain("🎉");
    } finally {
      editor.destroy();
    }
  });

  it("keeps every embed identifier on an atomic block", async () => {
    const editor = await editorFor(
      '<p>before</p><issue-embed-component id="emb-1" entity_identifier="ISS-1" project_identifier="P" workspace_identifier="ws" entity_name="Embedded"></issue-embed-component><p>after</p>'
    );
    try {
      const html = editor.getHTML();
      expect(html).toContain("<issue-embed-component");
      for (const value of ["emb-1", "ISS-1", 'project_identifier="P"', 'workspace_identifier="ws"', "Embedded"]) {
        expect(html).toContain(value);
      }
      let found = false;
      editor.state.doc.descendants((node) => {
        if (node.type.name === "pageEmbed") found = true;
      });
      expect(found).toBe(true);
    } finally {
      editor.destroy();
    }
  });

  it("round-trips an entity reference with its display label", async () => {
    const editor = await editorFor(
      '<p>see <entity-reference-component entity_name="issue" entity_identifier="ISS-2" project_identifier="P" workspace_identifier="ws" data-label="Fix login"></entity-reference-component></p>'
    );
    try {
      const html = editor.getHTML();
      expect(html).toContain("<entity-reference-component");
      expect(html).toContain('data-label="Fix login"');
      expect(html).toContain('entity_identifier="ISS-2"');
    } finally {
      editor.destroy();
    }
  });

  it("opens the shell peek from a preview card click and Enter (PAGE-043)", async () => {
    const seen = vi.fn();
    const unsubscribe = subscribeEntityPeek(seen);
    const editor = await editorFor(
      '<p><entity-reference-component entity_name="issue" entity_identifier="ISS-2" project_identifier="P" workspace_identifier="ws" project_id="p-1" data-label="Fix login"></entity-reference-component></p>'
    );
    try {
      const card = editor.view.dom.querySelector('[role="link"]');
      if (card === null) throw new Error("no entity preview card");
      card.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      expect(seen).toHaveBeenCalledWith({
        entityName: "issue",
        entityIdentifier: "ISS-2",
        projectIdentifier: "P",
        workspaceIdentifier: "ws",
        projectId: "p-1",
        label: "Fix login",
      });
      card.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      expect(seen).toHaveBeenCalledTimes(2);
    } finally {
      editor.destroy();
      unsubscribe();
    }
  });

  it("renders a community embed card with an outbound upgrade link", async () => {
    const editor = await editorFor(
      '<issue-embed-component id="emb-1" entity_identifier="ISS-1"></issue-embed-component>'
    );
    try {
      const embed = editor.view.dom.querySelector("issue-embed-component");
      expect(embed).not.toBeNull();
      const link = embed?.querySelector("a");
      expect(link?.getAttribute("href")).toContain("pricing");
      expect(link?.getAttribute("target")).toBe("_blank");
      expect(embed?.textContent).toContain("Pro feature");
    } finally {
      editor.destroy();
    }
  });

  it("falls back to an empty string when an emoji or entity reference stores no label", async () => {
    const editor = await editorFor(
      '<p><span data-type="emoji"></span><entity-reference-component></entity-reference-component></p>'
    );
    try {
      let emoji: string | null = null;
      let reference: string | null = null;
      editor.state.doc.descendants((node) => {
        const text = node.type.spec.toText?.({ node, pos: 0, parent: node } as never) ?? node.textContent;
        if (node.type.name === "pageEmoji") emoji = text;
        if (node.type.name === "pageEntityReference") reference = text;
      });
      expect(emoji).toBe("");
      expect(reference).toBe("");
    } finally {
      editor.destroy();
    }
  });

  it("serializes a glyph that is not stored as a string as empty", async () => {
    const editor = await editorFor('<p><span data-type="emoji"></span></p>');
    try {
      const emoji = editor.state.doc.firstChild?.firstChild;
      if (emoji === null || emoji === undefined) throw new Error("no emoji node");
      editor.commands.insertContentAt({ from: 1, to: 1 }, { type: "pageEmoji", attrs: { glyph: 7 } });
      expect(editor.getHTML()).not.toContain(">7<");
    } finally {
      editor.destroy();
    }
  });

  it("renders nothing for an entity reference without a label", async () => {
    const editor = await editorFor("<p><entity-reference-component></entity-reference-component></p>");
    try {
      const reference = editor.view.dom.querySelector("entity-reference-component");
      expect(reference?.textContent).toBe("");
    } finally {
      editor.destroy();
    }
  });

  it("serializes a missing emoji glyph and a missing entity label as empty text", async () => {
    const editor = await editorFor("<p>start</p>");
    try {
      editor.commands.insertContentAt(1, { type: "pageEmoji", attrs: {} });
      editor.commands.insertContentAt(1, { type: "pageEntityReference", attrs: {} });
      expect(editor.getHTML()).toContain('<span data-type="emoji"></span>');
      expect(editor.getHTML()).toContain("<entity-reference-component></entity-reference-component>");
    } finally {
      editor.destroy();
    }
  });
});

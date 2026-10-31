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
  it("registers callout, emoji, embed, entity reference and mention under their stored names", async () => {
    const names = (await loadPageExtensions()).map((extension) => extension.name);
    expect(names).toContain("calloutComponent");
    expect(names).toContain("emoji");
    expect(names).toContain("issue-embed-component");
    expect(names).toContain("entity-reference-component");
    expect(names).toContain("mention");
  });

  it("round-trips a divider in the stored div form, never an hr (PAGE-037)", async () => {
    const editor = await editorFor('<p>above</p><div data-type="horizontalRule"><div></div></div><p>below</p>');
    try {
      const html = editor.getHTML();
      expect(html).toContain('<div data-type="horizontalRule"');
      expect(html).not.toContain("<hr");
      let found = false;
      editor.state.doc.descendants((node) => {
        if (node.type.name === "horizontalRule") found = true;
      });
      expect(found).toBe(true);
    } finally {
      editor.destroy();
    }
  });

  it("round-trips a callout in the allowlisted stored form", async () => {
    const editor = await editorFor('<div data-block-type="callout-component"><p>notice</p></div>');
    try {
      const html = editor.getHTML();
      expect(html).toContain('<div data-block-type="callout-component"');
      expect(html).toContain("notice");
      expect(html).not.toContain("<callout-component");
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
        if (node.type.name === "issue-embed-component") found = true;
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
      const link = await vi.waitFor(() => {
        const found = embed?.querySelector("a");
        expect(found).not.toBeNull();
        return found as HTMLAnchorElement;
      });
      expect(link.getAttribute("href")).toContain("pricing");
      expect(link.getAttribute("target")).toBe("_blank");
      expect(link.className).toContain("underline");
      expect(embed?.textContent).toContain("Pro feature");
      expect(embed?.textContent).toContain("Pro");
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
        if (node.type.name === "emoji") emoji = text;
        if (node.type.name === "entity-reference-component") reference = text;
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
      editor.commands.insertContentAt({ from: 1, to: 1 }, { type: "emoji", attrs: { glyph: 7 } });
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
      editor.commands.insertContentAt(1, { type: "emoji", attrs: {} });
      editor.commands.insertContentAt(1, { type: "entity-reference-component", attrs: {} });
      expect(editor.getHTML()).toContain('<span data-type="emoji"></span>');
      expect(editor.getHTML()).toContain("<entity-reference-component></entity-reference-component>");
    } finally {
      editor.destroy();
    }
  });
});

describe("page body picture", () => {
  it("keeps an inserted picture in the stored element with its asset id (PAGE-039)", async () => {
    const editor = await editorFor("<p>before</p>");
    try {
      editor.commands.insertContent({
        type: "imageComponent",
        attrs: { src: "/api/assets/v2/a/", "data-stored-src": "asset-a", status: "uploaded", alt: "shot" },
      });
      const html = editor.getHTML();
      expect(html).toContain("<image-component");
      expect(html).toContain('src="asset-a"');
      expect(html).toContain('status="uploaded"');
      expect(html).toContain('alt="shot"');
      const names: string[] = [];
      editor.state.doc.descendants((node) => {
        names.push(node.type.name);
      });
      expect(names).toContain("imageComponent");
    } finally {
      editor.destroy();
    }
  });

  it("upgrades a legacy bare picture whose source is the asset id", async () => {
    const editor = await editorFor('<p>before</p><img src="asset-legacy" alt="old">');
    try {
      const html = editor.getHTML();
      expect(html).toContain("<image-component");
      expect(html).toContain('src="asset-legacy"');
      expect(html).not.toContain("<img");
    } finally {
      editor.destroy();
    }
  });

  it("leaves a picture addressed by a URL as a plain img", async () => {
    for (const source of ["https://example.test/a.png", "/api/assets/v2/a/", "//cdn.example.test/a.png"]) {
      const editor = await editorFor(`<p>before</p><img src="${source}">`);
      try {
        const html = editor.getHTML();
        expect(html).toContain(`<img src="${source}"`);
        expect(html).not.toContain("<image-component");
      } finally {
        editor.destroy();
      }
    }
  });

  it("paints the served source inside the stored element's node view", async () => {
    const editor = await editorFor(
      '<p>before</p><image-component src="asset-a" data-stored-src="asset-a" alt="shot" status="uploaded"></image-component>'
    );
    try {
      const element = editor.view.dom.querySelector("image-component");
      expect(element).not.toBeNull();
      expect(element?.getAttribute("contenteditable")).toBe("false");
      expect(element?.getAttribute("data-stored-src")).toBe("asset-a");
      const image = await vi.waitFor(() => {
        const found = element?.querySelector("img");
        expect(found).not.toBeNull();
        return found as HTMLImageElement;
      });
      expect(image.getAttribute("src")).toBe("asset-a");
      expect(image.getAttribute("alt")).toBe("shot");
    } finally {
      editor.destroy();
    }
  });
});

describe("picture identity", () => {
  it("keeps a stored picture's own id through a round trip", async () => {
    const editor = await editorFor(
      '<image-component id="pic-1" src="asset-a" width="35%" alignment="left" status="uploaded"></image-component>'
    );
    try {
      const html = editor.getHTML();
      expect(html).toContain('id="pic-1"');
      expect(html).toContain('src="asset-a"');
      let stored: string | null = null;
      editor.state.doc.descendants((node) => {
        if (node.type.name === "imageComponent") stored = node.attrs["id"] as string | null;
      });
      expect(stored).toBe("pic-1");
    } finally {
      editor.destroy();
    }
  });

  it("labels the painted picture image-<id> when it carries no alt text", async () => {
    const editor = await editorFor('<image-component id="pic-2" src="asset-b"></image-component>');
    try {
      const image = await vi.waitFor(() => {
        const found = editor.view.dom.querySelector("image-component img");
        expect(found).not.toBeNull();
        return found as HTMLImageElement;
      });
      expect(image.getAttribute("alt")).toBe("image-pic-2");
    } finally {
      editor.destroy();
    }
  });

  it("leaves the painted picture unlabelled when it has neither alt nor id", async () => {
    const editor = await editorFor('<image-component src="asset-c"></image-component>');
    try {
      const image = await vi.waitFor(() => {
        const found = editor.view.dom.querySelector("image-component img");
        expect(found).not.toBeNull();
        return found as HTMLImageElement;
      });
      expect(image.getAttribute("alt")).toBeNull();
    } finally {
      editor.destroy();
    }
  });
});

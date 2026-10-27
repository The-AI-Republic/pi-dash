// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Live-sync wire compatibility (parity/specs/pages.md §5.3, PAGE-034/036).
// A page body persists through live Yjs sync, and the collaboration
// server turns the synced state back into HTML with its own schema. Only
// the type and attribute names that schema knows survive the trip: an
// unknown mark is dropped with its span, and the plain image node comes
// back as a literal img instead of the stored image-component. The page
// editor must therefore speak the server's names for the colour mark and
// the stored-image node, and this suite pins that contract. The names
// below are the server's (verified against its schema); the simulation
// only drops what the server cannot know and renders what it keeps.

import type { AnyExtension } from "@tiptap/core";
import { describe, expect, it } from "vitest";

import { loadPageExtensions } from "./pageExtensions.js";
import { loadPageColorMark } from "./pageTableExtensions.js";

/** A structured node or mark in the editor's JSON form. */
interface WireNode {
  type?: unknown;
  text?: unknown;
  content?: WireNode[];
  marks?: WireNode[];
  attrs?: Record<string, unknown>;
}

/**
 * The live server's names for the types this suite covers, with the
 * attributes it keeps on each. Anything else on the wire is dropped in
 * the server's serialization, so the simulation below drops it too.
 */
const SERVER_NODES: Record<string, readonly string[]> = {
  doc: [],
  paragraph: [],
  text: [],
  image: ["src", "alt", "title"],
  imageComponent: ["src", "alt", "title", "id", "width", "height", "aspectRatio", "alignment", "status"],
};

const SERVER_MARKS: Record<string, readonly string[]> = {
  customColor: ["color", "backgroundColor"],
};

/** Drop the nodes, marks and attributes the server schema cannot know. */
function serverView(node: WireNode): WireNode | null {
  if (typeof node.type !== "string") return null;
  if (node.type === "text") {
    const kept = (node.marks ?? []).filter(
      (mark): mark is WireNode => typeof mark.type === "string" && SERVER_MARKS[mark.type] !== undefined
    );
    return {
      type: "text",
      text: node.text,
      marks: kept.map((mark) => ({
        type: mark.type,
        attrs: keepAttrs(mark.attrs, SERVER_MARKS[mark.type as string] ?? []),
      })),
    };
  }
  const keptAttrs = SERVER_NODES[node.type];
  if (keptAttrs === undefined) return null;
  const content = (node.content ?? []).map(serverView).filter((child): child is WireNode => child !== null);
  return { type: node.type, attrs: keepAttrs(node.attrs, keptAttrs), content };
}

function keepAttrs(attrs: Record<string, unknown> | undefined, kept: readonly string[]): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const name of kept) {
    const value = attrs?.[name];
    if (typeof value === "string" && value !== "") out[name] = value;
  }
  return out;
}

function escapeHtml(value: string): string {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/** Render the server's view the way its schema serializes each type. */
function serverHtml(node: WireNode): string {
  if (node.type === "text") {
    let html = escapeHtml(typeof node.text === "string" ? node.text : "");
    for (const mark of node.marks ?? []) {
      if (mark.type !== "customColor") continue;
      const attrs = mark.attrs ?? {};
      const parts: string[] = [];
      if (typeof attrs["color"] === "string") parts.push(`data-text-color="${escapeHtml(attrs["color"])}"`);
      if (typeof attrs["backgroundColor"] === "string")
        parts.push(`data-background-color="${escapeHtml(attrs["backgroundColor"])}"`);
      html = `<span${parts.length === 0 ? "" : ` ${parts.join(" ")}`}>${html}</span>`;
    }
    return html;
  }
  if (node.type === "image") {
    const src = typeof node.attrs?.["src"] === "string" ? escapeHtml(node.attrs["src"]) : "";
    return `<img src="${src}">`;
  }
  if (node.type === "imageComponent") {
    const attrs = node.attrs ?? {};
    const parts = Object.entries(attrs).map(([name, value]) => `${name}="${escapeHtml(String(value))}"`);
    return `<image-component${parts.length === 0 ? "" : ` ${parts.join(" ")}`}></image-component>`;
  }
  return (node.content ?? []).map(serverHtml).join("");
}

async function clientJson(html: string): Promise<WireNode> {
  const { Editor } = await import("@tiptap/core");
  const editor = new Editor({ extensions: (await loadPageExtensions()) as unknown as AnyExtension[], content: html });
  try {
    return editor.getJSON() as unknown as WireNode;
  } finally {
    editor.destroy();
  }
}

describe("live-sync wire names", () => {
  it("names the colour mark and the stored-image node for the server schema", async () => {
    const mark = await loadPageColorMark();
    expect(mark.name).toBe("customColor");
    // The page set carries the stored-image node; it is not attached
    // separately any more, so the assertion reads the set it ships in.
    const page = (await loadPageExtensions()) as unknown as Array<{ name: string }>;
    expect(page.map((extension) => extension.name)).toContain("imageComponent");
  });

  it("keeps letter and wash colours through the server's view (PAGE-036)", async () => {
    const json = await clientJson('<p><span data-text-color="pink" data-background-color="green">colors</span></p>');
    const paragraph = json.content?.[0];
    const mark = paragraph?.content?.[0]?.marks?.[0];
    expect(mark?.type).toBe("customColor");
    expect(mark?.attrs?.["color"]).toBe("pink");
    expect(mark?.attrs?.["backgroundColor"]).toBe("green");
    const html = serverHtml(serverView(json) ?? { type: "doc", content: [] });
    expect(html).toContain('data-text-color="pink"');
    expect(html).toContain('data-background-color="green"');
    expect(html).toContain("colors");
  });

  it("keeps a stored image as image-component through the server's view (PAGE-034)", async () => {
    const json = await clientJson(
      '<p>pic</p><image-component src="asset-1" status="uploaded" data-stored-src="asset-1"></image-component>'
    );
    const image = json.content?.find((child) => child.type === "imageComponent");
    expect(image?.type).toBe("imageComponent");
    expect(image?.attrs?.["status"]).toBe("uploaded");
    const html = serverHtml(serverView(json) ?? { type: "doc", content: [] });
    expect(html).toContain("<image-component");
    expect(html).toContain('status="uploaded"');
  });
});

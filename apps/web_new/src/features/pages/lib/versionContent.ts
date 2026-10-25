// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Snapshot body selection (parity/specs/pages.md §6.9, PAGE-052). A stored
// snapshot carries both the editor's structured document and its markup.
// The structured form is the fuller one — it keeps the marks and block
// semantics the editor wrote — so it wins whenever the whole document is
// made of nodes this renderer knows. Anything else falls back to the
// markup the server stored beside it, so an unfamiliar node degrades to
// the server's own rendering instead of rendering blank.

/** Nodes the snapshot renderer can paint; anything else uses the fallback. */
const RENDERABLE_TYPES = new Set([
  "doc",
  "paragraph",
  "heading",
  "text",
  "hardBreak",
  "bulletList",
  "orderedList",
  "listItem",
  "blockquote",
  "codeBlock",
  "horizontalRule",
]);

/** Inline marks it can paint; an unknown mark is dropped, not its text. */
const RENDERABLE_MARKS = new Set(["bold", "italic", "strike", "code", "link"]);

interface StructuredNode {
  type?: unknown;
  text?: unknown;
  content?: unknown;
  marks?: unknown;
  attrs?: unknown;
}

interface StructuredMark {
  type?: unknown;
  attrs?: unknown;
}

function nodeList(value: unknown): StructuredNode[] {
  return Array.isArray(value) ? (value as StructuredNode[]) : [];
}

function attrsOf(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
}

function escapeText(value: string): string {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** True when every node in the tree is one the renderer understands. */
function allRenderable(nodes: readonly StructuredNode[]): boolean {
  return nodes.every(
    (node) => typeof node.type === "string" && RENDERABLE_TYPES.has(node.type) && allRenderable(nodeList(node.content))
  );
}

function wrapMark(mark: StructuredMark, inner: string): string {
  if (mark.type === "bold") return `<strong>${inner}</strong>`;
  if (mark.type === "italic") return `<em>${inner}</em>`;
  if (mark.type === "strike") return `<s>${inner}</s>`;
  if (mark.type === "code") return `<code>${inner}</code>`;
  if (mark.type === "link") {
    const href = attrsOf(mark.attrs)["href"];
    return typeof href === "string" && href !== "" ? `<a href="${escapeText(href)}">${inner}</a>` : inner;
  }
  return inner;
}

/** Phrasing content: text with its marks, and hard breaks. */
function renderInline(nodes: readonly StructuredNode[]): string {
  return nodes
    .map((node) => {
      if (node.type === "hardBreak") return "<br>";
      if (node.type !== "text" || typeof node.text !== "string") return "";
      const marks = nodeList(node.marks as unknown) as StructuredMark[];
      return marks
        .filter((mark) => RENDERABLE_MARKS.has(mark.type as string))
        .reduceRight<string>((inner, mark) => wrapMark(mark, inner), escapeText(node.text));
    })
    .join("");
}

/** Block content: one markup fragment per block, in document order. */
function renderBlocks(nodes: readonly StructuredNode[]): string {
  return nodes.map(renderBlock).join("");
}

function renderListItem(item: StructuredNode): string {
  const children = nodeList(item.content);
  const blocks = children.filter((child) => child.type !== "bulletList" && child.type !== "orderedList");
  const nested = children.filter((child) => child.type === "bulletList" || child.type === "orderedList");
  return `<li>${blocks.map(renderBlock).join("")}${nested.map(renderBlock).join("")}</li>`;
}

function plainText(node: StructuredNode): string {
  if (typeof node.text === "string") return node.text;
  return nodeList(node.content).map(plainText).join("");
}

function renderBlock(node: StructuredNode): string {
  const attrs = attrsOf(node.attrs);
  switch (node.type) {
    case "doc":
      return renderBlocks(nodeList(node.content));
    case "paragraph":
      return `<p>${renderInline(nodeList(node.content))}</p>`;
    case "heading": {
      const raw = attrs["level"];
      const level = typeof raw === "number" ? Math.min(6, Math.max(1, raw)) : 1;
      return `<h${level}>${renderInline(nodeList(node.content))}</h${level}>`;
    }
    case "bulletList":
      return `<ul>${nodeList(node.content).map(renderListItem).join("")}</ul>`;
    case "orderedList": {
      const start = attrs["start"];
      const opening = typeof start === "number" && start !== 1 ? `<ol start="${start}">` : "<ol>";
      return `${opening}${nodeList(node.content).map(renderListItem).join("")}</ol>`;
    }
    case "blockquote":
      return `<blockquote>${renderBlocks(nodeList(node.content))}</blockquote>`;
    case "codeBlock":
      return `<pre><code>${escapeText(plainText(node))}</code></pre>`;
    case "horizontalRule":
      return "<hr>";
    default:
      // Only reachable from a hand-built tree; the gate above keeps real
      // documents out of here.
      return "";
  }
}

/**
 * The markup the overlay shows for one snapshot: the rendered structured
 * document when every node is known, else the server's stored markup.
 */
export function snapshotBodyHtml(snapshot: {
  description_json: Record<string, unknown>;
  description_html: string;
}): string {
  const document = snapshot.description_json as StructuredNode;
  if (document.type !== "doc" || !allRenderable([document])) return snapshot.description_html;
  const rendered = renderBlock(document);
  return rendered.trim() === "" ? snapshot.description_html : rendered;
}

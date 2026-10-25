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

/** Inline marks it can paint; an unknown mark sends the document to the fallback. */
type RenderableMark = { type: "bold" | "italic" | "underline" | "strike" | "code" | "link"; attrs?: unknown };

const RENDERABLE_MARKS: ReadonlySet<string> = new Set<RenderableMark["type"]>([
  "bold",
  "italic",
  "underline",
  "strike",
  "code",
  "link",
]);

/** The alignments the page editor writes on paragraphs and headings. */
const ALIGNMENTS: ReadonlySet<string> = new Set(["left", "center", "right"]);

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

function marksOf(value: unknown): StructuredMark[] {
  return Array.isArray(value) ? (value as StructuredMark[]) : [];
}

/** A mark is paintable when both its kind and the address it carries are. */
function markRenderable(mark: StructuredMark): boolean {
  if (typeof mark.type !== "string" || !RENDERABLE_MARKS.has(mark.type)) return false;
  const href = attrsOf(mark.attrs)["href"];
  return href === undefined || typeof href === "string";
}

/**
 * Alignment is paintable only in the three values the editor writes;
 * any other value is a formatting decision this renderer would drop.
 */
function alignmentRenderable(value: unknown): boolean {
  return value === undefined || (typeof value === "string" && ALIGNMENTS.has(value));
}

/** How each attribute the renderer paints is read, by owning node kind. */
const NODE_ATTRIBUTES: Readonly<Record<string, Readonly<Record<string, (value: unknown) => boolean>>>> = {
  paragraph: { textAlign: alignmentRenderable },
  heading: { textAlign: alignmentRenderable, level: (value) => typeof value === "number" },
  orderedList: { start: (value) => typeof value === "number" },
};

/**
 * A node keeps only the attributes the editor stores on it — alignment
 * on text blocks, a heading's level, a list's start — and only in the
 * shape the renderer reads. Anything else would be lost by painting the
 * structured form, so it sends the document to the fallback. The editor
 * writes every default attribute as null; those carry nothing.
 */
function attrsRenderable(type: string, attrs: unknown): boolean {
  const known = NODE_ATTRIBUTES[type] ?? {};
  return Object.entries(attrsOf(attrs)).every(
    ([name, value]) =>
      value === null || value === undefined || (Object.hasOwn(known, name) && known[name]?.(value) === true)
  );
}

/**
 * True when the renderer can paint the node whole: a known kind, no
 * attribute or mark it would have to drop, and the same for everything
 * inside it.
 */
function nodeRenderable(node: StructuredNode): boolean {
  if (typeof node.type !== "string" || !RENDERABLE_TYPES.has(node.type)) return false;
  if (node.type === "text") {
    // A text node with no text paints nothing, and nothing is what it
    // holds, so it costs no fidelity to skip it (the inline renderer
    // does exactly that).
    if (typeof node.text !== "string") return true;
    return node.marks === undefined || (Array.isArray(node.marks) && marksOf(node.marks).every(markRenderable));
  }
  if (node.marks !== undefined || !attrsRenderable(node.type, node.attrs)) return false;
  return nodeList(node.content).every(nodeRenderable);
}

function wrapMark(mark: RenderableMark, inner: string): string {
  switch (mark.type) {
    case "bold":
      return `<strong>${inner}</strong>`;
    case "italic":
      return `<em>${inner}</em>`;
    case "underline":
      return `<u>${inner}</u>`;
    case "strike":
      return `<s>${inner}</s>`;
    case "code":
      return `<code>${inner}</code>`;
    case "link": {
      const href = attrsOf(mark.attrs)["href"];
      return typeof href === "string" && href !== "" ? `<a href="${escapeText(href)}">${inner}</a>` : inner;
    }
  }
}

/**
 * Phrasing content: text with its marks, and hard breaks. A stored node
 * that is neither (a block where text belongs, or a text node with no
 * text) paints nothing rather than guessing.
 */
function renderInline(nodes: readonly StructuredNode[]): string {
  return nodes
    .map((node) => {
      if (node.type === "hardBreak") return "<br>";
      if (node.type !== "text" || typeof node.text !== "string") return "";
      return marksOf(node.marks).reduceRight<string>(
        (inner, mark) => wrapMark({ type: mark.type as RenderableMark["type"], attrs: mark.attrs }, inner),
        escapeText(node.text)
      );
    })
    .join("");
}

/** The alignment style a text block carries, or nothing. */
function alignAttribute(attrs: Record<string, unknown>): string {
  const alignment = attrs["textAlign"];
  return typeof alignment === "string" ? ` style="text-align: ${alignment}"` : "";
}

/** Block content: one markup fragment per block, in document order. */
function renderBlocks(nodes: readonly StructuredNode[]): string {
  return nodes.map((node) => renderBlock(node as RenderableBlock)).join("");
}

function renderListItem(item: StructuredNode): string {
  const children = nodeList(item.content);
  const blocks = children.filter((child) => child.type !== "bulletList" && child.type !== "orderedList");
  const nested = children.filter((child) => child.type === "bulletList" || child.type === "orderedList");
  return `<li>${blocks.map((block) => renderBlock(block as RenderableBlock)).join("")}${nested
    .map((block) => renderBlock(block as RenderableBlock))
    .join("")}</li>`;
}

function plainText(node: StructuredNode): string {
  if (typeof node.text === "string") return node.text;
  return nodeList(node.content).map(plainText).join("");
}

/** A block the gate has already accepted; the switch below is total. */
type RenderableBlock = {
  type: "doc" | "paragraph" | "heading" | "bulletList" | "orderedList" | "blockquote" | "codeBlock" | "horizontalRule";
  attrs?: unknown;
  content?: unknown;
};

function renderBlock(node: RenderableBlock): string {
  const attrs = attrsOf(node.attrs);
  switch (node.type) {
    case "doc":
      return renderBlocks(nodeList(node.content));
    case "paragraph":
      return `<p${alignAttribute(attrs)}>${renderInline(nodeList(node.content))}</p>`;
    case "heading": {
      const raw = attrs["level"];
      const level = typeof raw === "number" ? Math.min(6, Math.max(1, raw)) : 1;
      return `<h${level}${alignAttribute(attrs)}>${renderInline(nodeList(node.content))}</h${level}>`;
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
  if (document.type !== "doc" || !nodeRenderable(document)) return snapshot.description_html;
  const rendered = renderBlock(document as RenderableBlock);
  return rendered.trim() === "" ? snapshot.description_html : rendered;
}

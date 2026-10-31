// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// A small markdown parser for chat transcripts (UI kit and editor page).
// Assistant replies arrive as markdown and must read as formatted text,
// but the new tree carries no markdown package and a runtime dependency
// needs allowlist review, so this covers the subset an agent reply
// actually uses: paragraphs, ATX and setext headings, fenced and indented
// code, block quotes, ordered and unordered lists with nesting, rules,
// and the inline spans emphasis, strong, inline code, strikethrough and
// links. Raw HTML is never interpreted — it stays text — and link targets
// are restricted to browsing schemes, so the result is safe to render as
// React elements with no sanitizer pass.

/** A block of rendered-markdown content. */
export type MarkdownBlock =
  | { type: "paragraph"; spans: MarkdownSpan[] }
  | { type: "heading"; level: 1 | 2 | 3 | 4 | 5 | 6; spans: MarkdownSpan[] }
  | { type: "code"; language: string; text: string }
  | { type: "quote"; blocks: MarkdownBlock[] }
  | { type: "list"; ordered: boolean; start: number; items: MarkdownListItem[] }
  | { type: "rule" };

/** One list entry; nested blocks hang off their own paragraph. */
export interface MarkdownListItem {
  blocks: MarkdownBlock[];
}

/** A run of text, optionally carrying one inline style. */
export type MarkdownSpan =
  | { type: "text"; text: string }
  | { type: "code"; text: string }
  | { type: "strong"; spans: MarkdownSpan[] }
  | { type: "emphasis"; spans: MarkdownSpan[] }
  | { type: "strike"; spans: MarkdownSpan[] }
  | { type: "link"; href: string; spans: MarkdownSpan[] }
  | { type: "break" };

/** Schemes a chat reply may link to; anything else stays plain text. */
const ALLOWED_SCHEMES = new Set(["http:", "https:", "mailto:"]);

/**
 * Accept a link target only when it is a relative address, a fragment or
 * one of the browsing schemes above. Returns null for anything else
 * (javascript:, data:, file:), which the caller renders as text.
 *
 * A browser strips control characters and whitespace before resolving a
 * target, so `java&#9;script:` would become `javascript:` in the href
 * attribute. Targets carrying any of those stay text. A same-host
 * address only ever starts with a single slash; `//host` and `\\host`
 * are scheme-relative and leave the site, so they stay text too.
 */
export function safeHref(raw: string): string | null {
  const target = raw.trim();
  if (target === "") return null;
  if (hasDroppedCharacter(target)) return null;
  if (target.startsWith("//") || target.startsWith("\\\\")) return null;
  if (target.startsWith("#") || target.startsWith("/")) return target;
  const scheme = /^([a-z][a-z0-9+.-]*):/i.exec(target);
  if (!scheme) return target;
  return ALLOWED_SCHEMES.has(`${scheme[1]!.toLowerCase()}:`) ? target : null;
}

/**
 * True when a browser would silently drop a character before resolving the
 * target: ASCII controls and spaces, DEL and the C1 block, and the Unicode
 * spaces it treats as whitespace. `java&#9;script:` would resolve to
 * `javascript:` once that tab is gone, so such a target is never a link.
 */
function hasDroppedCharacter(target: string): boolean {
  for (const char of target) {
    // Every code point a for-of yields has a code point; the assertion
    // only satisfies the type.
    const code = char.codePointAt(0)!;
    const dropped =
      code <= 0x20 ||
      (code >= 0x7f && code <= 0xa0) ||
      code === 0x1680 ||
      (code >= 0x2000 && code <= 0x200a) ||
      code === 0x2028 ||
      code === 0x2029 ||
      code === 0x202f ||
      code === 0x205f ||
      code === 0x3000;
    if (dropped) return true;
  }
  return false;
}

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([^`\s]*)\s*$/;
const ATX = /^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^ {0,3}([-*_])(?:\s*\1){2,}\s*$/;
const QUOTE = /^ {0,3}>\s?(.*)$/;
const LIST_ITEM = /^( *)([-*+]|\d{1,9}[.)])(?:\s+(.*))?$/;
const SETEXT = /^ {0,3}(=+|-+)\s*$/;

function isBlank(line: string): boolean {
  return line.trim() === "";
}

/** Parse a markdown document into blocks. */
export function parseMarkdown(source: string): MarkdownBlock[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  return parseBlocks(lines);
}

function parseBlocks(lines: readonly string[]): MarkdownBlock[] {
  const blocks: MarkdownBlock[] = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index]!;
    if (isBlank(line)) {
      index += 1;
      continue;
    }

    const fence = FENCE.exec(line);
    if (fence) {
      const marker = fence[1]!;
      const language = fence[2]!;
      const body: string[] = [];
      index += 1;
      while (index < lines.length) {
        const closing = new RegExp(`^ {0,3}${marker[0] === "`" ? "`" : "~"}{${marker.length},}\\s*$`);
        if (closing.test(lines[index]!)) {
          index += 1;
          break;
        }
        body.push(lines[index]!);
        index += 1;
      }
      blocks.push({ type: "code", language, text: body.join("\n") });
      continue;
    }

    const heading = ATX.exec(line);
    if (heading) {
      blocks.push({
        type: "heading",
        level: heading[1]!.length as 1 | 2 | 3 | 4 | 5 | 6,
        spans: parseInline(heading[2]!),
      });
      index += 1;
      continue;
    }

    if (RULE.test(line)) {
      blocks.push({ type: "rule" });
      index += 1;
      continue;
    }

    const quote = QUOTE.exec(line);
    if (quote) {
      const body: string[] = [];
      while (index < lines.length) {
        const current = QUOTE.exec(lines[index]!);
        if (!current) {
          if (isBlank(lines[index]!)) {
            body.push("");
            index += 1;
            continue;
          }
          break;
        }
        body.push(current[1]!);
        index += 1;
      }
      blocks.push({ type: "quote", blocks: parseBlocks(body) });
      continue;
    }

    if (LIST_ITEM.test(line)) {
      const parsed = parseList(lines, index);
      blocks.push(parsed.block);
      index = parsed.next;
      continue;
    }

    const indented = /^( {4,}|\t)/.test(line);
    if (indented) {
      const body: string[] = [];
      while (index < lines.length && (/^( {4,}|\t)/.test(lines[index]!) || isBlank(lines[index]!))) {
        body.push(lines[index]!.replace(/^( {4}|\t)/, ""));
        index += 1;
      }
      blocks.push({ type: "code", language: "", text: trimTrailingBlanks(body).join("\n") });
      continue;
    }

    const paragraph: string[] = [];
    while (index < lines.length) {
      const current = lines[index]!;
      if (isBlank(current)) break;
      if (paragraph.length > 0) {
        if (SETEXT.test(current)) {
          const level = current.trim().startsWith("=") ? 1 : 2;
          blocks.push({ type: "heading", level, spans: parseInline(paragraph.join(" ")) });
          index += 1;
          paragraph.length = 0;
          break;
        }
        if (
          FENCE.test(current) ||
          ATX.test(current) ||
          RULE.test(current) ||
          QUOTE.test(current) ||
          LIST_ITEM.test(current)
        ) {
          break;
        }
      }
      paragraph.push(current.trim());
      index += 1;
    }
    if (paragraph.length > 0) blocks.push({ type: "paragraph", spans: parseInline(joinLines(paragraph)) });
  }
  return blocks;
}

/** Parse one list run, including its nested blocks and continuation lines. */
function parseList(lines: readonly string[], startIndex: number): { block: MarkdownBlock; next: number } {
  const first = LIST_ITEM.exec(lines[startIndex]!)!;
  const baseIndent = first[1]!.length;
  const ordered = /\d/.test(first[2]!);
  const start = ordered ? Number.parseInt(first[2]!, 10) : 1;
  const items: MarkdownListItem[] = [];
  let index = startIndex;
  let current: string[] = [];
  let itemIndent = baseIndent;

  const flush = (): void => {
    if (current.length > 0) items.push({ blocks: parseListItem(current) });
    current = [];
  };

  while (index < lines.length) {
    const line = lines[index]!;
    if (isBlank(line)) {
      current.push("");
      index += 1;
      // A blank line only continues the list if the next line still
      // belongs to it; otherwise the list ends.
      const next = lines[index];
      if (next === undefined) break;
      const nextItem = LIST_ITEM.exec(next);
      if (nextItem && nextItem[1]!.length <= baseIndent && nextItem[1]!.length >= Math.max(0, baseIndent - 1)) {
        continue;
      }
      if (/^ */.exec(next)![0].length > itemIndent) {
        continue;
      }
      break;
    }
    const item = LIST_ITEM.exec(line);
    if (item) {
      const indent = item[1]!.length;
      if (indent <= baseIndent + 1) {
        if (indent < baseIndent) break;
        flush();
        itemIndent = indent + item[2]!.length + 1;
        current.push(item[3] ?? "");
        index += 1;
        continue;
      }
    }
    const indent = /^ */.exec(line)![0].length;
    if (indent >= itemIndent) {
      current.push(line.slice(itemIndent));
      index += 1;
      continue;
    }
    break;
  }
  flush();
  return { block: { type: "list", ordered, start, items }, next: index };
}

/** A list entry's first line is a paragraph; later blocks parse normally. */
function parseListItem(lines: readonly string[]): MarkdownBlock[] {
  const body = trimTrailingBlanks([...lines]);
  if (body.length === 0) return [];
  return parseBlocks(body);
}

/** Wrapped lines read as one line; a break span comes from a literal newline. */
function joinLines(lines: readonly string[]): string {
  return lines.join(" ");
}

function trimTrailingBlanks(lines: string[]): string[] {
  let end = lines.length;
  while (end > 0 && isBlank(lines[end - 1]!)) end -= 1;
  return lines.slice(0, end);
}

/** Parse one line of inline spans, honouring code first so its text is literal. */
export function parseInline(source: string): MarkdownSpan[] {
  const spans: MarkdownSpan[] = [];
  let buffer = "";
  let index = 0;

  const pushText = (): void => {
    if (buffer === "") return;
    spans.push({ type: "text", text: buffer });
    buffer = "";
  };

  while (index < source.length) {
    const char = source[index]!;

    if (char === "\n") {
      pushText();
      spans.push({ type: "break" });
      index += 1;
      continue;
    }

    if (char === "\\" && index + 1 < source.length) {
      buffer += source[index + 1]!;
      index += 2;
      continue;
    }

    if (char === "`") {
      const run = /^`+/.exec(source.slice(index))![0];
      const closing = source.indexOf(run, index + run.length);
      if (closing !== -1) {
        pushText();
        const raw = source.slice(index + run.length, closing);
        spans.push({ type: "code", text: raw.replace(/^ (.*) $/, "$1") });
        index = closing + run.length;
        continue;
      }
    }

    if (char === "!" && source[index + 1] === "[") {
      // Images are not rendered in a transcript; their alt text reads.
      const close = source.indexOf("]", index + 2);
      const target = close === -1 ? -1 : source.indexOf(")", close + 2);
      if (close !== -1 && source[close + 1] === "(" && target !== -1) {
        buffer += source.slice(index + 2, close);
        index = target + 1;
        continue;
      }
    }

    if (char === "[") {
      const parsed = parseLink(source, index);
      if (parsed) {
        pushText();
        spans.push(parsed.span);
        index = parsed.next;
        continue;
      }
    }

    if (source.startsWith("**", index) || source.startsWith("__", index)) {
      const marker = source.slice(index, index + 2);
      const close = findCloser(source, index + 2, marker);
      if (close !== -1) {
        const inner = source.slice(index + 2, close);
        pushText();
        spans.push({ type: "strong", spans: parseInline(inner) });
        index = close + 2;
        continue;
      }
    }

    if (source.startsWith("~~", index)) {
      const close = source.indexOf("~~", index + 2);
      if (close !== -1) {
        pushText();
        spans.push({ type: "strike", spans: parseInline(source.slice(index + 2, close)) });
        index = close + 2;
        continue;
      }
    }

    if (char === "*" || char === "_") {
      const close = findCloser(source, index + 1, char);
      if (close !== -1 && close > index + 1) {
        const inner = source.slice(index + 1, close);
        if (inner.trim() !== "" && !/^\s/.test(inner) && !/\s$/.test(inner)) {
          pushText();
          spans.push({ type: "emphasis", spans: parseInline(inner) });
          index = close + 1;
          continue;
        }
      }
    }

    buffer += char;
    index += 1;
  }
  pushText();
  return spans;
}

/** Two trailing spaces or a backslash end a line inside a paragraph. */
function parseLink(source: string, index: number): { span: MarkdownSpan; next: number } | null {
  let depth = 0;
  let labelEnd = -1;
  for (let cursor = index; cursor < source.length; cursor += 1) {
    const char = source[cursor]!;
    if (char === "\\") {
      cursor += 1;
      continue;
    }
    if (char === "[") depth += 1;
    else if (char === "]") {
      depth -= 1;
      if (depth === 0) {
        labelEnd = cursor;
        break;
      }
    }
  }
  if (labelEnd === -1 || source[labelEnd + 1] !== "(") return null;
  let parens = 0;
  let targetEnd = -1;
  for (let cursor = labelEnd + 1; cursor < source.length; cursor += 1) {
    const char = source[cursor]!;
    if (char === "(") parens += 1;
    else if (char === ")") {
      parens -= 1;
      if (parens === 0) {
        targetEnd = cursor;
        break;
      }
    }
  }
  if (targetEnd === -1) return null;
  const target = source.slice(labelEnd + 2, targetEnd).trim();
  const href = safeHref(target.split(/\s+/)[0]!);
  const label = source.slice(index + 1, labelEnd);
  const next = targetEnd + 1;
  if (href === null) return { span: { type: "text", text: `[${label}]` }, next };
  return { span: { type: "link", href, spans: parseInline(label) }, next };
}

function findCloser(source: string, from: number, marker: string): number {
  let cursor = from;
  while (cursor < source.length) {
    if (source[cursor] === "\\") {
      cursor += 2;
      continue;
    }
    if (source.startsWith(marker, cursor)) return cursor;
    cursor += 1;
  }
  return -1;
}

/** Plain text of rendered blocks, for previews and tests. */
export function markdownText(blocks: readonly MarkdownBlock[]): string {
  return blocks
    .map(blockText)
    .filter((text) => text !== "")
    .join("\n");
}

function blockText(block: MarkdownBlock): string {
  switch (block.type) {
    case "paragraph":
    case "heading":
      return spanText(block.spans);
    case "code":
      return block.text;
    case "quote":
      return markdownText(block.blocks);
    case "list":
      return block.items.map((item) => markdownText(item.blocks)).join("\n");
    case "rule":
      return "";
  }
}

function spanText(spans: readonly MarkdownSpan[]): string {
  return spans
    .map((span) => {
      switch (span.type) {
        case "text":
        case "code":
          return span.text;
        case "strong":
        case "emphasis":
        case "strike":
        case "link":
          return spanText(span.spans);
        case "break":
          return "\n";
      }
    })
    .join("");
}

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Plain-text export scope (parity/specs/exports.md §5, EXP-020; the fix for
// NEWFRONT-356): the image-free scope drops every Markdown image tag — the
// inline `![alt](url)` form the editor writes, plus the full, collapsed and
// shortcut reference forms — while keeping the surrounding text. Code spans
// and fenced blocks are literal text, not pictures, so they pass through
// untouched; a backslash escape is a literal character, not a picture.
import type { PageExportScope } from "./pageExportModel.js";

/**
 * Where the fenced code block that starts at `start` ends, or `null` when
 * no fence opens there. A fence is three or more backticks or tildes at the
 * start of a line (up to three spaces of indent); it closes on a line whose
 * fence character repeats at least as many times, with nothing else on it.
 */
function fencedBlockEnd(markdown: string, start: number): number | null {
  if (start !== 0 && markdown[start - 1] !== "\n") return null;
  let cursor = start;
  let indent = 0;
  while (indent < 3 && markdown[cursor] === " ") {
    cursor += 1;
    indent += 1;
  }
  const fence = markdown[cursor];
  if (fence !== "`" && fence !== "~") return null;
  let length = 0;
  while (markdown[cursor + length] === fence) length += 1;
  if (length < 3) return null;
  const opening = markdown.slice(cursor + length);
  const lineEnd = opening.indexOf("\n");
  const info = lineEnd === -1 ? opening : opening.slice(0, lineEnd);
  // A backtick fence's info string cannot hold a backtick; that spelling is
  // an inline code span instead.
  if (fence === "`" && info.includes("`")) return null;
  let scan = lineEnd === -1 ? markdown.length : cursor + length + lineEnd + 1;
  while (scan < markdown.length) {
    const newline = markdown.indexOf("\n", scan);
    const next = newline === -1 ? markdown.length : newline;
    const line = markdown.slice(scan, next);
    // A closing fence repeats the opening character at least `length` times
    // and carries nothing else; up to three spaces of indent are allowed.
    let offset = 0;
    while (offset < 3 && line[offset] === " ") offset += 1;
    const body = line.slice(offset);
    if (body.length >= length && body === fence.repeat(body.length)) return next;
    scan = next + 1;
  }
  return markdown.length;
}

/** End of an inline code span opening at `start`, or `null` when unclosed. */
function codeSpanEnd(markdown: string, start: number): number | null {
  let length = 0;
  while (markdown[start + length] === "`") length += 1;
  let cursor = start + length;
  while (cursor < markdown.length) {
    if (markdown[cursor] !== "`") {
      cursor += 1;
      continue;
    }
    let run = 0;
    while (markdown[cursor + run] === "`") run += 1;
    if (run === length) return cursor + run;
    cursor += run;
  }
  return null;
}

/** Index just past the `]` that closes an image label opened at `start`. */
function labelEnd(markdown: string, start: number): number | null {
  let depth = 0;
  let cursor = start + 2;
  while (cursor < markdown.length) {
    const ch = markdown[cursor];
    if (ch === "\\") {
      cursor += 2;
      continue;
    }
    if (ch === "\n") return null;
    if (ch === "[") depth += 1;
    if (ch === "]") {
      if (depth === 0) return cursor + 1;
      depth -= 1;
    }
    cursor += 1;
  }
  return null;
}

/**
 * Index just past the `)` closing a destination opened at `start`. Balanced
 * parentheses belong to the address (a stored asset name can hold them), and
 * an angle-bracketed destination takes any inner punctuation literally.
 */
function destinationEnd(markdown: string, start: number): number | null {
  let cursor = start + 1;
  let depth = 0;
  let angle = false;
  while (cursor < markdown.length) {
    const ch = markdown[cursor];
    if (ch === "\\") {
      cursor += 2;
      continue;
    }
    if (ch === "\n") return null;
    if (angle) {
      if (ch === ">") angle = false;
      cursor += 1;
      continue;
    }
    if (ch === "<" && depth === 0) {
      angle = true;
      cursor += 1;
      continue;
    }
    if (ch === "(") depth += 1;
    if (ch === ")") {
      if (depth === 0) return cursor + 1;
      depth -= 1;
    }
    cursor += 1;
  }
  return null;
}

/** Index just past the `]` closing a reference opened at `start`. */
function referenceEnd(markdown: string, start: number): number | null {
  let cursor = start + 1;
  while (cursor < markdown.length) {
    const ch = markdown[cursor];
    if (ch === "\\") {
      cursor += 2;
      continue;
    }
    if (ch === "\n" || ch === "[") return null;
    if (ch === "]") return cursor + 1;
    cursor += 1;
  }
  return null;
}

/** Index just past one complete Markdown image tag opened at `start`. */
function imageTagEnd(markdown: string, start: number): number | null {
  const label = labelEnd(markdown, start);
  if (label === null) return null;
  const next = markdown[label];
  if (next === "(") return destinationEnd(markdown, label);
  if (next === "[") return referenceEnd(markdown, label);
  return label;
}

/** Remove every Markdown image tag from the text. */
export function stripMarkdownImages(markdown: string): string {
  let kept = "";
  let cursor = 0;
  while (cursor < markdown.length) {
    const fenced = fencedBlockEnd(markdown, cursor);
    if (fenced !== null) {
      kept += markdown.slice(cursor, fenced);
      cursor = fenced;
      continue;
    }
    const ch = markdown[cursor];
    if (ch === "\\") {
      kept += markdown.slice(cursor, cursor + 2);
      cursor += 2;
      continue;
    }
    if (ch === "`") {
      const span = codeSpanEnd(markdown, cursor);
      if (span !== null) {
        kept += markdown.slice(cursor, span);
        cursor = span;
        continue;
      }
    }
    if (ch === "!" && markdown[cursor + 1] === "[") {
      const tag = imageTagEnd(markdown, cursor);
      if (tag !== null) {
        cursor = tag;
        continue;
      }
    }
    kept += ch;
    cursor += 1;
  }
  return kept;
}

/** Honor the content scope: full fidelity keeps the markup untouched. */
export function applyMarkdownScope(markdown: string, scope: PageExportScope): string {
  if (scope === "No images") return stripMarkdownImages(markdown);
  return markdown;
}

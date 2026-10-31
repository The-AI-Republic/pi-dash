// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared PDF-byte reader for the page-export parity scenarios and the
// page-export slice's unit tests (EXP-018/019). Moved out of
// page-export.spec.ts so both read the same facts the same way. The
// bfchar path matches the oracle original; the bfrange path is additive
// (the new app's renderer emits bfrange-array CMaps while the old app's
// emits bfchar, so old-app bytes decode exactly as before).
import { inflateSync } from "node:zlib";

/** Facts introspected from a downloaded document's bytes (identity-H CMaps decoded per font). */
export interface PdfFacts {
  mediaBoxes: string[];
  baseFonts: string[];
  imageCount: number;
  text: string;
}

/** Decode one bfrange destination (one glyph's UTF-16 units, space-separated when astral). */
function decodeBfrangeDestination(units: string): string {
  let out = "";
  for (const unit of units.split(/\s+/)) {
    if (unit === "") continue;
    out += String.fromCharCode(parseInt(unit, 16));
  }
  return out;
}

/** Glyph-code (uppercase hex) to decoded text for every bfrange array block in a CMap. */
function parseBfrange(text: string): Map<string, string> {
  const map = new Map<string, string>();
  for (const block of text.matchAll(/beginbfrange([\s\S]*?)endbfrange/g)) {
    const body = block[1] ?? "";
    for (const row of body.matchAll(/<([0-9a-fA-F]+)>\s*<([0-9a-fA-F]+)>\s*\[([\s\S]*?)\]/g)) {
      const start = parseInt(row[1] ?? "0", 16);
      const destinations = [...(row[3] ?? "").matchAll(/<([0-9a-fA-F\s]+)>/g)];
      destinations.forEach((destination, index) => {
        map.set(
          (start + index).toString(16).toUpperCase().padStart(4, "0"),
          decodeBfrangeDestination(destination[1] ?? "")
        );
      });
    }
  }
  return map;
}

/** Glyph-code (uppercase hex) to decoded text for the CMap's bfchar block. */
function parseBfchar(text: string): Map<string, string> {
  const map = new Map<string, string>();
  const body = /beginbfchar([\s\S]*?)endbfchar/.exec(text);
  if (!body) return map;
  for (const row of (body[1] ?? "").matchAll(/<([0-9a-fA-F]+)>\s*<([0-9a-fA-F]+)>/g)) {
    const uni = row[2] ?? "";
    map.set((row[1] ?? "").toUpperCase(), String.fromCodePoint(parseInt(uni.slice(-4), 16)));
  }
  return map;
}

export function pdfFacts(file: { content: string }): PdfFacts {
  const bytes = Buffer.from(file.content, "latin1");
  const latin = file.content;
  const mediaBoxes = [...latin.matchAll(/\/MediaBox\s*\[([\d.\s]+)\]/g)].map((m) =>
    (m[1] ?? "").trim().replace(/\s+/g, " ")
  );
  const baseFonts = [...new Set([...latin.matchAll(/\/BaseFont\s*\/([^\s/\]()<>]+)/g)].map((m) => m[1] ?? ""))];
  const imageCount = latin.match(/\/Subtype\s*\/Image/g)?.length ?? 0;

  const objects = new Map<string, string>();
  const objectAt = new Map<string, number>();
  for (const m of latin.matchAll(/(\d+) 0 obj([\s\S]*?)endobj/g)) {
    objects.set(m[1] ?? "", m[2] ?? "");
    objectAt.set(m[1] ?? "", m.index ?? 0);
  }
  const fontCmapObj = new Map<string, string>();
  for (const [num, dict] of objects) {
    if (!/\/Type\s*\/Font/.test(dict)) continue;
    const target = /\/ToUnicode\s+(\d+) 0 R/.exec(dict);
    if (target) fontCmapObj.set(num, target[1] ?? "");
  }
  const resourceFont = new Map<string, string>();
  for (const dict of objects.values()) {
    const fonts = /\/Font\s*<<([\s\S]*?)>>/.exec(dict);
    if (!fonts) continue;
    for (const ref of (fonts[1] ?? "").matchAll(/\/(\w+)\s+(\d+) 0 R/g)) resourceFont.set(ref[1] ?? "", ref[2] ?? "");
  }
  // latin1 is byte-faithful, so string offsets index the byte buffer exactly.
  const inflated = new Map<string, string>();
  for (const [num, dict] of objects) {
    if (!dict.includes("/FlateDecode") || !dict.includes("stream")) continue;
    const objStart = objectAt.get(num) ?? 0;
    const streamAt = latin.indexOf("stream", objStart) + "stream".length;
    const dataStart = streamAt + (latin[streamAt] === "\r" ? 2 : 1);
    const dataEnd = latin.indexOf("endstream", dataStart);
    if (dataEnd < 0) continue;
    try {
      inflated.set(num, inflateSync(bytes.subarray(dataStart, dataEnd)).toString("latin1"));
    } catch {
      // Not a deflate stream; ignore it.
    }
  }
  const cmapByObj = new Map<string, Map<string, string>>();
  for (const [num, text] of inflated) {
    const merged = new Map([...parseBfchar(text), ...parseBfrange(text)]);
    if (merged.size > 0) cmapByObj.set(num, merged);
  }
  const cmapForResource = (resource: string): Map<string, string> => {
    const font = resourceFont.get(resource);
    const target = font ? fontCmapObj.get(font) : undefined;
    if (!target) return new Map();
    return cmapByObj.get(target) ?? new Map();
  };
  const runs: string[] = [];
  for (const text of inflated.values()) {
    if (!text.includes("TJ")) continue;
    let current = new Map<string, string>();
    const tokens = text.match(/\/\w+\s+[\d.]+\s+Tf|\[.*?\]\s*TJ/gs) ?? [];
    for (const token of tokens) {
      const select = /^\/(\w+)\s/.exec(token);
      if (select) {
        current = cmapForResource(select[1] ?? "");
        continue;
      }
      let run = "";
      for (const hex of token.matchAll(/<([0-9a-fA-F]+)>/g)) {
        const digits = (hex[1] ?? "").toUpperCase();
        for (let at = 0; at + 4 <= digits.length; at += 4) run += current.get(digits.slice(at, at + 4)) ?? "";
      }
      if (run !== "") runs.push(run);
    }
  }
  return { mediaBoxes, baseFonts, imageCount, text: runs.join(" ") };
}

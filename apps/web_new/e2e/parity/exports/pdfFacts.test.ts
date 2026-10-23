// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared PDF-byte reader (EXP-018/019): hand-built documents prove the
// media-box, font, image and text facts for both CMap styles the two
// apps emit (bfchar on the old app, bfrange arrays on the new one).
import { deflateSync } from "node:zlib";
import { describe, expect, it } from "vitest";

import { pdfFacts } from "./pdfFacts.js";

/** One deflated stream object with byte-faithful latin1 framing. */
function streamObject(num: number, payload: string): string {
  const deflated = deflateSync(Buffer.from(payload, "latin1")).toString("latin1");
  return `${num} 0 obj\n<<\n/Length ${Buffer.byteLength(deflated, "latin1")}\n/Filter /FlateDecode\n>>\nstream\n${deflated}\nendstream\nendobj\n`;
}

/** Minimal document: one page, one font, one content stream, one CMap. */
function buildPdf(options: {
  mediaBox: string;
  baseFont: string;
  cmap: string;
  content: string;
  image?: boolean;
  extra?: string;
}): string {
  const imageObject =
    options.image === true ? "6 0 obj\n<<\n/Type /XObject\n/Subtype /Image\n/Width 1\n>>\nendobj\n" : "";
  return (
    "%PDF-1.4\n" +
    `1 0 obj\n<<\n/Type /Page\n/MediaBox [${options.mediaBox}]\n/Resources << /Font << /F1 3 0 R >> >>\n>>\nendobj\n` +
    `3 0 obj\n<<\n/Type /Font\n/Subtype /Type0\n/BaseFont /${options.baseFont}\n/ToUnicode 4 0 R\n>>\nendobj\n` +
    streamObject(2, options.content) +
    streamObject(4, options.cmap) +
    imageObject +
    (options.extra ?? "")
  );
}

const BFCHAR_CMAP = "1 beginbfchar\n<0001> <0044>\n<0002> <006f>\nendbfchar\n";
const BFRANGE_CMAP = "1 beginbfrange\n<0001> <0002> [<0044> <006f>]\nendbfrange\n";
const CONTENT = "BT\n/F1 12 Tf\n[<0001> 0 <0002>] TJ\nET\n";

describe("pdfFacts", () => {
  it("reads the oracle facts from a bfchar document", () => {
    const facts = pdfFacts({
      content: buildPdf({
        mediaBox: "0 0 595.280029 841.890015",
        baseFont: "ABCDEF+Inter-Regular",
        cmap: BFCHAR_CMAP,
        content: CONTENT,
        image: true,
      }),
    });
    expect(facts.mediaBoxes).toEqual(["0 0 595.280029 841.890015"]);
    expect(facts.baseFonts).toEqual(["ABCDEF+Inter-Regular"]);
    expect(facts.imageCount).toBe(1);
    expect(facts.text).toBe("Do");
  });

  it("reads the same text from a bfrange-array document, with astral pairs", () => {
    const cmap = "1 beginbfrange\n<0001> <0003> [<0044> <006f> < D83D DE00 >]\nendbfrange\n";
    const content = "BT\n/F1 12 Tf\n[<00010002> 0 <0003>] TJ\nET\n";
    const facts = pdfFacts({
      content: buildPdf({ mediaBox: "0 0 612 792", baseFont: "XYZ+Inter-Regular", cmap, content }),
    });
    expect(facts.mediaBoxes).toEqual(["0 0 612 792"]);
    expect(facts.imageCount).toBe(0);
    expect(facts.text).toBe("Do\u{1F600}");
  });

  it("skips corrupt, truncated and plain streams without losing the text", () => {
    const extra =
      "7 0 obj\n<<\n/Length 15\n/Filter /FlateDecode\n>>\nstream\nnot-deflated!!!\nendstream\nendobj\n" +
      "9 0 obj\n<<\n/Length 5\n>>\nstream\nhello\nendstream\nendobj\n" +
      "10 0 obj\n<<\n/Type /Font\n/BaseFont /ZZZ+Inter-Bold\n>>\nendobj\n" +
      "8 0 obj\n<<\n/Length 4\n/Filter /FlateDecode\n>>\nstream\nzzzz\nendobj\n";
    const facts = pdfFacts({
      content: buildPdf({
        mediaBox: "0 0 612 792",
        baseFont: "ABCDEF+Inter-Regular",
        cmap: BFRANGE_CMAP,
        content: CONTENT,
        extra,
      }),
    });
    expect(facts.text).toBe("Do");
    expect(facts.baseFonts).toEqual(["ABCDEF+Inter-Regular", "ZZZ+Inter-Bold"]);
  });

  it("decodes nothing for resources without a mapped CMap", () => {
    const content = "BT\n/F1 12 Tf\n[<0001>] TJ\n/F9 12 Tf\n[<0002>] TJ\nET\n";
    const facts = pdfFacts({
      content: buildPdf({ mediaBox: "0 0 612 792", baseFont: "ABCDEF+Inter-Regular", cmap: BFRANGE_CMAP, content }),
    });
    expect(facts.text).toBe("D");
  });
});

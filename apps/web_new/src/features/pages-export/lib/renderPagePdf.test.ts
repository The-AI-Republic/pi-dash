// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export document renderer (parity/specs/exports.md §5, EXP-019): the
// produced bytes carry the oracle's facts — header, exact media boxes,
// Inter-only subset fonts, decoded title/body text and picture objects —
// read through the same reader the parity scenarios use.
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import { pdfFacts } from "../../../../e2e/parity/exports/pdfFacts.js";
import type { PageExportSize } from "./pageExportModel.js";
import {
  loadInterFontBytes,
  pagePdfSize,
  renderPagePdf,
  toExportImageDataUri,
  type RenderPagePdfInput,
} from "./renderPagePdf.js";

const PIXEL_BASE64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
const PIXEL_PNG = new Uint8Array(Buffer.from(PIXEL_BASE64, "base64"));

/** The packaged Inter regular face, as bytes. */
function interFontBytes(): Uint8Array {
  const require = createRequire(import.meta.url);
  const root = dirname(require.resolve("@expo-google-fonts/inter/package.json"));
  return new Uint8Array(readFileSync(join(root, "400Regular", "Inter_400Regular.ttf")));
}

function input(overrides: Partial<RenderPagePdfInput> = {}): RenderPagePdfInput {
  return {
    title: "Doc019 probe",
    paragraphs: ["body marker probe"],
    imageBytes: [],
    size: "A4",
    ...overrides,
  };
}

/** Render with injected font bytes and read the oracle facts. */
async function renderFacts(overrides: Partial<RenderPagePdfInput> = {}) {
  const bytes = await renderPagePdf(input(overrides), { fontBytes: interFontBytes() });
  return { bytes, facts: pdfFacts({ content: Buffer.from(bytes).toString("latin1") }) };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("pagePdfSize", () => {
  it("maps dialog labels to renderer size names", () => {
    expect(pagePdfSize("A4")).toBe("A4");
    expect(pagePdfSize("A3")).toBe("A3");
    expect(pagePdfSize("A2")).toBe("A2");
    expect(pagePdfSize("Letter")).toBe("LETTER");
    expect(pagePdfSize("Legal")).toBe("LEGAL");
    expect(pagePdfSize("Tabloid")).toBe("TABLOID");
  });
});

describe("toExportImageDataUri", () => {
  it("detects png, jpeg, gif and webp from magic bytes", () => {
    expect(toExportImageDataUri(PIXEL_PNG)?.startsWith("data:image/png;base64,")).toBe(true);
    expect(toExportImageDataUri(new Uint8Array([0xff, 0xd8, 0xff, 0xe0]))?.startsWith("data:image/jpeg;base64,")).toBe(
      true
    );
    expect(
      toExportImageDataUri(new Uint8Array([0x47, 0x49, 0x46, 0x38, 0x39, 0x61]))?.startsWith("data:image/gif;base64,")
    ).toBe(true);
    expect(
      toExportImageDataUri(new Uint8Array([0x52, 0x49, 0x46, 0x46, 0, 0, 0, 0, 0x57, 0x45, 0x42, 0x50]))?.startsWith(
        "data:image/webp;base64,"
      )
    ).toBe(true);
  });

  it("rejects short, riff-not-webp and unknown bytes", () => {
    expect(toExportImageDataUri(new Uint8Array([0x89, 0x50]))).toBeNull();
    expect(
      toExportImageDataUri(new Uint8Array([0x52, 0x49, 0x46, 0x46, 0, 0, 0, 0, 0x41, 0x56, 0x49, 0x20]))
    ).toBeNull();
    expect(toExportImageDataUri(new Uint8Array([1, 2, 3, 4]))).toBeNull();
  });

  it("encodes large pictures across chunk boundaries", () => {
    const big = new Uint8Array(100_000);
    big.set([0x89, 0x50, 0x4e, 0x47]);
    const uri = toExportImageDataUri(big);
    expect(uri?.startsWith("data:image/png;base64,")).toBe(true);
    expect(Buffer.from(uri?.split(",")[1] ?? "", "base64")).toEqual(Buffer.from(big));
  });
});

describe("loadInterFontBytes", () => {
  it("fetches the typeface through its asset URL", async () => {
    const font = interFontBytes();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string | URL) => {
        expect(typeof url).toBe("string");
        return {
          ok: true,
          arrayBuffer: async () => font.buffer.slice(font.byteOffset, font.byteOffset + font.byteLength),
        };
      })
    );
    await expect(loadInterFontBytes()).resolves.toEqual(font);
  });
});

describe("renderPagePdf", () => {
  it("renders the default size with the oracle's A4 facts", async () => {
    const { facts } = await renderFacts({ imageBytes: [PIXEL_PNG] });
    expect(facts.mediaBoxes.length).toBeGreaterThan(0);
    for (const box of facts.mediaBoxes) expect(box).toBe("0 0 595.280029 841.890015");
    expect(facts.baseFonts.length).toBeGreaterThan(0);
    for (const font of facts.baseFonts) expect(font).toMatch(/Inter/i);
    expect(facts.text).toContain("Doc019 probe");
    expect(facts.text).toContain("body marker probe");
    expect(facts.imageCount).toBeGreaterThan(0);
  }, 60_000);

  it("renders every offered size at its media box", async () => {
    const boxes: Record<PageExportSize, string> = {
      A4: "0 0 595.280029 841.890015",
      A3: "0 0 841.890015 1190.550049",
      A2: "0 0 1190.550049 1683.780029",
      Letter: "0 0 612 792",
      Legal: "0 0 612 1008",
      Tabloid: "0 0 792 1224",
    };
    for (const [size, box] of Object.entries(boxes)) {
      const { facts } = await renderFacts({ size: size as PageExportSize });
      expect(facts.mediaBoxes.length).toBeGreaterThan(0);
      for (const mediaBox of facts.mediaBoxes) expect(mediaBox).toBe(box);
    }
  }, 120_000);

  it("drops pictures while keeping text when no image bytes arrive", async () => {
    const { facts } = await renderFacts({ imageBytes: [new Uint8Array([9, 9, 9, 9])] });
    expect(facts.imageCount).toBe(0);
    expect(facts.text).toContain("body marker probe");
  }, 60_000);

  it("renders a title-only document from empty paragraphs", async () => {
    const { bytes, facts } = await renderFacts({ paragraphs: [] });
    expect(Buffer.from(bytes).toString("latin1").startsWith("%PDF")).toBe(true);
    expect(facts.text).toContain("Doc019 probe");
  }, 60_000);

  it("loads the typeface through an injected loader", async () => {
    const bytes = await renderPagePdf(input(), { loadFontBytes: async () => interFontBytes() });
    expect(Buffer.from(bytes).toString("latin1").startsWith("%PDF")).toBe(true);
  }, 60_000);

  it("falls back to the production font loader with default deps", async () => {
    const font = interFontBytes();
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({
        ok: true,
        arrayBuffer: async () => font.buffer.slice(font.byteOffset, font.byteOffset + font.byteLength),
      }))
    );
    const bytes = await renderPagePdf(input());
    expect(Buffer.from(bytes).toString("latin1").startsWith("%PDF")).toBe(true);
  }, 60_000);
});

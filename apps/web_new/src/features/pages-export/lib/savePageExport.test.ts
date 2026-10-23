// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export file writer (parity/specs/exports.md §5, EXP-018–021): both
// formats render from host content with an injected download surface,
// asserting the file name and the bytes shape. (Node: the PDF stack's
// browser build cannot render inside jsdom, so the DOM extractor stands
// in here and the browser-surface test lives in savePageExportBrowser.)
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import { pdfFacts } from "../../../../e2e/parity/exports/pdfFacts.js";
import type { PageExportSink } from "./downloadSink.js";
import type { ExtractedExportContent } from "./extractExportContent.js";
import type { PageExportHost } from "./pageExportDialogRegistry.js";
import { savePageExport } from "./savePageExport.js";

const PIXEL_BASE64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
const PIXEL_PNG = new Uint8Array(Buffer.from(PIXEL_BASE64, "base64"));

function interFontBytes(): Uint8Array {
  const require = createRequire(import.meta.url);
  const root = dirname(require.resolve("@expo-google-fonts/inter/package.json"));
  return new Uint8Array(readFileSync(join(root, "400Regular", "Inter_400Regular.ttf")));
}

function host(): PageExportHost {
  return {
    pageTitle: "Doc019 probe",
    projectId: "project-1",
    workspaceSlug: "acme",
    getHtml: () => "<p>body marker probe</p>",
    getMarkdown: () => "body marker probe\n![undefined](https://example.com/shot.png)\n",
  };
}

function extracted(): ExtractedExportContent {
  return {
    paragraphs: ["body marker probe", "caption"],
    imageUrls: ["https://cdn.example/shot.png", "/api/assets/v2/uploaded"],
  };
}

interface SavedFile {
  blob: Blob;
  filename: string;
}

function recordingSink(): PageExportSink & { saved: SavedFile[]; revoked: string[] } {
  const saved: SavedFile[] = [];
  const revoked: string[] = [];
  const blobs = new Map<string, Blob>();
  return {
    saved,
    revoked,
    createObjectUrl: (blob) => {
      const url = `blob:save-${saved.length + revoked.length}`;
      blobs.set(url, blob);
      return url;
    },
    revokeObjectUrl: (url) => {
      revoked.push(url);
    },
    save: (url, filename) => {
      const blob = blobs.get(url);
      if (blob === undefined) throw new Error(`unknown object URL ${url}`);
      saved.push({ blob, filename });
    },
  };
}

async function savedBytes(sink: { saved: SavedFile[] }): Promise<Uint8Array> {
  const file = sink.saved[0];
  if (file === undefined) throw new Error("expected one saved file");
  return new Uint8Array(await file.blob.arrayBuffer());
}

async function savedText(sink: { saved: SavedFile[] }): Promise<string> {
  const file = sink.saved[0];
  if (file === undefined) throw new Error("expected one saved file");
  return file.blob.text();
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("savePageExport", () => {
  it("renders a document with pictures under full fidelity", async () => {
    const sink = recordingSink();
    const seen: string[] = [];
    const saved = await savePageExport(
      host(),
      { format: "PDF", scope: "Everything", size: "A4" },
      {
        sink,
        origin: "https://app.example",
        fontBytes: interFontBytes(),
        extractContent: () => extracted(),
        fetchBytes: async (url) => {
          seen.push(url);
          return PIXEL_PNG;
        },
      }
    );
    expect(saved).toEqual({ filename: "doc019-probe-a4.pdf" });
    expect(seen).toEqual(["https://cdn.example/shot.png", "https://app.example/api/assets/v2/uploaded"]);
    expect(sink.saved).toHaveLength(1);
    expect(sink.saved[0]?.filename).toBe("doc019-probe-a4.pdf");
    expect(sink.saved[0]?.blob.type).toBe("application/pdf");
    expect(sink.revoked).toHaveLength(1);
    const bytes = await savedBytes(sink);
    const latin = Buffer.from(bytes).toString("latin1");
    expect(latin.startsWith("%PDF")).toBe(true);
    const facts = pdfFacts({ content: latin });
    expect(facts.mediaBoxes.length).toBeGreaterThan(0);
    for (const box of facts.mediaBoxes) expect(box).toBe("0 0 595.280029 841.890015");
    for (const font of facts.baseFonts) expect(font).toMatch(/Inter/i);
    expect(facts.text).toContain("Doc019 probe");
    expect(facts.text).toContain("body marker probe");
    expect(facts.imageCount).toBeGreaterThan(0);
  }, 60_000);

  it("drops pictures without fetching under the image-free scope", async () => {
    const sink = recordingSink();
    const saved = await savePageExport(
      host(),
      { format: "PDF", scope: "No images", size: "Letter" },
      {
        sink,
        origin: "https://app.example",
        fontBytes: interFontBytes(),
        extractContent: () => extracted(),
        fetchBytes: async () => {
          throw new Error("must not fetch");
        },
      }
    );
    expect(saved.filename).toBe("doc019-probe-letter.pdf");
    const bytes = await savedBytes(sink);
    const facts = pdfFacts({ content: Buffer.from(bytes).toString("latin1") });
    expect(facts.mediaBoxes.length).toBeGreaterThan(0);
    for (const box of facts.mediaBoxes) expect(box).toBe("0 0 612 792");
    expect(facts.imageCount).toBe(0);
    expect(facts.text).toContain("body marker probe");
  }, 60_000);

  it("saves plain text honoring the scope", async () => {
    const full = recordingSink();
    await savePageExport(
      host(),
      { format: "Markdown", scope: "Everything", size: "A4" },
      {
        sink: full,
        origin: "https://app.example",
        extractContent: () => extracted(),
      }
    );
    expect(full.saved[0]?.filename).toBe("doc019-probe.md");
    expect(full.saved[0]?.blob.type).toBe("text/markdown");
    expect(await savedText(full)).toContain("![undefined](https://example.com/shot.png)");

    const stripped = recordingSink();
    await savePageExport(
      host(),
      { format: "Markdown", scope: "No images", size: "A4" },
      {
        sink: stripped,
        origin: "https://app.example",
        extractContent: () => extracted(),
      }
    );
    expect(stripped.saved[0]?.filename).toBe("doc019-probe.md");
    const text = await savedText(stripped);
    expect(text).toContain("body marker probe");
    expect(text).not.toContain("![undefined]");
  }, 60_000);

  it("falls back to the network fetch for pictures", async () => {
    const seen: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) => {
        seen.push(url);
        return { ok: true, arrayBuffer: async () => PIXEL_PNG.buffer.slice(0) } as Response;
      })
    );
    const sink = recordingSink();
    const saved = await savePageExport(
      host(),
      { format: "PDF", scope: "Everything", size: "A4" },
      {
        sink,
        origin: "https://app.example",
        fontBytes: interFontBytes(),
        extractContent: () => extracted(),
      }
    );
    expect(saved.filename).toBe("doc019-probe-a4.pdf");
    expect(seen).toEqual(["https://cdn.example/shot.png", "https://app.example/api/assets/v2/uploaded"]);
    const bytes = await savedBytes(sink);
    expect(pdfFacts({ content: Buffer.from(bytes).toString("latin1") }).imageCount).toBeGreaterThan(0);
  }, 60_000);

  it("still revokes the object URL when the save throws", async () => {
    const sink = recordingSink();
    sink.save = () => {
      throw new Error("sabotaged save");
    };
    await expect(
      savePageExport(
        host(),
        { format: "Markdown", scope: "Everything", size: "A4" },
        {
          sink,
          origin: "https://app.example",
          extractContent: () => extracted(),
        }
      )
    ).rejects.toThrow("sabotaged save");
    expect(sink.revoked).toHaveLength(1);
  });
});

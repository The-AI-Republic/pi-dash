// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export picture loading (parity/specs/exports.md §5, EXP-018):
// stack assets resolve and fetch, inline pictures decode, failures skip.
import { afterEach, describe, expect, it, vi } from "vitest";

import { browserFetchExportImageBytes, fetchExportImages } from "./fetchExportImages.js";

const PIXEL_BASE64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("browserFetchExportImageBytes", () => {
  it("reads the response bytes when found", async () => {
    const payload = new Uint8Array([137, 80, 78, 71]);
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ ok: true, arrayBuffer: async () => payload.buffer }) as Response)
    );
    await expect(browserFetchExportImageBytes("https://example.com/a.png")).resolves.toEqual(payload);
  });

  it("rejects when the picture is missing", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ ok: false, arrayBuffer: async () => new ArrayBuffer(0) }) as Response)
    );
    await expect(browserFetchExportImageBytes("https://example.com/gone.png")).rejects.toThrow("Could not load");
  });
});

describe("fetchExportImages", () => {
  it("resolves stack assets against the origin and fetches in order", async () => {
    const seen: string[] = [];
    const bytes = await fetchExportImages(
      ["/api/assets/v2/one", "https://example.com/two.png"],
      "https://app.example",
      async (url) => {
        seen.push(url);
        return new Uint8Array([url.length]);
      }
    );
    expect(seen).toEqual(["https://app.example/api/assets/v2/one", "https://example.com/two.png"]);
    expect(bytes).toHaveLength(2);
  });

  it("decodes inline pictures locally", async () => {
    const bytes = await fetchExportImages(
      [`data:image/png;base64,${PIXEL_BASE64}`],
      "https://app.example",
      async () => {
        throw new Error("must not fetch");
      }
    );
    expect(bytes).toHaveLength(1);
    expect(bytes[0]?.slice(0, 4)).toEqual(new Uint8Array([137, 80, 78, 71]));
  });

  it("skips unusable entries without failing", async () => {
    const bytes = await fetchExportImages(
      ["data:image/png,not-base64", `data:image/png;base64,%%%`, "http://[invalid", "https://example.com/gone.png"],
      "https://app.example",
      async () => {
        throw new Error("missing");
      }
    );
    expect(bytes).toEqual([]);
  });
});

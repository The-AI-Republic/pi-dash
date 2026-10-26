// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page upload paths (parity/specs/pages.md §11.4, PAGE-034/039): one
// file goes through the shared page-asset pipeline; a picture pasted
// from another page is cloned onto this page, and an upload failure
// keeps its safe message.

import { describe, expect, it, vi } from "vitest";

import { ApiError } from "@pidash/api-client";

import {
  clonePageImage,
  clonePastedAssets,
  pageImageUploadFailure,
  pastedAssetIds,
  rewritePastedAssets,
  uploadPageImage,
} from "./pageMedia.js";

const CLIENT = {} as never;
const FILE = new File(["bytes"], "shot.png", { type: "image/png" });

const pageAssets = vi.hoisted(() => ({ uploadPageAsset: vi.fn(), duplicatePageAsset: vi.fn(), pageAssetUrl: vi.fn() }));

vi.mock("@pidash/api-client", async (original) => {
  const actual = await original<typeof import("@pidash/api-client")>();
  return {
    ...actual,
    uploadPageAsset: pageAssets.uploadPageAsset,
    duplicatePageAsset: pageAssets.duplicatePageAsset,
    pageAssetUrl: pageAssets.pageAssetUrl,
  };
});

describe("uploadPageImage", () => {
  it("delegates to the page-asset pipeline with the page scope", async () => {
    pageAssets.uploadPageAsset.mockResolvedValue({ assetId: "asset-1", assetUrl: "/api/assets/asset-1/" });
    const uploaded = await uploadPageImage(CLIENT, {
      workspaceSlug: "acme",
      projectId: "project-1",
      pageId: "page-1",
      file: FILE,
    });
    expect(pageAssets.uploadPageAsset).toHaveBeenCalledWith(CLIENT, "acme", "project-1", "page-1", FILE, undefined);
    expect(uploaded).toEqual({ assetId: "asset-1", assetUrl: "/api/assets/asset-1/" });
  });

  it("passes an injected byte leg straight through", async () => {
    pageAssets.uploadPageAsset.mockResolvedValue({ assetId: "asset-2", assetUrl: "" });
    const uploadBytes = vi.fn();
    const uploaded = await uploadPageImage(
      CLIENT,
      { workspaceSlug: "acme", projectId: "project-1", pageId: "page-1", file: FILE },
      uploadBytes
    );
    expect(pageAssets.uploadPageAsset).toHaveBeenCalledWith(CLIENT, "acme", "project-1", "page-1", FILE, uploadBytes);
    expect(uploaded.assetId).toBe("asset-2");
  });

  it("propagates a refused upload", async () => {
    pageAssets.uploadPageAsset.mockRejectedValue(new Error("rejected"));
    await expect(
      uploadPageImage(CLIENT, { workspaceSlug: "acme", projectId: "project-1", pageId: "page-1", file: FILE })
    ).rejects.toThrow("rejected");
  });
});

describe("clonePageImage", () => {
  it("clones the source asset onto the target page and returns the fresh id", async () => {
    pageAssets.duplicatePageAsset.mockResolvedValue({ asset_id: "clone-1" });
    pageAssets.pageAssetUrl.mockReturnValue("/api/assets/v2/workspaces/acme/projects/p/clone-1/");
    const clone = await clonePageImage(CLIENT, {
      workspaceSlug: "acme",
      projectId: "p",
      pageId: "page-b",
      sourceAssetId: "asset-a",
    });
    expect(pageAssets.duplicatePageAsset).toHaveBeenCalledWith(CLIENT, "acme", "p", "asset-a", "page-b");
    expect(clone).toEqual({
      assetId: "clone-1",
      assetUrl: "/api/assets/v2/workspaces/acme/projects/p/clone-1/",
    });
  });
});

describe("pastedAssetIds", () => {
  it("reads bare image ids and skips absolute sources", () => {
    const html =
      '<image-component src="asset-a"></image-component>' +
      '<image-component src="https://cdn/x.png"></image-component>' +
      '<img src="/api/assets/v2/x">' +
      '<img src="asset-b">';
    expect(pastedAssetIds(html).sort()).toEqual(["asset-a", "asset-b"]);
  });

  it("returns nothing without a DOM parser", () => {
    const parser = globalThis.DOMParser;
    // @ts-expect-error the test deliberately removes the parser.
    delete globalThis.DOMParser;
    try {
      expect(pastedAssetIds("<img src=x>")).toEqual([]);
      expect(rewritePastedAssets("<img src=x>", new Map())).toBe("<img src=x>");
    } finally {
      globalThis.DOMParser = parser;
    }
  });
});

describe("clonePastedAssets and rewritePastedAssets", () => {
  it("names a pasted clone's row readably rather than by its UUID (PAGE-039)", async () => {
    pageAssets.duplicatePageAsset.mockResolvedValue({ asset_id: "clone-9" });
    pageAssets.pageAssetUrl.mockReturnValue("/served/clone-9/");
    const clones = await clonePastedAssets(
      CLIENT,
      { workspaceSlug: "acme", projectId: "p", pageId: "page-b" },
      '<image-component src="asset-a"></image-component>'
    );
    const rewritten = rewritePastedAssets('<image-component src="asset-a"></image-component>', clones);
    const parsed = new DOMParser().parseFromString(rewritten, "text/html");
    expect(parsed.querySelector("image-component")?.getAttribute("alt")).toBe("image-clone-9");
  });

  it("keeps an existing alt on a cloned picture", async () => {
    pageAssets.duplicatePageAsset.mockResolvedValue({ asset_id: "clone-10" });
    pageAssets.pageAssetUrl.mockReturnValue("/served/clone-10/");
    const clones = await clonePastedAssets(
      CLIENT,
      { workspaceSlug: "acme", projectId: "p", pageId: "page-b" },
      '<image-component src="asset-b" alt="Chart"></image-component>'
    );
    const rewritten = rewritePastedAssets('<image-component src="asset-b" alt="Chart"></image-component>', clones);
    expect(
      new DOMParser().parseFromString(rewritten, "text/html").querySelector("image-component")?.getAttribute("alt")
    ).toBe("Chart");
  });

  it("clones each foreign id and rewrites the markup to the new source", async () => {
    pageAssets.duplicatePageAsset.mockResolvedValue({ asset_id: "clone-2" });
    pageAssets.pageAssetUrl.mockReturnValue("/served/clone-2/");
    const clones = await clonePastedAssets(
      CLIENT,
      { workspaceSlug: "acme", projectId: "p", pageId: "page-b" },
      '<p><image-component src="asset-a"></image-component></p>'
    );
    expect([...clones.keys()]).toEqual(["asset-a"]);
    const rewritten = rewritePastedAssets('<p><image-component src="asset-a"></image-component></p>', clones);
    expect(rewritten).toContain('src="/served/clone-2/"');
    expect(rewritten).toContain('data-stored-src="clone-2"');
    expect(rewritten).toContain('status="uploaded"');
  });

  it("leaves markup untouched when nothing was cloned", () => {
    const html = '<image-component src="asset-a"></image-component>';
    expect(rewritePastedAssets(html, new Map())).toBe(html);
  });
});

describe("pageImageUploadFailure", () => {
  it("shows the error's own message", () => {
    expect(pageImageUploadFailure(new ApiError({ status: 413, code: "http", message: "Too large." }))).toBe(
      "Too large."
    );
  });

  it("falls back for a non-error, an empty message or whitespace", () => {
    expect(pageImageUploadFailure("boom")).toBe("The image could not be uploaded.");
    expect(pageImageUploadFailure(new Error("   "))).toBe("The image could not be uploaded.");
    expect(pageImageUploadFailure(undefined)).toBe("The image could not be uploaded.");
  });
});

describe("pasted asset edges", () => {
  it("ignores an image with neither stored nor served source", () => {
    expect(pastedAssetIds("<image-component></image-component>")).toEqual([]);
  });

  it("leaves an uncloned image untouched", () => {
    const html = '<image-component data-stored-src="a"></image-component>';
    expect(rewritePastedAssets(html, new Map())).toBe(html);
    const other = '<image-component data-stored-src="b"></image-component>';
    expect(rewritePastedAssets(other, new Map([["a", { assetId: "x", assetUrl: "" }]]))).toBe(other);
  });

  it("falls back to the asset id when the clone names no address", () => {
    const html = '<img src="a">';
    const rewritten = rewritePastedAssets(html, new Map([["a", { assetId: "fresh", assetUrl: "" }]]));
    expect(rewritten).toContain('src="fresh"');
    expect(rewritten).toContain('data-stored-src="fresh"');
  });
});

describe("asset source fallbacks", () => {
  it("reads the served source when no stored source is present", () => {
    expect(pastedAssetIds('<img src="served">')).toEqual(["served"]);
    expect(rewritePastedAssets('<img src="served">', new Map([["served", { assetId: "x", assetUrl: "" }]]))).toContain(
      'data-stored-src="x"'
    );
  });
});

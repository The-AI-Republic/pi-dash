// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Toolbar image upload (parity/specs/pages.md §5.3, §11.4, PAGE-034): one
// file goes through the shared page-asset contract and comes back as the
// stored id plus the address the editor paints.

import { describe, expect, it, vi } from "vitest";

import { ApiError } from "@pidash/api-client";

import { pageImageUploadFailure, uploadPageImage } from "./pageMedia.js";

const CLIENT = {} as never;
const FILE = new File(["bytes"], "shot.png", { type: "image/png" });

const pageAssets = vi.hoisted(() => ({ uploadPageAsset: vi.fn() }));

vi.mock("@pidash/api-client", async (original) => {
  const actual = await original<typeof import("@pidash/api-client")>();
  return { ...actual, uploadPageAsset: pageAssets.uploadPageAsset };
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

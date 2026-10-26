// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Toolbar image upload path (parity/specs/pages.md §5.3, §11.4,
// PAGE-034). One file goes through the shared file pipeline — the
// `@pidash/api-client` page-asset grant, byte leg and confirm — and comes
// back as the stored asset id plus the served address the editor paints.
// This is the toolbar entry's path; the block handlers (NEWFRONT-842)
// extend the same contract module rather than forking a copy, and the
// assets pane (NEWFRONT-843) resolves display and download from the id.

import { duplicatePageAsset, pageAssetUrl, uploadPageAsset, type PageAssetByteUpload } from "@pidash/api-client";

import type { ApiClient } from "@pidash/api-client";

/** What the editor needs after a successful upload. */
export interface UploadedPageImage {
  /** Stored asset id the body keeps in the picture node. */
  assetId: string;
  /** Served address the editing surface paints. */
  assetUrl: string;
}

/**
 * Upload one image for a page body. The page id scopes the parked asset
 * so the description write that follows already belongs to it; the caller
 * inserts `assetId` as the picture's stored source.
 */
export async function uploadPageImage(
  client: ApiClient,
  input: { workspaceSlug: string; projectId: string; pageId: string; file: File },
  uploadBytes?: PageAssetByteUpload
): Promise<UploadedPageImage> {
  const uploaded = await uploadPageAsset(
    client,
    input.workspaceSlug,
    input.projectId,
    input.pageId,
    input.file,
    uploadBytes
  );
  return { assetId: uploaded.assetId, assetUrl: uploaded.assetUrl };
}

/**
 * A notice for a refused upload. The upload contract rejects with an
 * `ApiError` whose message is safe to show; anything else (a network
 * failure, a hostile body) falls back to the generic wording so a stack
 * trace never reaches the page.
 */
export function pageImageUploadFailure(error: unknown): string {
  if (error instanceof Error && error.message.trim() !== "") return error.message;
  return "The image could not be uploaded.";
}

/**
 * Clone an existing page image onto `pageId` (PAGE-039). A paste or block
 * duplication from another page must not leave the body pointing at a
 * foreign asset id, so the duplicate-assets contract returns the fresh id
 * the cloned node stores.
 */
export async function clonePageImage(
  client: ApiClient,
  input: { workspaceSlug: string; projectId: string; pageId: string; sourceAssetId: string }
): Promise<UploadedPageImage> {
  const clone = await duplicatePageAsset(
    client,
    input.workspaceSlug,
    input.projectId,
    input.sourceAssetId,
    input.pageId
  );
  return { assetId: clone.asset_id, assetUrl: pageAssetUrl(input.workspaceSlug, input.projectId, clone.asset_id) };
}

/** Bare stored asset ids inside pasted HTML (absolute sources excluded). */
export function pastedAssetIds(html: string): string[] {
  if (typeof globalThis.DOMParser === "undefined") return [];
  const document = new DOMParser().parseFromString(html, "text/html");
  const ids = new Set<string>();
  for (const element of Array.from(document.querySelectorAll("image-component, img"))) {
    const src = element.getAttribute("data-stored-src") ?? element.getAttribute("src") ?? "";
    if (src === "" || /^(https?:|data:|blob:|\/)/i.test(src)) continue;
    ids.add(src);
  }
  return [...ids];
}

/** Clone every foreign id in pasted markup, returning old→new ids. */
export async function clonePastedAssets(
  client: ApiClient,
  input: { workspaceSlug: string; projectId: string; pageId: string },
  html: string
): Promise<Map<string, UploadedPageImage>> {
  const clones = new Map<string, UploadedPageImage>();
  for (const source of pastedAssetIds(html)) {
    clones.set(source, await clonePageImage(client, { ...input, sourceAssetId: source }));
  }
  return clones;
}

/** Rewrite pasted image sources onto their cloned asset ids. */
export function rewritePastedAssets(html: string, clones: ReadonlyMap<string, UploadedPageImage>): string {
  if (typeof globalThis.DOMParser === "undefined" || clones.size === 0) return html;
  const document = new DOMParser().parseFromString(html, "text/html");
  for (const element of Array.from(document.querySelectorAll("image-component, img"))) {
    const stored = element.getAttribute("data-stored-src") ?? element.getAttribute("src") ?? "";
    const clone = clones.get(stored);
    if (clone === undefined) continue;
    element.setAttribute("src", clone.assetUrl === "" ? clone.assetId : clone.assetUrl);
    element.setAttribute("data-stored-src", clone.assetId);
    element.setAttribute("status", "uploaded");
  }
  return document.body.innerHTML;
}

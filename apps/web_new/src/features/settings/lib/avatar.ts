// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Avatar picker rules (parity/specs/profile.md §6, PROF-012). The dialog
// accepts the four raster formats the product offers, at most five
// megabytes; rejections are inline. Removal picks the v2 user-asset route
// for an asset URL and the legacy user-file route for a plain reference.

/** Accepted image MIME types, in the order the dialog lists formats. */
export const AVATAR_ACCEPTED_TYPES: readonly string[] = ["image/jpeg", "image/png", "image/webp", "image/gif"];

/** The picker's accept attribute. */
export const AVATAR_ACCEPT = AVATAR_ACCEPTED_TYPES.join(",");

/** Five megabytes, the product's ceiling for an avatar image. */
export const AVATAR_MAX_BYTES = 5 * 1024 * 1024;

export const AVATAR_MESSAGES = {
  invalidFormat: "Please upload a file in a valid format.",
  tooLarge: "The image size cannot exceed 5 MB.",
} as const;

/** Why a picked file is refused, or null when it can upload. */
export function avatarFileError(file: { type: string; size: number }): string | null {
  if (!AVATAR_ACCEPTED_TYPES.includes(file.type)) return AVATAR_MESSAGES.invalidFormat;
  if (file.size > AVATAR_MAX_BYTES) return AVATAR_MESSAGES.tooLarge;
  return null;
}

/** The asset id a stored user-asset URL ends on, or null when it carries none. */
export function userAssetIdFromUrl(url: string): string | null {
  const segments = url.split("/").filter(Boolean);
  const last = segments[segments.length - 1];
  return last === undefined || last === "" ? null : last;
}

/**
 * Where a stored avatar reference is deleted from: a relative
 * `/api/assets/...` URL is a v2 asset (its final segment is the id), any
 * other reference is a legacy file key. Null means there is nothing to
 * delete.
 */
export type AvatarRemoval = { kind: "asset"; assetId: string } | { kind: "legacy"; assetKey: string } | null;

export function avatarRemovalFor(reference: string | null | undefined): AvatarRemoval {
  const trimmed = (reference ?? "").trim();
  if (trimmed === "") return null;
  const id = userAssetIdFromUrl(trimmed);
  if (id === null) return null;
  return trimmed.startsWith("/api/assets/") ? { kind: "asset", assetId: id } : { kind: "legacy", assetKey: id };
}

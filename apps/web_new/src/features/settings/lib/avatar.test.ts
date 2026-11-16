// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { AVATAR_MAX_BYTES, AVATAR_MESSAGES, avatarFileError, avatarRemovalFor, userAssetIdFromUrl } from "./avatar.js";

describe("avatar picker rules", () => {
  it("accepts the four raster formats under the size ceiling", () => {
    expect(avatarFileError({ type: "image/png", size: 10 })).toBeNull();
    expect(avatarFileError({ type: "image/gif", size: AVATAR_MAX_BYTES })).toBeNull();
  });

  it("rejects non-images and oversized files", () => {
    expect(avatarFileError({ type: "text/plain", size: 10 })).toBe(AVATAR_MESSAGES.invalidFormat);
    expect(avatarFileError({ type: "image/png", size: AVATAR_MAX_BYTES + 1 })).toBe(AVATAR_MESSAGES.tooLarge);
  });

  it("reads the asset id off a stored URL", () => {
    expect(userAssetIdFromUrl("/api/assets/v2/static/abc/")).toBe("abc");
    expect(userAssetIdFromUrl("https://cdn.test/a/b.png")).toBe("b.png");
    expect(userAssetIdFromUrl("")).toBeNull();
    expect(userAssetIdFromUrl("///")).toBeNull();
  });

  it("chooses the delete path for the stored reference", () => {
    expect(avatarRemovalFor("/api/assets/v2/static/abc/")).toEqual({ kind: "asset", assetId: "abc" });
    expect(avatarRemovalFor("legacy/key.png")).toEqual({ kind: "legacy", assetKey: "key.png" });
    expect(avatarRemovalFor("  legacy/key.png  ")).toEqual({ kind: "legacy", assetKey: "key.png" });
    expect(avatarRemovalFor("https://cdn.test/a/b.png")).toEqual({ kind: "legacy", assetKey: "b.png" });
    expect(avatarRemovalFor("/")).toBeNull();
    expect(avatarRemovalFor(undefined)).toBeNull();
    expect(avatarRemovalFor(null)).toBeNull();
    expect(avatarRemovalFor("")).toBeNull();
    expect(avatarRemovalFor("   ")).toBeNull();
  });
});

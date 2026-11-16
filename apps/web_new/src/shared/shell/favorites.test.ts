// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FAVORITES_SECTION_OPEN_KEY, isFavoritesSectionOpen, setFavoritesSectionOpen } from "./favorites.js";

beforeEach(() => {
  globalThis.localStorage.clear();
});

describe("favorites section flag", () => {
  it("starts closed on a missing key", () => {
    expect(isFavoritesSectionOpen()).toBe(false);
  });

  it("treats any value but the open mark as closed", () => {
    globalThis.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "yes");
    expect(isFavoritesSectionOpen()).toBe(false);
  });

  it("round-trips the open mark", () => {
    setFavoritesSectionOpen(true);
    expect(globalThis.localStorage.getItem(FAVORITES_SECTION_OPEN_KEY)).toBe("1");
    expect(isFavoritesSectionOpen()).toBe(true);
    setFavoritesSectionOpen(false);
    expect(globalThis.localStorage.getItem(FAVORITES_SECTION_OPEN_KEY)).toBeNull();
    expect(isFavoritesSectionOpen()).toBe(false);
  });

  it("reads closed and writes nothing without storage", () => {
    vi.stubGlobal("localStorage", undefined);
    try {
      expect(isFavoritesSectionOpen()).toBe(false);
      expect(() => setFavoritesSectionOpen(true)).not.toThrow();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("survives denied storage", () => {
    const denied = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
      removeItem: () => {
        throw new Error("denied");
      },
    };
    vi.stubGlobal("localStorage", denied);
    try {
      expect(isFavoritesSectionOpen()).toBe(false);
      expect(() => setFavoritesSectionOpen(true)).not.toThrow();
      expect(() => setFavoritesSectionOpen(false)).not.toThrow();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

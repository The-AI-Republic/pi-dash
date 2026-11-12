// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { LANGUAGE_STORAGE_KEY, registerLocale, resetLocalesForTests } from "../../../core/i18n/locales.js";

import { healSpaceLocale, storedSpaceLocaleNeedsHeal, useSpaceLocaleTag } from "./spaceLocale.js";

afterEach(() => {
  globalThis.localStorage?.clear();
  document.documentElement.removeAttribute("lang");
  resetLocalesForTests();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("storedSpaceLocaleNeedsHeal", () => {
  it("ignores a missing stored value", () => {
    expect(storedSpaceLocaleNeedsHeal()).toBe(false);
  });

  it("ignores a supported stored value", () => {
    globalThis.localStorage.setItem(LANGUAGE_STORAGE_KEY, "en");
    expect(storedSpaceLocaleNeedsHeal()).toBe(false);
  });

  it("flags an unsupported stored value", () => {
    globalThis.localStorage.setItem(LANGUAGE_STORAGE_KEY, "fr");
    expect(storedSpaceLocaleNeedsHeal()).toBe(true);
  });

  it("keeps a packed locale in builds that offer it", () => {
    registerLocale({ tag: "fr", label: "Francais" });
    globalThis.localStorage.setItem(LANGUAGE_STORAGE_KEY, "fr");
    expect(storedSpaceLocaleNeedsHeal()).toBe(false);
  });

  it("survives missing storage", () => {
    vi.stubGlobal("localStorage", undefined);
    expect(storedSpaceLocaleNeedsHeal()).toBe(false);
  });

  it("treats a throwing store as healthy", () => {
    const getItem = vi.spyOn(globalThis.localStorage, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(storedSpaceLocaleNeedsHeal()).toBe(false);
    expect(getItem).toHaveBeenCalled();
    getItem.mockRestore();
  });
});

describe("healSpaceLocale", () => {
  it("heals an unsupported stored tag to English and stamps the document", () => {
    globalThis.localStorage.setItem(LANGUAGE_STORAGE_KEY, "fr");
    expect(healSpaceLocale()).toBe("en");
    expect(globalThis.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("en");
    expect(document.documentElement.getAttribute("lang")).toBe("en");
  });

  it("keeps a supported tag and stamps it", () => {
    registerLocale({ tag: "fr", label: "Francais" });
    globalThis.localStorage.setItem(LANGUAGE_STORAGE_KEY, "fr");
    expect(healSpaceLocale()).toBe("fr");
    expect(globalThis.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("fr");
    expect(document.documentElement.getAttribute("lang")).toBe("fr");
  });

  it("defaults to English without writing when nothing is stored", () => {
    expect(healSpaceLocale()).toBe("en");
    expect(globalThis.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBeNull();
    expect(document.documentElement.getAttribute("lang")).toBe("en");
  });
});

describe("useSpaceLocaleTag", () => {
  it("heals on mount and returns the tag", () => {
    globalThis.localStorage.setItem(LANGUAGE_STORAGE_KEY, "fr");
    const { result } = renderHook(() => useSpaceLocaleTag());
    expect(result.current).toBe("en");
    expect(globalThis.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("en");
    expect(document.documentElement.getAttribute("lang")).toBe("en");
  });
});

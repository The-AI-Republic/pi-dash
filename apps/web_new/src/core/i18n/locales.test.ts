// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  isSupportedLocale,
  readStoredLanguage,
  registerLocale,
  resetLocalesForTests,
  resolveInitialLanguage,
  supportedLocales,
  writeStoredLanguage,
} from "./locales.js";

describe("locale model", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    resetLocalesForTests();
  });

  it("ships English only and layers fuller builds on top", () => {
    expect(supportedLocales()).toEqual([{ tag: "en", label: "English" }]);
    expect(isSupportedLocale("en")).toBe(true);
    expect(isSupportedLocale("fr")).toBe(false);
    registerLocale({ tag: "fr", label: "French" });
    expect(supportedLocales()).toEqual([
      { tag: "en", label: "English" },
      { tag: "fr", label: "French" },
    ]);
    registerLocale({ tag: "fr", label: "Français" });
    expect(supportedLocales()).toEqual([
      { tag: "en", label: "English" },
      { tag: "fr", label: "Français" },
    ]);
  });

  it("reads and writes the stored tag defensively", () => {
    vi.stubGlobal("localStorage", { getItem: () => "en", setItem: vi.fn() });
    expect(readStoredLanguage()).toBe("en");
    vi.stubGlobal("localStorage", { getItem: () => "fr" });
    expect(readStoredLanguage()).toBe("en");
    vi.stubGlobal("localStorage", undefined);
    expect(readStoredLanguage()).toBe("en");
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    });
    expect(readStoredLanguage()).toBe("en");
    expect(() => writeStoredLanguage("en")).not.toThrow();
    vi.stubGlobal("localStorage", undefined);
    expect(() => writeStoredLanguage("en")).not.toThrow();
    const setItem = vi.fn();
    vi.stubGlobal("localStorage", { getItem: () => null, setItem });
    writeStoredLanguage("en");
    expect(setItem).toHaveBeenCalledWith("userLanguage", "en");
  });

  it("resolves the starting tag without ever leaking unknown tags", () => {
    expect(resolveInitialLanguage("en", "en")).toBe("en");
    expect(resolveInitialLanguage("fr", "en")).toBe("en");
    expect(resolveInitialLanguage("xx", undefined)).toBe("en");
    registerLocale({ tag: "fr", label: "French" });
    expect(resolveInitialLanguage("fr", "en")).toBe("fr");
    expect(resolveInitialLanguage("xx", "fr")).toBe("fr");
  });
});

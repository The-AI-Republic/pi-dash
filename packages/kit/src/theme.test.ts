// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { afterEach, describe, expect, it, vi } from "vitest";
import { applyDensity, applyTheme, resolveTheme, watchSystemTheme } from "./theme";

afterEach(() => {
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-density");
  vi.unstubAllGlobals();
});

describe("resolveTheme", () => {
  it("passes light and dark through", () => {
    expect(resolveTheme("light")).toBe("light");
    expect(resolveTheme("dark")).toBe("dark");
  });

  it("follows the OS for system", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: true }) as MediaQueryList);
    expect(resolveTheme("system")).toBe("dark");
    vi.stubGlobal("matchMedia", () => ({ matches: false }) as MediaQueryList);
    expect(resolveTheme("system")).toBe("light");
  });
});

describe("applyTheme", () => {
  it("pins light and dark on the root", () => {
    applyTheme("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    applyTheme("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("removes the pin for system so CSS follows the OS live", () => {
    applyTheme("dark");
    applyTheme("system");
    expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
  });
});

describe("applyDensity", () => {
  it("switches between compact and comfortable", () => {
    applyDensity("comfortable");
    expect(document.documentElement.getAttribute("data-density")).toBe("comfortable");
    applyDensity("compact");
    expect(document.documentElement.getAttribute("data-density")).toBe("compact");
  });
});

describe("watchSystemTheme", () => {
  it("listens for OS changes and unsubscribes", () => {
    const add = vi.fn();
    const remove = vi.fn();
    vi.stubGlobal("matchMedia", () => ({ addEventListener: add, removeEventListener: remove }));
    const seen: Array<string> = [];
    const stop = watchSystemTheme((theme) => {
      seen.push(theme);
    });
    expect(add).toHaveBeenCalledWith("change", expect.any(Function));
    add.mock.calls[0]![1]({ matches: true });
    expect(seen).toEqual(["dark"]);
    stop();
    expect(remove).toHaveBeenCalledWith("change", expect.any(Function));
  });
});

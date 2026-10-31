// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  canWatchSystemTheme,
  readStoredSpaceTheme,
  resolveSpaceTheme,
  SPACE_THEME_STORAGE_KEY,
  useSpaceTheme,
  writeStoredSpaceTheme,
  type SpaceTheme,
} from "./spaceTheme.js";

interface StubbedMedia {
  listeners: Array<(event: { matches: boolean }) => void>;
  addEventListener: ReturnType<typeof vi.fn>;
  removeEventListener: ReturnType<typeof vi.fn>;
}

/** OS scheme stub that also records system-theme listeners. */
function stubColorScheme(dark: boolean): StubbedMedia {
  const stub: StubbedMedia = {
    listeners: [],
    addEventListener: vi.fn((_event: string, listener: (event: { matches: boolean }) => void) => {
      stub.listeners.push(listener);
    }),
    removeEventListener: vi.fn(),
  };
  vi.stubGlobal("matchMedia", () => ({ matches: dark, ...stub }));
  return stub;
}

afterEach(() => {
  globalThis.localStorage?.clear();
  document.documentElement.removeAttribute("data-theme");
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("readStoredSpaceTheme", () => {
  it("reads the stored explicit choice", () => {
    globalThis.localStorage.setItem(SPACE_THEME_STORAGE_KEY, "dark");
    expect(readStoredSpaceTheme()).toBe("dark");
  });

  it("treats missing and unknown values as no choice", () => {
    expect(readStoredSpaceTheme()).toBeNull();
    globalThis.localStorage.setItem(SPACE_THEME_STORAGE_KEY, "sepia");
    expect(readStoredSpaceTheme()).toBeNull();
  });

  it("survives unreadable storage", () => {
    vi.stubGlobal("localStorage", undefined);
    expect(readStoredSpaceTheme()).toBeNull();
    writeStoredSpaceTheme("dark");
    vi.unstubAllGlobals();
    expect(globalThis.localStorage.getItem(SPACE_THEME_STORAGE_KEY)).toBeNull();
  });

  it("treats a throwing store as no choice", () => {
    const getItem = vi.spyOn(globalThis.localStorage, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(readStoredSpaceTheme()).toBeNull();
    expect(getItem).toHaveBeenCalled();
    getItem.mockRestore();
  });
});

describe("writeStoredSpaceTheme", () => {
  it("persists the explicit choice", () => {
    writeStoredSpaceTheme("light");
    expect(globalThis.localStorage.getItem(SPACE_THEME_STORAGE_KEY)).toBe("light");
  });

  it("dismisses a refused write", () => {
    const setItem = vi.spyOn(globalThis.localStorage, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(() => writeStoredSpaceTheme("dark")).not.toThrow();
    expect(setItem).toHaveBeenCalled();
    setItem.mockRestore();
  });
});

describe("resolveSpaceTheme", () => {
  it("prefers the stored choice over the OS", () => {
    stubColorScheme(true);
    expect(resolveSpaceTheme("light")).toBe("light");
  });

  it("follows the OS without a stored choice", () => {
    stubColorScheme(true);
    expect(resolveSpaceTheme(null)).toBe("dark");
    stubColorScheme(false);
    expect(resolveSpaceTheme(null)).toBe("light");
  });
});

describe("canWatchSystemTheme", () => {
  it("needs both a window and the media query", () => {
    expect(canWatchSystemTheme()).toBe(false);
    stubColorScheme(false);
    expect(canWatchSystemTheme()).toBe(true);
    vi.stubGlobal("window", undefined);
    expect(canWatchSystemTheme()).toBe(false);
  });
});

describe("useSpaceTheme", () => {
  it("starts on the OS scheme and stores nothing", () => {
    stubColorScheme(true);
    const { result } = renderHook(() => useSpaceTheme());
    expect(result.current.theme).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(globalThis.localStorage.getItem(SPACE_THEME_STORAGE_KEY)).toBeNull();
  });

  it("flips the OS default on the first toggle and stores it", () => {
    stubColorScheme(true);
    const { result } = renderHook(() => useSpaceTheme());
    act(() => {
      result.current.toggle();
    });
    expect(result.current.theme).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(globalThis.localStorage.getItem(SPACE_THEME_STORAGE_KEY)).toBe("light");
  });

  it("alternates on later toggles", () => {
    stubColorScheme(false);
    globalThis.localStorage.setItem(SPACE_THEME_STORAGE_KEY, "dark");
    const { result } = renderHook(() => useSpaceTheme());
    expect(result.current.theme).toBe("dark");
    act(() => {
      result.current.toggle();
    });
    expect(result.current.theme).toBe("light");
    act(() => {
      result.current.toggle();
    });
    expect(result.current.theme).toBe("dark");
    expect(globalThis.localStorage.getItem(SPACE_THEME_STORAGE_KEY)).toBe("dark");
  });

  it("restores the stored choice across loads", () => {
    stubColorScheme(true);
    globalThis.localStorage.setItem(SPACE_THEME_STORAGE_KEY, "light");
    const { result } = renderHook(() => useSpaceTheme());
    expect(result.current.theme).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("follows OS changes live while nothing is stored", () => {
    const stub = stubColorScheme(false);
    const { result } = renderHook(() => useSpaceTheme());
    expect(result.current.theme).toBe("light");
    expect(stub.addEventListener).toHaveBeenCalledWith("change", expect.any(Function));
    act(() => {
      for (const listener of stub.listeners) listener({ matches: true });
    });
    expect(result.current.theme).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("ignores the OS once an explicit choice is stored", () => {
    const stub = stubColorScheme(false);
    globalThis.localStorage.setItem(SPACE_THEME_STORAGE_KEY, "light");
    const { result, unmount } = renderHook(() => useSpaceTheme());
    expect(stub.addEventListener).not.toHaveBeenCalled();
    expect(result.current.theme).toBe("light");
    unmount();
    expect(stub.removeEventListener).not.toHaveBeenCalled();
  });

  it("stops watching on unmount", () => {
    const stub = stubColorScheme(true);
    const { unmount } = renderHook(() => useSpaceTheme());
    unmount();
    expect(stub.removeEventListener).toHaveBeenCalledWith("change", expect.any(Function));
  });

  it("renders light without watching when the media query is missing", () => {
    const { result } = renderHook(() => useSpaceTheme());
    expect(result.current.theme).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("re-pins the resolved theme when an external writer overwrites it", async () => {
    stubColorScheme(false);
    globalThis.localStorage.setItem(SPACE_THEME_STORAGE_KEY, "light");
    const { result } = renderHook(() => useSpaceTheme());
    expect(result.current.theme).toBe("light");
    // A later passive effect (the app root's workspace-keyed mark) lands second.
    document.documentElement.setAttribute("data-theme", "dark");
    await act(async () => {});
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("releases the pin on unmount", async () => {
    stubColorScheme(false);
    const { unmount } = renderHook(() => useSpaceTheme());
    unmount();
    document.documentElement.setAttribute("data-theme", "dark");
    await act(async () => {});
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("holds no pin where mutation observers are missing", () => {
    stubColorScheme(false);
    vi.stubGlobal("MutationObserver", undefined);
    const { result } = renderHook(() => useSpaceTheme());
    expect(result.current.theme).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("keeps the document pin and the toggle state in agreement", () => {
    stubColorScheme(false);
    const seen: SpaceTheme[] = [];
    const { result } = renderHook(() => useSpaceTheme());
    for (let turn = 0; turn < 3; turn++) {
      seen.push(result.current.theme);
      expect(document.documentElement.getAttribute("data-theme")).toBe(result.current.theme);
      act(() => {
        result.current.toggle();
      });
    }
    expect(seen).toEqual(["light", "dark", "light"]);
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";
import { preferenceStorage } from "./preferenceStorage.js";

afterEach(() => {
  vi.restoreAllMocks();
  globalThis.localStorage.clear();
});

describe("preferenceStorage", () => {
  it("uses the browser profile when writable", () => {
    const storage = preferenceStorage();
    expect(storage).toBe(globalThis.localStorage);
    storage.setItem("pidash:test", "1");
    expect(globalThis.localStorage.getItem("pidash:test")).toBe("1");
  });

  it("falls back to memory when the profile refuses writes", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    const storage = preferenceStorage();
    expect(storage).not.toBe(globalThis.localStorage);
    storage.setItem("density", "icons");
    expect(storage.getItem("density")).toBe("icons");
    expect(storage.getItem("missing")).toBeNull();
    storage.removeItem("density");
    expect(storage.getItem("density")).toBeNull();
  });

  it("shares one memory fallback across calls", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    const first = preferenceStorage();
    const second = preferenceStorage();
    expect(second).toBe(first);
    first.setItem("shared", "yes");
    expect(second.getItem("shared")).toBe("yes");
  });
});

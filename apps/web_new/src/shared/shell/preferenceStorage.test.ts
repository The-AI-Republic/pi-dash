// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";
import { loadPreference, preferenceStorage, storePreference } from "./preferenceStorage.js";

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

describe("loadPreference", () => {
  const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null;

  it("reads a stored payload", () => {
    storePreference("pref:read", { density: "icons" });
    expect(loadPreference("pref:read", {}, isRecord)).toEqual({ density: "icons" });
  });

  it("falls back on a missing key", () => {
    expect(loadPreference("pref:missing", { density: "labeled" }, isRecord)).toEqual({ density: "labeled" });
  });

  it("falls back on corrupt JSON", () => {
    globalThis.localStorage.setItem("pref:corrupt", "{oops");
    expect(loadPreference("pref:corrupt", { density: "labeled" }, isRecord)).toEqual({ density: "labeled" });
  });

  it("falls back when validation fails", () => {
    globalThis.localStorage.setItem("pref:shape", '"just a string"');
    expect(loadPreference("pref:shape", { density: "labeled" }, isRecord)).toEqual({ density: "labeled" });
  });

  it("falls back when storage throws", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(loadPreference("pref:read", { density: "labeled" }, isRecord)).toEqual({ density: "labeled" });
  });

  it("round-trips through the memory fallback when the profile refuses writes", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    storePreference("pref:memory", { density: "icons" });
    expect(loadPreference("pref:memory", {}, isRecord)).toEqual({ density: "icons" });
  });
});

describe("storePreference", () => {
  it("swallows write failures", () => {
    // The memory fallback's setItem never throws, so reach the catch by
    // breaking serialization instead: circular JSON.stringify throws.
    const circular: Record<string, unknown> = {};
    circular.self = circular;
    expect(() => storePreference("pref:unserializable", circular)).not.toThrow();
  });
});

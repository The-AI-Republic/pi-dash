// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rail visibility (parity/specs/agents.md §5, AGT-016): the hidden set is
// keyed per workspace and project in local storage, survives a reload,
// and treats broken storage or a malformed value as "nothing hidden".
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createMemoryStorage } from "../../testSupport/memoryStorage.js";

import {
  browserStorage,
  readHiddenSchedulers,
  useSchedulerVisibility,
  visibilityStorageKey,
  writeHiddenSchedulers,
} from "./visibility.js";

const SLUG = "acme";
const PROJECT = "p1";

function install(storage: Storage): void {
  Object.defineProperty(window, "localStorage", { value: storage, configurable: true, writable: true });
}

beforeEach(() => install(createMemoryStorage()));
afterEach(() => install(createMemoryStorage()));

describe("storage key", () => {
  it("scopes the preference to one project", () => {
    expect(visibilityStorageKey(SLUG, PROJECT)).toBe("pidash:scheduler-calendar-hidden:acme:p1");
  });
});

describe("browserStorage", () => {
  it("reads the page storage and answers null without a page", () => {
    expect(browserStorage()).toBe(window.localStorage);
    expect(browserStorage(null)).toBeNull();
  });
});

describe("readHiddenSchedulers", () => {
  it("reads nothing before the first choice", () => {
    expect(readHiddenSchedulers(window.localStorage, SLUG, PROJECT)).toEqual([]);
  });

  it("round-trips the hidden ids", () => {
    writeHiddenSchedulers(window.localStorage, SLUG, PROJECT, ["s1", "s2"]);
    expect(readHiddenSchedulers(window.localStorage, SLUG, PROJECT)).toEqual(["s1", "s2"]);
  });

  it("keeps projects independent", () => {
    writeHiddenSchedulers(window.localStorage, SLUG, PROJECT, ["s1"]);
    expect(readHiddenSchedulers(window.localStorage, SLUG, "p2")).toEqual([]);
  });

  it("ignores a malformed or non-list value", () => {
    window.localStorage.setItem(visibilityStorageKey(SLUG, PROJECT), "{not json");
    expect(readHiddenSchedulers(window.localStorage, SLUG, PROJECT)).toEqual([]);
    window.localStorage.setItem(visibilityStorageKey(SLUG, PROJECT), JSON.stringify({ id: "s1" }));
    expect(readHiddenSchedulers(window.localStorage, SLUG, PROJECT)).toEqual([]);
    window.localStorage.setItem(visibilityStorageKey(SLUG, PROJECT), JSON.stringify(["s1", 3, null]));
    expect(readHiddenSchedulers(window.localStorage, SLUG, PROJECT)).toEqual(["s1"]);
  });

  it("reads empty when the browser refuses the storage getter", () => {
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get: () => {
        throw new Error("denied");
      },
    });
    const { result } = renderHook(() => useSchedulerVisibility(SLUG, PROJECT));
    expect(result.current.isVisible("s1")).toBe(true);
    act(() => result.current.toggle("s1"));
    expect(result.current.isVisible("s1")).toBe(false);
  });

  it("stays quiet without a storage area or when it throws", () => {
    expect(readHiddenSchedulers(null, SLUG, PROJECT)).toEqual([]);
    const throwing = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    };
    expect(readHiddenSchedulers(throwing, SLUG, PROJECT)).toEqual([]);
    expect(() => writeHiddenSchedulers(throwing, SLUG, PROJECT, ["s1"])).not.toThrow();
    expect(() => writeHiddenSchedulers(null, SLUG, PROJECT, ["s1"])).not.toThrow();
  });
});

describe("useSchedulerVisibility", () => {
  it("loads the stored choice on mount", () => {
    writeHiddenSchedulers(window.localStorage, SLUG, PROJECT, ["s2"]);
    const { result } = renderHook(() => useSchedulerVisibility(SLUG, PROJECT));
    expect(result.current.isVisible("s1")).toBe(true);
    expect(result.current.isVisible("s2")).toBe(false);
  });

  it("toggles on and off and persists each change", () => {
    const { result } = renderHook(() => useSchedulerVisibility(SLUG, PROJECT));
    act(() => result.current.toggle("s1"));
    expect(result.current.isVisible("s1")).toBe(false);
    expect(readHiddenSchedulers(window.localStorage, SLUG, PROJECT)).toEqual(["s1"]);
    act(() => result.current.toggle("s1"));
    expect(result.current.isVisible("s1")).toBe(true);
    expect(readHiddenSchedulers(window.localStorage, SLUG, PROJECT)).toEqual([]);
  });

  it("reads each project's own set when the project changes in place", () => {
    writeHiddenSchedulers(window.localStorage, SLUG, "p1", ["s1"]);
    writeHiddenSchedulers(window.localStorage, SLUG, "p2", ["s2"]);
    const { result, rerender } = renderHook(
      ({ project }: { project: string }) => useSchedulerVisibility(SLUG, project),
      { initialProps: { project: "p1" } }
    );
    expect(result.current.isVisible("s1")).toBe(false);
    expect(result.current.isVisible("s2")).toBe(true);

    rerender({ project: "p2" });
    expect(result.current.isVisible("s1")).toBe(true);
    expect(result.current.isVisible("s2")).toBe(false);

    act(() => result.current.toggle("s1"));
    expect(readHiddenSchedulers(window.localStorage, SLUG, "p1")).toEqual(["s1"]);
    expect(readHiddenSchedulers(window.localStorage, SLUG, "p2")).toEqual(["s2", "s1"]);
  });

  it("starts a project without a stored set empty after a switch", () => {
    const { result, rerender } = renderHook(
      ({ project }: { project: string }) => useSchedulerVisibility(SLUG, project),
      { initialProps: { project: "p1" } }
    );
    act(() => result.current.hideAll(["s1"]));
    rerender({ project: "p2" });
    expect(result.current.isVisible("s1")).toBe(true);
    expect(readHiddenSchedulers(window.localStorage, SLUG, "p2")).toEqual([]);
    expect(readHiddenSchedulers(window.localStorage, SLUG, "p1")).toEqual(["s1"]);
  });

  it("hides all and shows all", () => {
    const { result } = renderHook(() => useSchedulerVisibility(SLUG, PROJECT));
    act(() => result.current.hideAll(["s1", "s2"]));
    expect([...result.current.hidden]).toEqual(["s1", "s2"]);
    expect(readHiddenSchedulers(window.localStorage, SLUG, PROJECT)).toEqual(["s1", "s2"]);
    act(() => result.current.showAll());
    expect(result.current.isVisible("s1")).toBe(true);
    expect(readHiddenSchedulers(window.localStorage, SLUG, PROJECT)).toEqual([]);
  });
});

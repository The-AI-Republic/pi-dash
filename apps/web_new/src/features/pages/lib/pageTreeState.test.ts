// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// In-session expansion store (parity/specs/pages.md §12.2, PAGE-085): one
// set per project, toggling broadcasts to every reader, and a reset clears
// only the scope it names.

import { act, renderHook } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  expandedPageIds,
  resetPageExpansion,
  setPageExpanded,
  subscribePageExpansion,
  togglePageExpanded,
  usePageExpansion,
} from "./pageTreeState.js";

const PROJECT_A = "project-a";
const PROJECT_B = "project-b";

afterEach(() => {
  resetPageExpansion();
});

describe("pageTreeState", () => {
  it("keeps expansion per workspace and project", () => {
    setPageExpanded("acme", PROJECT_A, "page-1", true);
    setPageExpanded("acme", PROJECT_B, "page-2", true);
    expect([...expandedPageIds("acme", PROJECT_A)]).toEqual(["page-1"]);
    expect([...expandedPageIds("acme", PROJECT_B)]).toEqual(["page-2"]);
    expect([...expandedPageIds("other", PROJECT_A)]).toEqual([]);
  });

  it("reports the new state from a toggle without emitting a no-op twice", () => {
    const listener = vi.fn();
    const unsubscribe = subscribePageExpansion(listener);
    expect(togglePageExpanded("acme", PROJECT_A, "page-1")).toBe(true);
    expect(togglePageExpanded("acme", PROJECT_A, "page-1")).toBe(false);
    expect(listener).toHaveBeenCalledTimes(2);
    setPageExpanded("acme", PROJECT_A, "page-1", false);
    expect(listener).toHaveBeenCalledTimes(2);
    unsubscribe();
  });

  it("stops notifying after unsubscribe", () => {
    const listener = vi.fn();
    subscribePageExpansion(listener)();
    setPageExpanded("acme", PROJECT_A, "page-1", true);
    expect(listener).not.toHaveBeenCalled();
  });

  it("clears one scope or every scope", () => {
    setPageExpanded("acme", PROJECT_A, "page-1", true);
    setPageExpanded("acme", PROJECT_B, "page-2", true);
    resetPageExpansion("acme", PROJECT_A);
    expect([...expandedPageIds("acme", PROJECT_A)]).toEqual([]);
    expect([...expandedPageIds("acme", PROJECT_B)]).toEqual(["page-2"]);
    resetPageExpansion();
    expect([...expandedPageIds("acme", PROJECT_B)]).toEqual([]);
  });

  it("reads the expansion scope during a server render", () => {
    setPageExpanded("acme", PROJECT_A, "page-1", true);
    function Reader(): React.ReactElement {
      return React.createElement("span", null, [...usePageExpansion("acme", PROJECT_A)].join(","));
    }
    expect(renderToStaticMarkup(React.createElement(Reader))).toBe("<span>page-1</span>");
  });

  it("re-renders a mounted reader when its scope changes", () => {
    const { result } = renderHook(() => usePageExpansion("acme", PROJECT_A));
    expect([...result.current]).toEqual([]);
    act(() => {
      togglePageExpanded("acme", PROJECT_A, "page-1");
    });
    expect([...result.current]).toEqual(["page-1"]);
    act(() => {
      resetPageExpansion("acme", PROJECT_A);
    });
    expect([...result.current]).toEqual([]);
  });
});

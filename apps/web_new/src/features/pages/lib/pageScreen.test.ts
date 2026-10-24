// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages body-state order and title (PAGE-001/019/020).

import { describe, expect, it } from "vitest";

import { PAGES_CRUMB, pagesTitle, resolvePagesBody, type PagesBodyInput } from "./pageScreen.js";

const BASE: PagesBodyInput = {
  projectPending: false,
  projectError: false,
  gateOff: false,
  pagesPending: false,
  pagesError: false,
  pageCount: 1,
  tabCount: 1,
  resultCount: 1,
  searchActive: false,
  filtersActive: false,
};

describe("resolvePagesBody", () => {
  it("puts the project read and gate before the list read", () => {
    expect(resolvePagesBody({ ...BASE, projectPending: true, gateOff: true })).toBe("skeleton");
    expect(resolvePagesBody({ ...BASE, projectError: true, gateOff: true })).toBe("error");
    expect(resolvePagesBody({ ...BASE, gateOff: true, pagesPending: true })).toBe("gate");
    expect(resolvePagesBody({ ...BASE, pagesPending: true })).toBe("skeleton");
    expect(resolvePagesBody({ ...BASE, pagesError: true })).toBe("zero");
  });

  it("distinguishes a truly empty collection and tab from narrowed results", () => {
    expect(resolvePagesBody({ ...BASE, pageCount: 0, tabCount: 0, resultCount: 0 })).toBe("zero");
    expect(resolvePagesBody({ ...BASE, tabCount: 0, resultCount: 0 })).toBe("zero");
    expect(resolvePagesBody({ ...BASE, resultCount: 0, searchActive: true })).toBe("no-results");
    expect(resolvePagesBody({ ...BASE, resultCount: 0, filtersActive: true })).toBe("no-results");
    expect(resolvePagesBody({ ...BASE, resultCount: 0 })).toBe("zero");
    expect(resolvePagesBody(BASE)).toBe("list");
  });

  it("titles the tab by project", () => {
    expect(pagesTitle("Website relaunch")).toBe("Website relaunch - Pages");
    expect(PAGES_CRUMB).toBe("Pages");
  });
});

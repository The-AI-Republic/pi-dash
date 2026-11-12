// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Body paint cache (PAGE-079): the skeleton is a cold-load surface only.

import { afterEach, describe, expect, it } from "vitest";

import { isPageBodyReady, markPageBodyReady, resetPageBodyCache } from "./pageBodyCache.js";

afterEach(() => {
  resetPageBodyCache();
});

describe("page body cache", () => {
  it("starts cold for every page", () => {
    expect(isPageBodyReady("page-1")).toBe(false);
  });

  it("remembers each page independently", () => {
    markPageBodyReady("page-1");
    expect(isPageBodyReady("page-1")).toBe(true);
    expect(isPageBodyReady("page-2")).toBe(false);
  });

  it("is idempotent and clears on reset", () => {
    markPageBodyReady("page-1");
    markPageBodyReady("page-1");
    resetPageBodyCache();
    expect(isPageBodyReady("page-1")).toBe(false);
  });
});

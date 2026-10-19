// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { IssueListFallback, LazyAnalyticsEntry, LazyIssueList, preloadIssueListLeaves } from "./lazyLeaves.js";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("issue list lazy leaves", () => {
  it("resolves both leaf chunks", async () => {
    await expect(preloadIssueListLeaves()).resolves.toBeUndefined();
  });

  it("rides React.lazy so the leaves stay out of the eager module", () => {
    expect(LazyIssueList.$$typeof).toBe(Symbol.for("react.lazy"));
    expect(LazyAnalyticsEntry.$$typeof).toBe(Symbol.for("react.lazy"));
  });

  it("shows the list skeleton as the list leaf's fallback", () => {
    render(<IssueListFallback />);
    expect(screen.getByTestId("issues-skeleton-list")).toBeInTheDocument();
  });

  it("swallows a preload failure so the route's own boundary reports it", async () => {
    vi.resetModules();
    vi.doMock("./IssueList.js", () => {
      throw new Error("chunk unavailable");
    });
    const leaves = await import("./lazyLeaves.js");
    await expect(leaves.preloadIssueListLeaves()).resolves.toBeUndefined();
    vi.doUnmock("./IssueList.js");
    vi.resetModules();
  });
});

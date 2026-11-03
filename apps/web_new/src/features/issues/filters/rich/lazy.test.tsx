// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The issues screen renders the rich filters through a chunk the route
// awaits before rendering (NEWFRONT-911). These tests pin the contract the
// route relies on: one shared request, a resolved component after it
// settles, and a loud failure when the route renders before awaiting it.
import { describe, expect, it, vi } from "vitest";

const filters = vi.hoisted(() => ({
  default: (props: { marker: string }) => <div data-testid="filters">{props.marker}</div>,
}));

vi.mock("./IssueFilters.js", () => ({ default: filters.default }));

import { loadIssueFilters, resolvedIssueFilters } from "./lazy.js";

describe("issues rich filters chunk", () => {
  it("rejects rendering before the chunk has loaded", () => {
    expect(() => resolvedIssueFilters()).toThrow(/await loadIssueFilters\(\)/);
  });

  it("resolves the component after load and reuses the request", async () => {
    const first = loadIssueFilters();
    const second = loadIssueFilters();
    expect(second).toBe(first);
    await expect(first).resolves.toEqual({ default: filters.default });
    expect(resolvedIssueFilters()).toBe(filters.default);
  });
});

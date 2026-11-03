// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The list layout loads through a chunk the route awaits before rendering
// (NEWFRONT-619). These tests pin the contract the routes rely on: one
// shared request, a resolved component after it settles, and a loud failure
// when a route renders before awaiting it.
import { describe, expect, it, vi } from "vitest";

const layout = vi.hoisted(() => ({
  ListLayout: (props: { marker: string }) => <div data-testid="layout">{props.marker}</div>,
}));

vi.mock("./ListLayout.js", () => ({ ListLayout: layout.ListLayout }));

import { loadListLayout, resolvedListLayout } from "./lazy.js";

describe("list layout chunk", () => {
  it("rejects rendering before the chunk has loaded", () => {
    expect(() => resolvedListLayout()).toThrow(/await loadListLayout\(\)/);
  });

  it("resolves the component after load and reuses the request", async () => {
    const first = loadListLayout();
    const second = loadListLayout();
    expect(second).toBe(first);
    await expect(first).resolves.toEqual({ default: layout.ListLayout });
    expect(resolvedListLayout()).toBe(layout.ListLayout);
  });
});

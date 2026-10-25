// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The issues screen loads through a chunk the route awaits before rendering
// (NEWFRONT-906). These tests pin the contract the route relies on: one shared
// request, a resolved component after it settles, and a loud failure when the
// route renders before awaiting it.
import { describe, expect, it, vi } from "vitest";

const screen = vi.hoisted(() => ({
  IssuesListContent: (props: { marker: string }) => <div data-testid="screen">{props.marker}</div>,
}));

vi.mock("./IssuesListContent.js", () => ({ IssuesListContent: screen.IssuesListContent }));

import { loadIssuesListContent, resolvedIssuesListContent } from "./lazy.js";

describe("issues screen chunk", () => {
  it("rejects rendering before the chunk has loaded", () => {
    expect(() => resolvedIssuesListContent()).toThrow(/await loadIssuesListContent\(\)/);
  });

  it("resolves the component after load and reuses the request", async () => {
    const first = loadIssuesListContent();
    const second = loadIssuesListContent();
    expect(second).toBe(first);
    await expect(first).resolves.toMatchObject({ IssuesListContent: screen.IssuesListContent });
    expect(resolvedIssuesListContent()).toBe(screen.IssuesListContent);
  });
});

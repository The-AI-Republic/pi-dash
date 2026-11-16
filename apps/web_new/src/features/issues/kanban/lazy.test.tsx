// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The board layout loads through a chunk the issues route awaits before
// rendering (NEWFRONT-622). These tests pin the contract the route relies
// on: one shared request, the resolved component after it settles, and a
// loud failure when a route renders before awaiting it.
import { describe, expect, it, vi } from "vitest";

const layout = vi.hoisted(() => ({
  KanbanBoard: (props: { marker: string }) => <div data-testid="board">{props.marker}</div>,
}));

vi.mock("./KanbanBoard.js", () => ({ KanbanBoard: layout.KanbanBoard }));

import { loadKanbanLayout, resolvedKanbanLayout } from "./lazy.js";

describe("kanban layout chunk", () => {
  it("rejects rendering before the chunk has loaded", () => {
    expect(() => resolvedKanbanLayout()).toThrow(/await loadKanbanLayout\(\)/);
  });

  it("resolves the component after load and reuses the request", async () => {
    const first = loadKanbanLayout();
    const second = loadKanbanLayout();
    expect(second).toBe(first);
    await expect(first).resolves.toEqual({ default: layout.KanbanBoard });
    expect(resolvedKanbanLayout()).toBe(layout.KanbanBoard);
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board drag gesture (parity/specs/issues.md §5, ISS-037–040, ISS-043):
// target tracking under the pointer, release and cancel, and the paced
// auto-scroll of the board plus the hovered column.
import { act, fireEvent, renderHook } from "@testing-library/react";
import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BOARD_AUTOSCROLL_INTERVAL_MS } from "./autoScroll.js";
import type { BoardDropPoint } from "./dropTarget.js";
import { useBoardDrag, type DraggedCard } from "./useBoardDrag.js";

const COLUMN = "323e4567-e89b-12d3-a456-426614174002";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

function row(): IssueListItem {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: COLUMN,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 42,
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
  };
}

function draggedCard(): DraggedCard {
  return { issue: row(), groupKey: COLUMN, laneKey: null, sourceIndex: 0 };
}

function fire(type: "pointermove" | "pointerup" | "pointercancel", x = 50, y = 120): void {
  fireEvent(document, new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y }));
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  document.body.innerHTML = "";
});

function mountCard(top = 100, bottom = 180): HTMLElement {
  const card = document.createElement("article");
  card.setAttribute("data-testid", "kanban-card");
  card.setAttribute("data-issue-id", ISSUE);
  card.setAttribute("data-group-key", COLUMN);
  card.setAttribute("data-lane-key", "");
  card.getBoundingClientRect = () =>
    ({ left: 0, top, right: 300, bottom, width: 300, height: bottom - top }) as DOMRect;
  document.body.appendChild(card);
  return card;
}

describe("useBoardDrag", () => {
  it("tracks the target under the pointer", () => {
    mountCard();
    const boardRef = React.createRef<HTMLElement>();
    const onDrop = vi.fn();
    const { result } = renderHook(() => useBoardDrag(boardRef, onDrop));
    expect(result.current.dragged).toBeNull();
    act(() => result.current.startDrag(draggedCard()));
    expect(result.current.dragged?.issue.id).toBe(ISSUE);
    act(() => fire("pointermove", 50, 120));
    expect(result.current.target).toEqual({
      kind: "card",
      columnKey: COLUMN,
      laneKey: null,
      issueId: ISSUE,
      edge: "above",
    });
  });

  it("releases to the commit and clears the gesture", () => {
    mountCard();
    const boardRef = React.createRef<HTMLElement>();
    const drops: Array<{ dragged: DraggedCard; target: BoardDropPoint | null }> = [];
    const { result } = renderHook(() =>
      useBoardDrag(boardRef, (dragged, target) => {
        drops.push({ dragged, target });
      })
    );
    act(() => result.current.startDrag(draggedCard()));
    act(() => fire("pointermove", 50, 160));
    act(() => fire("pointerup", 50, 160));
    expect(drops).toHaveLength(1);
    expect(drops[0]?.dragged.issue.id).toBe(ISSUE);
    expect(drops[0]?.target?.edge).toBe("below");
    expect(result.current.dragged).toBeNull();
    expect(result.current.target).toBeNull();
  });

  it("falls back to the last target when the release resolves nowhere", () => {
    mountCard();
    const boardRef = React.createRef<HTMLElement>();
    const drops: Array<BoardDropPoint | null> = [];
    const { result } = renderHook(() => useBoardDrag(boardRef, (_dragged, target) => drops.push(target)));
    act(() => result.current.startDrag(draggedCard()));
    act(() => fire("pointermove", 50, 120));
    expect(result.current.target?.kind).toBe("card");
    act(() => fire("pointerup", 900, 900));
    expect(drops[0]?.kind).toBe("card");
  });

  it("cancels without committing", () => {
    const boardRef = React.createRef<HTMLElement>();
    const onDrop = vi.fn();
    const { result } = renderHook(() => useBoardDrag(boardRef, onDrop));
    act(() => result.current.startDrag(draggedCard()));
    act(() => fire("pointercancel"));
    expect(onDrop).not.toHaveBeenCalled();
    expect(result.current.dragged).toBeNull();
  });

  it("scrolls the board toward a held pointer on the loop", () => {
    mountCard();
    const board = document.createElement("div");
    board.getBoundingClientRect = () =>
      ({ left: 0, top: 0, right: 800, bottom: 600, width: 800, height: 600 }) as DOMRect;
    const scrollBy = vi.fn();
    board.scrollBy = scrollBy;
    const boardRef = { current: board } as React.RefObject<HTMLElement | null>;
    const { result } = renderHook(() => useBoardDrag(boardRef, vi.fn()));
    act(() => result.current.startDrag(draggedCard()));
    act(() => fire("pointermove", 795, 300));
    scrollBy.mockClear();
    act(() => vi.advanceTimersByTime(BOARD_AUTOSCROLL_INTERVAL_MS));
    expect(scrollBy).toHaveBeenCalledTimes(1);
    const step = scrollBy.mock.calls[0]?.[0] as { left: number; top: number };
    expect(step.left).toBeGreaterThan(0);
  });

  it("scrolls the hovered column body vertically", () => {
    mountCard(520, 600);
    const board = document.createElement("div");
    board.innerHTML = `<section data-column-key="${COLUMN}"><div data-testid="kanban-column-body"></div></section>`;
    board.getBoundingClientRect = () =>
      ({ left: 0, top: 0, right: 800, bottom: 600, width: 800, height: 600 }) as DOMRect;
    board.scrollBy = vi.fn();
    const body = board.querySelector('[data-testid="kanban-column-body"]') as HTMLElement;
    body.getBoundingClientRect = () =>
      ({ left: 0, top: 0, right: 350, bottom: 600, width: 350, height: 600 }) as DOMRect;
    const bodyScrollBy = vi.fn();
    body.scrollBy = bodyScrollBy;
    const boardRef = { current: board } as React.RefObject<HTMLElement | null>;
    const { result } = renderHook(() => useBoardDrag(boardRef, vi.fn()));
    act(() => result.current.startDrag(draggedCard()));
    act(() => fire("pointermove", 50, 590));
    expect(bodyScrollBy).toHaveBeenCalled();
    const step = bodyScrollBy.mock.calls[0]?.[0] as { left: number; top: number };
    expect(step.top).toBeGreaterThan(0);
  });

  it("stops scrolling once the drag settles", () => {
    mountCard();
    const board = document.createElement("div");
    board.getBoundingClientRect = () =>
      ({ left: 0, top: 0, right: 800, bottom: 600, width: 800, height: 600 }) as DOMRect;
    const scrollBy = vi.fn();
    board.scrollBy = scrollBy;
    const boardRef = { current: board } as React.RefObject<HTMLElement | null>;
    const { result, unmount } = renderHook(() => useBoardDrag(boardRef, vi.fn()));
    act(() => result.current.startDrag(draggedCard()));
    act(() => fire("pointermove", 795, 300));
    act(() => fire("pointerup", 795, 300));
    scrollBy.mockClear();
    act(() => vi.advanceTimersByTime(BOARD_AUTOSCROLL_INTERVAL_MS * 3));
    expect(scrollBy).not.toHaveBeenCalled();
    unmount();
  });
});

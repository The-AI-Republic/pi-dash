// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Virtualized board card shell (parity/specs/issues.md §5, ISS-043):
// content while vertically near the board at any horizontal distance
// (ISS-039), a marked placeholder past the vertical overscan, content
// while forced or without an observer, and the board itself as the
// observer's root — an implicit root is clipped to the board's
// scrollport, which is what stranded far columns (NEWFRONT-982).
import { act, render, renderHook, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  KanbanVirtualCard,
  useMeasuredHeight,
  useNearViewport,
  virtualCardRootMargin,
  VIRTUAL_CARD_HEIGHT_PX,
  VIRTUAL_CROSS_OVERSCAN_PX,
  VIRTUAL_OVERSCAN_PX,
} from "./KanbanVirtualCard.js";

const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const COLUMN = "323e4567-e89b-12d3-a456-426614174002";

type ObserverCallback = (entries: Array<{ isIntersecting: boolean }>) => void;

const seen = vi.hoisted(() => ({ callbacks: [] as ObserverCallback[], options: [] as object[] }));

function installObserver(): void {
  seen.callbacks = [];
  seen.options = [];
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      callback: ObserverCallback;
      constructor(callback: ObserverCallback, options?: object) {
        this.callback = callback;
        seen.callbacks.push(callback);
        seen.options.push(options ?? {});
      }
      observe(): void {}
      disconnect(): void {}
    }
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
  if (originalOffsetHeight !== undefined) {
    Object.defineProperty(HTMLElement.prototype, "offsetHeight", originalOffsetHeight);
  }
});

const originalOffsetHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight");

/** jsdom never lays out, so pin the content height the shell measures. */
function stubCardHeight(px: number): void {
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get: () => px,
  });
}

function shell(forceContent = false, root: HTMLElement | null = null): React.ReactElement {
  return (
    <KanbanVirtualCard issueId={ISSUE} groupKey={COLUMN} laneKey={null} forceContent={forceContent} root={root}>
      <article data-testid="kanban-card">Fix login</article>
    </KanbanVirtualCard>
  );
}

/** A board scroller element to root the card window at. */
function boardElement(): HTMLElement {
  const board = document.createElement("div");
  board.dataset.testid = "board";
  document.body.append(board);
  return board;
}

describe("KanbanVirtualCard", () => {
  it("renders content without an observer", () => {
    render(shell());
    expect(screen.getByText("Fix login")).toBeVisible();
    expect(VIRTUAL_OVERSCAN_PX).toBe(800);
    expect(screen.getByText("Fix login").parentElement).toHaveClass("shrink-0");
  });

  it("mounts content once the shell nears the board", () => {
    installObserver();
    render(shell(false, boardElement()));
    expect(screen.queryByText("Fix login")).toBeNull();
    const placeholder = screen.getByTestId("kanban-card");
    expect(placeholder).toHaveAttribute("data-issue-id", ISSUE);
    expect(placeholder).toHaveAttribute("data-group-key", COLUMN);
    expect(placeholder).toHaveAttribute("data-virtualized", "true");
    expect(placeholder).toHaveStyle({ height: `${VIRTUAL_CARD_HEIGHT_PX}px` });
    expect(placeholder).toHaveClass("shrink-0");
    act(() => seen.callbacks[0]?.([{ isIntersecting: true }]));
    expect(screen.getByText("Fix login")).toBeVisible();
  });

  it("unmounts content past the overscan", () => {
    installObserver();
    render(shell(false, boardElement()));
    act(() => seen.callbacks[0]?.([{ isIntersecting: true }]));
    expect(screen.getByText("Fix login")).toBeVisible();
    act(() => seen.callbacks[0]?.([{ isIntersecting: false }]));
    expect(screen.queryByText("Fix login")).toBeNull();
    expect(screen.getByTestId("kanban-card")).toHaveAttribute("data-virtualized", "true");
  });

  it("keeps content while forced even past the overscan", () => {
    installObserver();
    render(shell(true, boardElement()));
    act(() => seen.callbacks[0]?.([{ isIntersecting: false }]));
    expect(screen.getByText("Fix login")).toBeVisible();
  });

  it("reads near when the ref never attaches", () => {
    installObserver();
    const { result } = renderHook(() => useNearViewport<HTMLDivElement>());
    expect(result.current.near).toBe(true);
  });

  it("observes with vertical overscan and unbounded horizontal reach", () => {
    // ISS-039: a lone card in a far column (horizontal distance only)
    // must mount its content, while ISS-043 keeps windowing tall
    // columns vertically.
    expect(virtualCardRootMargin()).toBe(
      `${VIRTUAL_OVERSCAN_PX}px ${VIRTUAL_CROSS_OVERSCAN_PX}px ${VIRTUAL_OVERSCAN_PX}px ${VIRTUAL_CROSS_OVERSCAN_PX}px`
    );
    expect(VIRTUAL_CROSS_OVERSCAN_PX).toBe(1_000_000);
    installObserver();
    render(shell(false, boardElement()));
    expect(seen.options[0]).toMatchObject({ rootMargin: virtualCardRootMargin() });
  });

  it("measures against the board scroller rather than the implicit viewport", () => {
    // NEWFRONT-982: the browser clips an implicit-root intersection to
    // the nearest scrolling ancestor — the board — so the horizontal
    // margin never reaches a column the board scrolled out sideways.
    // Rooting at the board is what makes the reach real.
    installObserver();
    const board = boardElement();
    render(shell(false, board));
    expect(seen.options[0]).toMatchObject({
      root: board,
      rootMargin: virtualCardRootMargin(),
    });
  });

  it("keeps windowing vertically while rooted at the board", () => {
    installObserver();
    const board = boardElement();
    render(shell(false, board));
    expect(screen.queryByText("Fix login")).toBeNull();
    act(() => seen.callbacks[0]?.([{ isIntersecting: true }]));
    expect(screen.getByText("Fix login")).toBeVisible();
    act(() => seen.callbacks[0]?.([{ isIntersecting: false }]));
    expect(screen.queryByText("Fix login")).toBeNull();
    expect(screen.getByTestId("kanban-card")).toHaveAttribute("data-virtualized", "true");
  });

  it("falls back to the implicit root until the board scroller attaches", () => {
    // The board node arrives one commit after the cards on first mount.
    // Until then the window observes without a root, exactly as it did
    // before the board handed its scroller over.
    installObserver();
    render(shell(false, null));
    expect(seen.options[0]).toMatchObject({ root: null });
  });

  it("re-observes against the board scroller once it attaches", () => {
    installObserver();
    const { rerender } = render(<>{shell(false, null)}</>);
    expect(seen.options).toHaveLength(1);
    const board = boardElement();
    rerender(<>{shell(false, board)}</>);
    expect(seen.options).toHaveLength(2);
    expect(seen.options[1]).toMatchObject({ root: board });
  });

  it("re-virtualizes at the measured content height instead of the estimate", () => {
    // NEWFRONT-983: the placeholder used to snap back to the 120px
    // estimate, shifting the column and re-tripping the window edge —
    // the churn that flickered mid-window cards after the end scroll.
    // Reusing the measured height removes the shift, so the window
    // settles.
    stubCardHeight(148);
    installObserver();
    render(shell(false, boardElement()));
    expect(screen.getByTestId("kanban-card")).toHaveStyle({
      height: `${VIRTUAL_CARD_HEIGHT_PX}px`,
    });
    act(() => seen.callbacks[0]?.([{ isIntersecting: true }]));
    expect(screen.getByText("Fix login")).toBeVisible();
    act(() => seen.callbacks[0]?.([{ isIntersecting: false }]));
    expect(screen.queryByText("Fix login")).toBeNull();
    expect(screen.getByTestId("kanban-card")).toHaveStyle({ height: "148px" });
  });

  it("tracks content as it settles and keeps the latest height", () => {
    stubCardHeight(148);
    installObserver();
    const board = boardElement();
    const { rerender } = render(shell(false, board));
    act(() => seen.callbacks[0]?.([{ isIntersecting: true }]));
    expect(screen.getByText("Fix login")).toBeVisible();
    stubCardHeight(200);
    rerender(shell(false, board));
    act(() => seen.callbacks[0]?.([{ isIntersecting: false }]));
    expect(screen.getByTestId("kanban-card")).toHaveStyle({ height: "200px" });
  });

  it("leaves the estimate in place until content measures", () => {
    // jsdom reports offsetHeight 0: a zero read must never replace
    // the estimate, so unmeasured shells keep their 120px height.
    installObserver();
    render(shell(false, boardElement()));
    act(() => seen.callbacks[0]?.([{ isIntersecting: true }]));
    act(() => seen.callbacks[0]?.([{ isIntersecting: false }]));
    expect(screen.getByTestId("kanban-card")).toHaveStyle({
      height: `${VIRTUAL_CARD_HEIGHT_PX}px`,
    });
  });

  it("measures nothing while idle or detached", () => {
    const detached = { current: null };
    expect(renderHook(() => useMeasuredHeight(detached, true)).result.current).toBeNull();
    const idle = { current: document.createElement("div") };
    expect(renderHook(() => useMeasuredHeight(idle, false)).result.current).toBeNull();
  });
});

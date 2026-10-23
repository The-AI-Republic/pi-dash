// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { firstPageGroups } from "./model.js";
import { SPACE_BOARD_SCROLL_DEBOUNCE_MS, SpaceBoardLayout } from "./SpaceBoardLayout.js";
import { boardIssue, DONE_ID, EMPTY_ID, firstPage, STATES, TODO_ID } from "./testFixtures.js";

interface ObserverRecord {
  callback: IntersectionObserverCallback;
  root: Element | null;
}

const observers: ObserverRecord[] = [];

class TestIntersectionObserver {
  constructor(
    private readonly callback: IntersectionObserverCallback,
    private readonly options?: IntersectionObserverInit
  ) {}
  observe(): void {
    const root = this.options?.root;
    observers.push({ callback: this.callback, root: root instanceof Element ? root : null });
  }
  disconnect(): void {}
}

function renderBoard(
  page: ReturnType<typeof firstPage>,
  overrides: { paging?: string[]; failed?: string[]; onLoadMore?: (stateId: string) => void } = {}
) {
  const onLoadMore = overrides.onLoadMore ?? vi.fn();
  render(
    <SpaceBoardLayout
      groups={firstPageGroups(STATES, page)}
      paging={new Set(overrides.paging ?? [])}
      failed={new Set(overrides.failed ?? [])}
      onLoadMore={onLoadMore}
      projectIdentifier="SPC"
    />
  );
  return onLoadMore;
}

function column(stateId: string): HTMLElement {
  const found = screen
    .getAllByTestId("space-board-column")
    .find((node) => node.getAttribute("data-state-id") === stateId);
  if (found === undefined) throw new Error(`missing column ${stateId}`);
  return found;
}

beforeEach(() => {
  observers.length = 0;
  vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
  vi.useFakeTimers({ shouldAdvanceTime: true });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("SpaceBoardLayout (SPC-024)", () => {
  it("renders every state as a sideways column in board order, empty ones included", () => {
    renderBoard(firstPage({ [TODO_ID]: { results: [boardIssue(1)], total_results: 1 } }));
    const strip = screen.getByTestId("space-board-layout");
    expect(strip.className).toContain("flex");
    expect(strip.className).toContain("overflow-x-auto");
    const columns = screen.getAllByTestId("space-board-column");
    expect(columns.map((node) => node.getAttribute("data-state-id"))).toEqual([TODO_ID, EMPTY_ID, DONE_ID]);
    const empty = column(EMPTY_ID);
    expect(within(empty).getByTestId("space-group-count")).toHaveTextContent("0");
    expect(within(empty).queryAllByTestId("space-board-card")).toHaveLength(0);
  });

  it("gives each column a fixed width and keeps its own vertical scroller under a sticky header", () => {
    renderBoard(firstPage({ [TODO_ID]: { results: [boardIssue(1)], total_results: 1 } }));
    const todo = column(TODO_ID);
    expect(todo.className).toContain("w-[300px]");
    expect(todo.className).toContain("shrink-0");
    const header = within(todo).getByTestId("space-board-column-header");
    expect(header.className).toContain("sticky");
    const scroller = within(todo).getByTestId("space-board-column-scroll");
    expect(scroller.className).toContain("overflow-y-auto");
  });

  it("shows each column's marker, name, server count and cards", () => {
    renderBoard(firstPage({ [TODO_ID]: { results: [boardIssue(7, TODO_ID, "Card title")], total_results: 55 } }));
    const todo = column(TODO_ID);
    expect(within(todo).getByTestId("space-group-name")).toHaveTextContent("Todo");
    expect(within(todo).getByTestId("space-group-count")).toHaveTextContent("55");
    expect(within(todo).getByTestId("space-state-marker")).toHaveAttribute("title", "Todo");
    expect(within(todo).getByText("Card title")).toBeInTheDocument();
    expect(within(todo).getByText("SPC-7")).toBeInTheDocument();
  });

  it("offers no drag targets or manual load control on columns", () => {
    renderBoard(
      firstPage(
        { [TODO_ID]: { results: [boardIssue(1)], total_results: 60 } },
        {
          next_cursor: "50:1:0",
          next_page_results: true,
        }
      )
    );
    expect(screen.queryByTestId("space-list-load-more")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /load more/i })).not.toBeInTheDocument();
    for (const card of screen.getAllByTestId("space-board-card")) {
      expect(card).not.toHaveAttribute("draggable");
    }
  });
});

describe("SpaceBoardLayout paging (SPC-026)", () => {
  it("asks one column for its next page only when its tripwire becomes visible", () => {
    const onLoadMore = renderBoard(
      firstPage(
        { [TODO_ID]: { results: [boardIssue(1)], total_results: 60 } },
        {
          next_cursor: "50:1:0",
          next_page_results: true,
        }
      )
    );
    // Only the paged column observes, and it watches its own scroller: empty
    // and drained columns install no observer at all.
    const root = within(column(TODO_ID)).getByTestId("space-board-column-scroll");
    expect(observers.length).toBeGreaterThan(0);
    expect(observers.every((record) => record.root === root)).toBe(true);
    act(() => {
      for (const record of observers) {
        record.callback([{ isIntersecting: true } as IntersectionObserverEntry], {} as IntersectionObserver);
      }
    });
    act(() => {
      vi.advanceTimersByTime(SPACE_BOARD_SCROLL_DEBOUNCE_MS);
    });
    expect(onLoadMore).toHaveBeenCalledWith(TODO_ID);
  });

  it("folds a scroll burst into one request", () => {
    const onLoadMore = renderBoard(
      firstPage(
        { [TODO_ID]: { results: [boardIssue(1)], total_results: 60 } },
        {
          next_cursor: "50:1:0",
          next_page_results: true,
        }
      )
    );
    const scroller = within(column(TODO_ID)).getByTestId("space-board-column-scroll");
    for (let index = 0; index < 5; index += 1) {
      fireEvent.scroll(scroller);
      act(() => {
        vi.advanceTimersByTime(60);
      });
    }
    expect(onLoadMore).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(SPACE_BOARD_SCROLL_DEBOUNCE_MS);
    });
    expect(onLoadMore).toHaveBeenCalledTimes(1);
    expect(onLoadMore).toHaveBeenCalledWith(TODO_ID);
  });

  it("ignores scroll on a column with nothing left to load", () => {
    const onLoadMore = renderBoard(firstPage({ [TODO_ID]: { results: [boardIssue(1)], total_results: 1 } }));
    const scroller = within(column(TODO_ID)).getByTestId("space-board-column-scroll");
    fireEvent.scroll(scroller);
    act(() => {
      vi.advanceTimersByTime(SPACE_BOARD_SCROLL_DEBOUNCE_MS);
    });
    expect(onLoadMore).not.toHaveBeenCalled();
  });

  it("ignores scroll while its page is already in flight", () => {
    const onLoadMore = renderBoard(
      firstPage(
        { [TODO_ID]: { results: [boardIssue(1)], total_results: 60 } },
        {
          next_cursor: "50:1:0",
          next_page_results: true,
        }
      ),
      { paging: [TODO_ID] }
    );
    expect(observers).toHaveLength(0);
    fireEvent.scroll(within(column(TODO_ID)).getByTestId("space-board-column-scroll"));
    act(() => {
      vi.advanceTimersByTime(SPACE_BOARD_SCROLL_DEBOUNCE_MS);
    });
    expect(onLoadMore).not.toHaveBeenCalled();
  });

  it("shows placeholder cards in the column while its page loads", () => {
    renderBoard(
      firstPage(
        { [TODO_ID]: { results: [boardIssue(1)], total_results: 60 } },
        {
          next_cursor: "50:1:0",
          next_page_results: true,
        }
      ),
      { paging: [TODO_ID] }
    );
    const todo = column(TODO_ID);
    expect(within(todo).getByTestId("space-board-card-placeholder")).toBeInTheDocument();
    expect(within(column(DONE_ID)).queryByTestId("space-board-card-placeholder")).not.toBeInTheDocument();
  });

  it("drops a pending request when the column unmounts", () => {
    const onLoadMore = vi.fn();
    const view = render(
      <SpaceBoardLayout
        groups={firstPageGroups(
          STATES,
          firstPage(
            { [TODO_ID]: { results: [boardIssue(1)], total_results: 60 } },
            {
              next_cursor: "50:1:0",
              next_page_results: true,
            }
          )
        )}
        paging={new Set()}
        failed={new Set()}
        onLoadMore={onLoadMore}
        projectIdentifier="SPC"
      />
    );
    fireEvent.scroll(within(column(TODO_ID)).getByTestId("space-board-column-scroll"));
    view.unmount();
    act(() => {
      vi.advanceTimersByTime(SPACE_BOARD_SCROLL_DEBOUNCE_MS);
    });
    expect(onLoadMore).not.toHaveBeenCalled();
  });

  it("keeps the trigger stable across the re-render a request causes", () => {
    const onLoadMore = vi.fn();
    const page = firstPage(
      { [TODO_ID]: { results: [boardIssue(1)], total_results: 60 } },
      {
        next_cursor: "50:1:0",
        next_page_results: true,
      }
    );
    const view = render(
      <SpaceBoardLayout
        groups={firstPageGroups(STATES, page)}
        paging={new Set()}
        failed={new Set()}
        onLoadMore={onLoadMore}
        projectIdentifier="SPC"
      />
    );
    fireEvent.scroll(within(column(TODO_ID)).getByTestId("space-board-column-scroll"));
    view.rerender(
      <SpaceBoardLayout
        groups={firstPageGroups(STATES, page)}
        paging={new Set([TODO_ID])}
        failed={new Set()}
        onLoadMore={onLoadMore}
        projectIdentifier="SPC"
      />
    );
    act(() => {
      vi.advanceTimersByTime(SPACE_BOARD_SCROLL_DEBOUNCE_MS);
    });
    expect(onLoadMore).toHaveBeenCalledTimes(1);
  });

  it("marks a column whose page failed so a later scroll can retry", () => {
    renderBoard(
      firstPage(
        { [TODO_ID]: { results: [boardIssue(1)], total_results: 60 } },
        {
          next_cursor: "50:1:0",
          next_page_results: true,
        }
      ),
      { failed: [TODO_ID] }
    );
    expect(column(TODO_ID)).toHaveAttribute("data-failed", "true");
    expect(column(DONE_ID)).not.toHaveAttribute("data-failed");
  });
});

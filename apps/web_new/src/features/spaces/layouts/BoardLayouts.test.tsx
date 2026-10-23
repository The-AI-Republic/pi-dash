// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { SpaceLayout } from "../lib/address.js";
import { BoardLayouts, EmptyBoardNotice, SPACE_BOARD_EMPTY_TEXT } from "./BoardLayouts.js";
import { boardIssue, DONE_ID, firstPage, STATES, TODO_ID } from "./testFixtures.js";

const client = createClient({
  baseUrl: "https://api.test",
  transport: (() => Promise.reject(new Error("unexpected network"))) as Transport,
});

function renderLayouts(
  layout: SpaceLayout,
  page = firstPage({ [TODO_ID]: { results: [boardIssue(1)], total_results: 1 } }),
  onGoHome?: () => void
) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <BoardLayouts
        client={client}
        anchor="anchor-1"
        layout={layout}
        filters={{}}
        states={STATES}
        page={page}
        projectIdentifier="SPC"
        onGoHome={onGoHome}
      />
    </QueryClientProvider>
  );
}

describe("BoardLayouts (SPC-025)", () => {
  it("renders the list surface when the settled layout is list", () => {
    renderLayouts("list");
    expect(screen.getByTestId("space-list-layout")).toBeInTheDocument();
    expect(screen.queryByTestId("space-board-layout")).not.toBeInTheDocument();
  });

  it("renders the board surface when the settled layout is kanban", () => {
    renderLayouts("kanban");
    expect(screen.getByTestId("space-board-layout")).toBeInTheDocument();
    expect(screen.queryByTestId("space-list-layout")).not.toBeInTheDocument();
  });

  it("catches a zero-issue group's missing bucket instead of showing an error", () => {
    renderLayouts("list", firstPage({ [DONE_ID]: { results: [boardIssue(2, DONE_ID)], total_results: 1 } }));
    expect(screen.getByText("Issue 2")).toBeInTheDocument();
    expect(screen.getAllByTestId("space-group-count").map((node) => node.textContent)).toEqual(["0", "0", "1"]);
  });

  for (const layout of ["list", "kanban"] as const) {
    it(`replaces the whole ${layout} surface with one empty notice on a zero-issue board`, () => {
      renderLayouts(layout, firstPage({}));
      expect(screen.getByTestId("space-board-empty")).toBeInTheDocument();
      expect(screen.getByRole("status")).toHaveTextContent(SPACE_BOARD_EMPTY_TEXT);
      expect(screen.queryByTestId("space-list-layout")).not.toBeInTheDocument();
      expect(screen.queryByTestId("space-board-layout")).not.toBeInTheDocument();
      expect(screen.queryByTestId("space-list-group")).not.toBeInTheDocument();
      expect(screen.queryByTestId("space-board-column")).not.toBeInTheDocument();
      expect(screen.queryByTestId("space-list-placeholder")).not.toBeInTheDocument();
      expect(screen.queryByTestId("space-board-card-placeholder")).not.toBeInTheDocument();
    });
  }

  it("decides on the fetch's total, not on the rendered rows", () => {
    // A filter emptied the board: the total is zero even though buckets exist.
    renderLayouts("kanban", firstPage({ [TODO_ID]: { results: [], total_results: 0 } }, { total_count: 0 }));
    expect(screen.getByTestId("space-board-empty")).toBeInTheDocument();
    // A group with no rows among others is not the empty board.
    renderLayouts("kanban");
    expect(screen.getAllByTestId("space-board-empty")).toHaveLength(1);
  });

  it("uses the board's own empty line", () => {
    expect(SPACE_BOARD_EMPTY_TEXT).toBe("No work items found");
  });
});

describe("EmptyBoardNotice", () => {
  it("offers a way back only when the caller provides one", async () => {
    const onGoHome = vi.fn();
    const { unmount } = render(<EmptyBoardNotice onGoHome={onGoHome} />);
    await userEvent.click(screen.getByRole("button", { name: "Back to all boards" }));
    expect(onGoHome).toHaveBeenCalledTimes(1);
    unmount();
    render(<EmptyBoardNotice />);
    expect(screen.queryByRole("button", { name: "Back to all boards" })).not.toBeInTheDocument();
  });
});

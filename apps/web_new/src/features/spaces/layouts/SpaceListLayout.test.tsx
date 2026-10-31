// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceBoardIssuePage } from "@pidash/api-client";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { firstPageGroups, type SpaceIssueGroup } from "./model.js";
import { SpaceListLayout } from "./SpaceListLayout.js";
import { boardIssue, DONE_ID, EMPTY_ID, firstPage, issueHref, LABELS, STATES, TODO_ID } from "./testFixtures.js";

function groupsOf(page: SpaceBoardIssuePage, states = STATES): SpaceIssueGroup[] {
  return firstPageGroups(states, page);
}

function renderList(
  groups: SpaceIssueGroup[],
  overrides: { paging?: string[]; failed?: string[]; onLoadMore?: (stateId: string) => void } = {}
) {
  const onLoadMore = overrides.onLoadMore ?? vi.fn();
  render(
    <SpaceListLayout
      groups={groups}
      paging={new Set(overrides.paging ?? [])}
      failed={new Set(overrides.failed ?? [])}
      onLoadMore={onLoadMore}
      projectIdentifier="SPC"
      labels={LABELS}
      issueHref={issueHref}
    />
  );
  return onLoadMore;
}

describe("SpaceListLayout (SPC-023)", () => {
  it("renders one section per state in board order, each with its name and server count", () => {
    renderList(
      groupsOf(
        firstPage({
          [TODO_ID]: { results: [boardIssue(1), boardIssue(2)], total_results: 2 },
          [DONE_ID]: { results: [boardIssue(3, DONE_ID)], total_results: 1 },
        })
      )
    );
    const sections = screen.getAllByTestId("space-list-group");
    expect(sections.map((section) => section.getAttribute("data-state-id"))).toEqual([TODO_ID, EMPTY_ID, DONE_ID]);
    const names = screen.getAllByTestId("space-group-name").map((node) => node.textContent);
    expect(names).toEqual(["Todo", "Empty", "Done"]);
    const counts = screen.getAllByTestId("space-group-count").map((node) => node.textContent);
    expect(counts).toEqual(["2", "0", "1"]);
  });

  it("renders empty states as a bare header with a zero count", () => {
    renderList(groupsOf(firstPage({ [TODO_ID]: { results: [boardIssue(1)], total_results: 1 } })));
    const empty = screen
      .getAllByTestId("space-list-group")
      .find((node) => node.getAttribute("data-state-id") === EMPTY_ID);
    expect(empty).toBeDefined();
    if (empty === undefined) throw new Error("missing empty group");
    expect(within(empty).getByTestId("space-group-count")).toHaveTextContent("0");
    expect(within(empty).queryAllByTestId("space-board-issue")).toHaveLength(0);
  });

  it("shows a colour marker and the public key plus title for every row", () => {
    renderList(groupsOf(firstPage({ [TODO_ID]: { results: [boardIssue(7, TODO_ID, "Read me")], total_results: 1 } })));
    const marker = screen.getAllByTestId("space-state-marker")[0];
    expect(marker).toHaveAttribute("title", "Todo");
    expect(marker).toHaveAttribute("style", expect.stringContaining("rgb(170, 0, 0)"));
    expect(screen.getByText("SPC-7")).toBeInTheDocument();
    expect(screen.getByText("Read me")).toBeInTheDocument();
  });

  it("collapses and expands a group without touching its rows", async () => {
    renderList(
      groupsOf(
        firstPage(
          { [TODO_ID]: { results: [boardIssue(1, TODO_ID, "Kept row")], total_results: 60 } },
          {
            next_cursor: "50:1:0",
            next_page_results: true,
          }
        )
      )
    );
    const header = screen.getAllByTestId("space-list-group-header")[0];
    expect(header).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("Kept row")).toBeInTheDocument();
    await userEvent.click(header!);
    expect(header).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("Kept row")).not.toBeInTheDocument();
    // Collapsing keeps the paging control state: expanding shows it again.
    await userEvent.click(header!);
    expect(header).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("Kept row")).toBeInTheDocument();
    expect(screen.getByTestId("space-list-load-more")).toBeInTheDocument();
  });

  it("offers a manual load control only while a group has another page", () => {
    renderList(groupsOf(firstPage({ [TODO_ID]: { results: [boardIssue(1)], total_results: 1 } })));
    expect(screen.queryByTestId("space-list-load-more")).not.toBeInTheDocument();
  });

  it("asks for the group the visitor pressed, and only that group", async () => {
    const page = firstPage(
      {
        [TODO_ID]: { results: [boardIssue(1)], total_results: 60 },
        [DONE_ID]: { results: [boardIssue(2, DONE_ID)], total_results: 60 },
      },
      { next_cursor: "50:1:0", next_page_results: true }
    );
    const onLoadMore = renderList(groupsOf(page));
    const controls = screen.getAllByTestId("space-list-load-more");
    expect(controls).toHaveLength(2);
    await userEvent.click(controls[1]!);
    expect(onLoadMore).toHaveBeenCalledTimes(1);
    expect(onLoadMore).toHaveBeenCalledWith(DONE_ID);
  });

  it("replaces the control with placeholder rows while the page is in flight", () => {
    const page = firstPage(
      { [TODO_ID]: { results: [boardIssue(1)], total_results: 60 } },
      {
        next_cursor: "50:1:0",
        next_page_results: true,
      }
    );
    renderList(groupsOf(page), { paging: [TODO_ID] });
    const group = screen
      .getAllByTestId("space-list-group")
      .find((node) => node.getAttribute("data-state-id") === TODO_ID);
    if (group === undefined) throw new Error("missing Todo group");
    expect(within(group).getByTestId("space-list-placeholder")).toBeInTheDocument();
    expect(within(group).queryByTestId("space-list-load-more")).not.toBeInTheDocument();
  });

  it("labels the control as a retry after a failed page", async () => {
    const page = firstPage(
      { [TODO_ID]: { results: [boardIssue(1)], total_results: 60 } },
      {
        next_cursor: "50:1:0",
        next_page_results: true,
      }
    );
    const onLoadMore = renderList(groupsOf(page), { failed: [TODO_ID] });
    const control = screen.getByTestId("space-list-load-more");
    expect(control).toHaveTextContent("Load more — retry");
    await userEvent.click(control);
    expect(onLoadMore).toHaveBeenCalledWith(TODO_ID);
  });
});

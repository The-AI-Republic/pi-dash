// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs list (NEWFRONT-545, RUN-016). Columns, rows, empty/loading states,
// pager end-states and page turns.
import type { AgentRun, AgentRunPage } from "@pidash/api-client";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { RUNS_EMPTY_TEXT, RunsList } from "./RunsList.js";

const run = (overrides: Partial<AgentRun> = {}): AgentRun =>
  ({
    id: "run-1",
    status: "running",
    prompt: "do the thing",
    started_at: "2024-01-02T03:04:05Z",
    created_at: "2024-01-01T00:00:00Z",
    ...overrides,
  }) as AgentRun;

const page = (overrides: Partial<AgentRunPage> = {}): AgentRunPage => ({
  results: [run()],
  count: 1,
  total_count: 31,
  total_pages: 2,
  page: 2,
  per_page: 30,
  ...overrides,
});

function renderList(data: AgentRunPage | undefined, pageNo = 2, loading = false) {
  const onPage = vi.fn();
  const onSelect = vi.fn();
  render(<RunsList data={data} loading={loading} page={pageNo} onPage={onPage} onSelect={onSelect} />);
  return { onPage, onSelect };
}

describe("RunsList", () => {
  it("renders the three columns and one row per run", () => {
    renderList(page({ results: [run(), run({ id: "run-2", prompt: "another" })] }));
    const table = screen.getByTestId("runs-table");
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((cell) => cell.textContent)
    ).toEqual(["Started", "Status", "Prompt"]);
    expect(screen.getAllByTestId("runs-row")).toHaveLength(2);
    expect(screen.getByText("do the thing")).toBeInTheDocument();
    expect(screen.getAllByText("running")).toHaveLength(2);
    expect(within(screen.getAllByTestId("runs-row-status")[0]!).getByText("running")).toBeInTheDocument();
  });

  it("selects a row by click and by keyboard", async () => {
    const { onSelect } = renderList(page());
    const row = screen.getByTestId("runs-row");
    await userEvent.click(row);
    expect(onSelect).toHaveBeenCalledWith("run-1");
    row.focus();
    await userEvent.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledTimes(2);
    await userEvent.keyboard(" ");
    expect(onSelect).toHaveBeenCalledTimes(3);
  });

  it("shows the empty state with no pager and no rows", () => {
    renderList(page({ results: [], count: 0, total_count: 0, total_pages: 0 }));
    expect(screen.getByText(RUNS_EMPTY_TEXT)).toBeInTheDocument();
    expect(screen.getByTestId("runs-empty")).toHaveTextContent(RUNS_EMPTY_TEXT);
    expect(screen.queryByTestId("runs-pager")).toBeNull();
    expect(screen.queryAllByTestId("runs-row")).toHaveLength(0);
  });

  it("shows a spinner instead of the empty text while the first page loads", () => {
    renderList(undefined, 1, true);
    expect(screen.queryByText(RUNS_EMPTY_TEXT)).toBeNull();
    expect(screen.getByRole("status")).toBeInTheDocument();
    expect(screen.queryByTestId("runs-empty")).toBeNull();
  });

  it("labels the page and disables the ends", () => {
    renderList(page());
    expect(screen.getByTestId("runs-page-label")).toHaveTextContent("Page 2 of 2");
    expect(screen.getByRole("button", { name: "Previous" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
  });

  it("turns to the previous page", async () => {
    const { onPage } = renderList(page());
    await userEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(onPage).toHaveBeenCalledWith(1);
  });

  it("turns to the next page when there is one", async () => {
    const { onPage } = renderList(page({ total_pages: 3, page: 2 }));
    await userEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(onPage).toHaveBeenCalledWith(3);
  });

  it("keeps the pager hidden while there are no runs at all", () => {
    renderList(page({ results: [], total_count: 0, total_pages: 1, page: 1 }));
    expect(screen.queryByTestId("runs-pager")).toBeNull();
  });
});

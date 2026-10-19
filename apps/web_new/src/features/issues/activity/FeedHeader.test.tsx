// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Activity header (ISS-195/196): the four category toggles, their checked
// state, the narrowed marker and the sort button that flips direction.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ACTIVITY_HEADING, FILTER_LABEL, FeedHeader, SORT_LABEL } from "./FeedHeader.js";
import { DEFAULT_FILTERS, type FeedFilter } from "./model.js";

function renderHeader(filters: readonly FeedFilter[] = DEFAULT_FILTERS, order: "asc" | "desc" = "asc") {
  const onToggleFilter = vi.fn();
  const onToggleSort = vi.fn();
  render(<FeedHeader filters={filters} onToggleFilter={onToggleFilter} order={order} onToggleSort={onToggleSort} />);
  return { onToggleFilter, onToggleSort };
}

describe("FeedHeader", () => {
  it("shows the Activity heading", () => {
    renderHeader();
    expect(screen.getByRole("heading", { name: ACTIVITY_HEADING })).toBeInTheDocument();
  });

  it("offers the four categories in menu order, all checked by default", async () => {
    const user = userEvent.setup();
    renderHeader();
    await user.click(screen.getByRole("button", { name: FILTER_LABEL }));
    const group = await screen.findByRole("group", { name: "Activity categories" });
    const boxes = within(group).getAllByRole("checkbox");
    expect(boxes.map((box) => box.textContent)).toEqual(["Updates", "Comments", "State", "Assignee"]);
    expect(boxes.map((box) => box.getAttribute("aria-checked"))).toEqual(["true", "true", "true", "true"]);
  });

  it("reflects a narrowed selection and asks to toggle the clicked category", async () => {
    const user = userEvent.setup();
    const { onToggleFilter } = renderHeader(["COMMENT", "STATE"]);
    await user.click(screen.getByRole("button", { name: FILTER_LABEL }));
    const group = await screen.findByRole("group", { name: "Activity categories" });
    const boxes = within(group).getAllByRole("checkbox");
    expect(boxes.map((box) => box.getAttribute("aria-checked"))).toEqual(["false", "true", "true", "false"]);
    await user.click(within(group).getByRole("checkbox", { name: "State" }));
    expect(onToggleFilter).toHaveBeenCalledWith("STATE");
  });

  it("marks the trigger while any category is off", async () => {
    const user = userEvent.setup();
    const { unmount } = render(
      <FeedHeader filters={DEFAULT_FILTERS} onToggleFilter={vi.fn()} order="asc" onToggleSort={vi.fn()} />
    );
    const filter = screen.getByRole("button", { name: FILTER_LABEL });
    expect(filter.querySelector(".sr-only")).toBeNull();
    expect(filter.querySelector("span.absolute")).toBeNull();
    unmount();
    renderHeader(["ACTIVITY"]);
    const narrowed = screen.getByRole("button", { name: new RegExp(FILTER_LABEL) });
    expect(narrowed.querySelector(".sr-only")?.textContent).toBe("Filters narrowed");
    expect(narrowed.querySelector("span.absolute")).not.toBeNull();
    await user.click(narrowed);
    expect(await screen.findByRole("group", { name: "Activity categories" })).toBeInTheDocument();
  });

  it("reports the collapse state on the filter trigger", async () => {
    const user = userEvent.setup();
    renderHeader();
    const trigger = screen.getByRole("button", { name: FILTER_LABEL });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    await user.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
  });

  it("names the sort direction and flips it on click", async () => {
    const user = userEvent.setup();
    const ascending = renderHeader(DEFAULT_FILTERS, "asc");
    const up = screen.getByRole("button", { name: `${SORT_LABEL}: oldest first` });
    expect(up).toHaveAttribute("aria-pressed", "false");
    await user.click(up);
    expect(ascending.onToggleSort).toHaveBeenCalledTimes(1);
  });

  it("flips the one sort glyph between the orders", () => {
    const { rerender } = render(
      <FeedHeader filters={DEFAULT_FILTERS} onToggleFilter={vi.fn()} order="asc" onToggleSort={vi.fn()} />
    );
    const ascending = screen.getByRole("button", { name: `${SORT_LABEL}: oldest first` }).querySelector("svg");
    expect(ascending?.getAttribute("class") ?? "").toContain("rotate-180");
    rerender(<FeedHeader filters={DEFAULT_FILTERS} onToggleFilter={vi.fn()} order="desc" onToggleSort={vi.fn()} />);
    const descending = screen.getByRole("button", { name: `${SORT_LABEL}: newest first` }).querySelector("svg");
    expect(descending?.getAttribute("class") ?? "").not.toContain("rotate-180");
  });

  it("marks the descending sort as pressed", () => {
    renderHeader(DEFAULT_FILTERS, "desc");
    const down = screen.getByRole("button", { name: `${SORT_LABEL}: newest first` });
    expect(down).toHaveAttribute("aria-pressed", "true");
  });
});

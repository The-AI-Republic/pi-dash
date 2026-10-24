// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// PAGE-005 applied-filter chips and clear-all.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PagesFilterChips } from "./PagesFilterChips.js";

const FILTERS = { favorites: true, created: "1_week_ago" as const, creator: "u1" };

describe("PagesFilterChips", () => {
  it("renders nothing without filters", () => {
    render(
      <PagesFilterChips
        filters={{ favorites: false, created: null, creator: null }}
        creatorName={null}
        onFiltersChange={() => undefined}
      />
    );
    expect(screen.queryByTestId("pages-filter-chips")).not.toBeInTheDocument();
  });

  it("shows each active dimension and clears all", () => {
    const onFiltersChange = vi.fn();
    render(<PagesFilterChips filters={FILTERS} creatorName="Ada" onFiltersChange={onFiltersChange} />);
    expect(screen.getByTestId("pages-filter-chip-favorites")).toHaveTextContent("Favorites");
    expect(screen.getByTestId("pages-filter-chip-created")).toHaveTextContent("Created At");
    expect(screen.getByTestId("pages-filter-chip-created")).toHaveTextContent("1 week ago");
    expect(screen.getByTestId("pages-filter-chip-creator")).toHaveTextContent("Created By");
    expect(screen.getByTestId("pages-filter-chip-creator")).toHaveTextContent("Ada");
    fireEvent.click(screen.getByRole("button", { name: "Clear all" }));
    expect(onFiltersChange).toHaveBeenCalledWith({ favorites: false, created: null, creator: null });
  });

  it("removes only the chosen dimension", () => {
    const onFiltersChange = vi.fn();
    render(<PagesFilterChips filters={FILTERS} creatorName="Ada" onFiltersChange={onFiltersChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Remove Favorites" }));
    expect(onFiltersChange).toHaveBeenLastCalledWith({ ...FILTERS, favorites: false });
    fireEvent.click(screen.getByRole("button", { name: "Remove Created At" }));
    expect(onFiltersChange).toHaveBeenLastCalledWith({ ...FILTERS, created: null });
    fireEvent.click(screen.getByRole("button", { name: "Remove Created By" }));
    expect(onFiltersChange).toHaveBeenLastCalledWith({ ...FILTERS, creator: null });
  });
});

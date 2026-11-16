// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// PAGE-003/005 no-results actions clear only the dimension that narrowed rows.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PagesNoResults } from "./PagesNoResults.js";

describe("PagesNoResults", () => {
  it("offers clear search when only search is active", () => {
    const onClearSearch = vi.fn();
    const onClearFilters = vi.fn();
    render(<PagesNoResults search filters={false} onClearSearch={onClearSearch} onClearFilters={onClearFilters} />);
    expect(screen.getByText("No matching results.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect(onClearSearch).toHaveBeenCalledTimes(1);
    expect(onClearFilters).not.toHaveBeenCalled();
  });

  it("offers clear filters when only filters are active", () => {
    const onClearSearch = vi.fn();
    const onClearFilters = vi.fn();
    render(<PagesNoResults search={false} filters onClearSearch={onClearSearch} onClearFilters={onClearFilters} />);
    fireEvent.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(onClearFilters).toHaveBeenCalledTimes(1);
    expect(onClearSearch).not.toHaveBeenCalled();
  });

  it("clears both dimensions together", () => {
    const onClearSearch = vi.fn();
    const onClearFilters = vi.fn();
    render(<PagesNoResults search filters onClearSearch={onClearSearch} onClearFilters={onClearFilters} />);
    fireEvent.click(screen.getByRole("button", { name: "Clear all filters" }));
    expect(onClearSearch).toHaveBeenCalledTimes(1);
    expect(onClearFilters).toHaveBeenCalledTimes(1);
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// PAGE-004 sort menu labels, key picks and direction picks.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DEFAULT_PAGE_SORT } from "../lib/pageList.js";
import { PagesSortMenu } from "./PagesSortMenu.js";

describe("PagesSortMenu", () => {
  it("shows the active key and default descending glyph", () => {
    render(<PagesSortMenu sort={DEFAULT_PAGE_SORT} onSortChange={() => undefined} />);
    expect(screen.getByRole("button", { name: "Date modified" })).toBeInTheDocument();
  });

  it("picks a different key and leaves the direction alone", () => {
    const onSortChange = vi.fn();
    render(<PagesSortMenu sort={DEFAULT_PAGE_SORT} onSortChange={onSortChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Date modified" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Name" }));
    expect(onSortChange).toHaveBeenCalledWith({ key: "name", direction: "descending" });
  });

  it("picks a different direction", () => {
    const onSortChange = vi.fn();
    render(<PagesSortMenu sort={DEFAULT_PAGE_SORT} onSortChange={onSortChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Date modified" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Ascending" }));
    expect(onSortChange).toHaveBeenCalledWith({ key: "modified", direction: "ascending" });
  });

  it("re-picking the current key or direction is inert", () => {
    const onSortChange = vi.fn();
    render(<PagesSortMenu sort={DEFAULT_PAGE_SORT} onSortChange={onSortChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Date modified" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Date modified" }));
    fireEvent.click(screen.getByRole("button", { name: "Date modified" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Descending" }));
    expect(onSortChange).not.toHaveBeenCalled();
  });
});

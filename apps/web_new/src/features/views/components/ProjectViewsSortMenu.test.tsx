// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { ViewSort } from "../lib/viewListControls.js";
import { ProjectViewsSortMenu } from "./ProjectViewsSortMenu.js";

function open(sort: ViewSort, onSortChange: (next: ViewSort) => void = vi.fn()) {
  render(<ProjectViewsSortMenu sort={sort} onSortChange={onSortChange} />);
  fireEvent.click(screen.getByTestId("project-views-sort-trigger"));
  return onSortChange;
}

function marked(label: string): boolean {
  return screen.getByRole("menuitem", { name: label }).querySelector("svg") !== null;
}

describe("ProjectViewsSortMenu", () => {
  it("names the active key and marks it with the active direction", () => {
    open({ key: "updated_at", direction: "descending" });
    expect(screen.getByTestId("project-views-sort-trigger")).toHaveTextContent("Updated at");
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Name",
      "Created at",
      "Updated at",
      "Ascending",
      "Descending",
    ]);
    expect(screen.getByRole("separator")).toBeInTheDocument();
    expect(marked("Updated at")).toBe(true);
    expect(marked("Descending")).toBe(true);
    expect(marked("Name")).toBe(false);
    expect(marked("Ascending")).toBe(false);
  });

  it("switches the key but keeps the direction", () => {
    const onSortChange = vi.fn();
    open({ key: "updated_at", direction: "ascending" }, onSortChange);
    fireEvent.click(screen.getByRole("menuitem", { name: "Created at" }));
    expect(onSortChange).toHaveBeenCalledWith({ key: "created_at", direction: "ascending" });
  });

  it("flips the direction but keeps the key", () => {
    const onSortChange = vi.fn();
    open({ key: "name", direction: "descending" }, onSortChange);
    fireEvent.click(screen.getByRole("menuitem", { name: "Ascending" }));
    expect(onSortChange).toHaveBeenCalledWith({ key: "name", direction: "ascending" });
  });

  it("ignores a pick of the active key or the active direction", () => {
    const onSortChange = vi.fn();
    open({ key: "name", direction: "ascending" }, onSortChange);
    fireEvent.click(screen.getByRole("menuitem", { name: "Name" }));
    expect(onSortChange).not.toHaveBeenCalled();
  });

  it("ignores a pick of the active direction", () => {
    const onSortChange = vi.fn();
    open({ key: "name", direction: "ascending" }, onSortChange);
    fireEvent.click(screen.getByRole("menuitem", { name: "Ascending" }));
    expect(onSortChange).not.toHaveBeenCalled();
  });
});

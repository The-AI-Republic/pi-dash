// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ListSort } from "../lib/list-display.js";
import { ProjectsSortMenu } from "./ProjectsSortMenu.js";

function setup(sort: ListSort, onSortChange: (sort: ListSort) => void = vi.fn()) {
  render(<ProjectsSortMenu sort={sort} onSortChange={onSortChange} />);
  fireEvent.click(screen.getByTestId("projects-sort-trigger"));
  return onSortChange;
}

function checked(label: string): boolean {
  return screen.getByRole("menuitem", { name: label }).querySelector("svg") !== null;
}

describe("ProjectsSortMenu", () => {
  it("names the current key and marks it with the direction", () => {
    setup({ key: "name", direction: "ascending" });

    expect(screen.getByTestId("projects-sort-trigger")).toHaveTextContent("Name");
    expect(checked("Name")).toBe(true);
    expect(checked("Manual")).toBe(false);
    expect(checked("Ascending")).toBe(true);
    expect(checked("Descending")).toBe(false);
  });

  it("keeps the direction when the key changes", () => {
    const onSortChange = vi.fn();
    setup({ key: "name", direction: "descending" }, onSortChange);

    fireEvent.click(screen.getByRole("menuitem", { name: "Created date" }));

    expect(onSortChange).toHaveBeenCalledWith({ key: "created", direction: "descending" });
  });

  it("ignores the current key", () => {
    const onSortChange = vi.fn();
    setup({ key: "name", direction: "ascending" }, onSortChange);

    fireEvent.click(screen.getByRole("menuitem", { name: "Name" }));

    expect(onSortChange).not.toHaveBeenCalled();
  });

  it("locks the direction switch on manual order", () => {
    const onSortChange = vi.fn();
    setup({ key: "name", direction: "descending" }, onSortChange);

    fireEvent.click(screen.getByRole("menuitem", { name: "Manual" }));

    expect(onSortChange).toHaveBeenCalledWith({ key: "manual", direction: "ascending" });
  });

  it("disables both directions while manual order applies", () => {
    const onSortChange = vi.fn();
    setup({ key: "manual", direction: "ascending" }, onSortChange);

    expect(screen.getByRole("menuitem", { name: "Ascending" })).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("menuitem", { name: "Descending" })).toHaveAttribute("aria-disabled", "true");
    expect(checked("Ascending")).toBe(false);
    fireEvent.click(screen.getByRole("menuitem", { name: "Descending" }));
    expect(onSortChange).not.toHaveBeenCalled();
  });

  it("flips the direction and ignores the current one", () => {
    const onSortChange = vi.fn();
    setup({ key: "name", direction: "ascending" }, onSortChange);

    fireEvent.click(screen.getByRole("menuitem", { name: "Ascending" }));
    expect(onSortChange).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("projects-sort-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Descending" }));
    expect(onSortChange).toHaveBeenCalledWith({ key: "name", direction: "descending" });
  });
});

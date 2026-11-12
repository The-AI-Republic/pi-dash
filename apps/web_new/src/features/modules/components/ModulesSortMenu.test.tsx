// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules ordering menu (MOD-005): the trigger names the active key, the
// six keys render in pinned order, the directions follow a separator except
// under Manual, re-picking the current entry changes nothing, and picking a
// key keeps the stored direction.

import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DEFAULT_MODULE_SORT, type ModuleSort } from "../lib/moduleListControls.js";
import { ModulesSortMenu } from "./ModulesSortMenu.js";

function renderMenu(sort: ModuleSort = DEFAULT_MODULE_SORT, onSortChange = vi.fn()) {
  render(<ModulesSortMenu sort={sort} onSortChange={onSortChange} />);
  return onSortChange;
}

function openMenu(): HTMLElement {
  fireEvent.click(screen.getByTestId("modules-sort-trigger"));
  return screen.getByRole("menu");
}

function items(menu: HTMLElement): string[] {
  return within(menu)
    .getAllByRole("menuitem")
    .map((node) => (node.textContent ?? "").trim());
}

describe("ModulesSortMenu", () => {
  it("names the active key on the trigger", () => {
    renderMenu();
    expect(screen.getByTestId("modules-sort-label")).toHaveTextContent("Name");
    expect(screen.getByTestId("modules-sort-trigger")).toHaveAccessibleName("Order by Name");
  });

  it("lists the six keys in order and then the direction", () => {
    renderMenu();
    const menu = openMenu();
    expect(items(menu)).toEqual([
      "Name",
      "Progress",
      "Number of work items",
      "Due date",
      "Created date",
      "Manual",
      "Ascending",
      "Descending",
    ]);
    expect(within(menu).getAllByRole("separator")).toHaveLength(1);
  });

  it("shows both directions with the active one marked", () => {
    renderMenu({ key: "progress", direction: "descending" });
    const menu = openMenu();
    expect(items(menu).slice(6)).toEqual(["Ascending", "Descending"]);
    const descending = within(menu).getByRole("menuitem", { name: "Descending" });
    expect(descending.querySelector("svg")).not.toBeNull();
  });

  it("picks a key and adds the direction entry to the same menu", () => {
    const onSortChange = renderMenu();
    const menu = openMenu();
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Due date" }));
    expect(onSortChange).toHaveBeenCalledWith({ key: "due-date", direction: "ascending" });
  });

  it("keeps the stored direction when the key changes", () => {
    const onSortChange = renderMenu({ key: "name", direction: "descending" });
    fireEvent.click(within(openMenu()).getByRole("menuitem", { name: "Progress" }));
    expect(onSortChange).toHaveBeenCalledWith({ key: "progress", direction: "descending" });
  });

  it("picks a direction for the current key", () => {
    const onSortChange = renderMenu({ key: "due-date", direction: "ascending" });
    fireEvent.click(within(openMenu()).getByRole("menuitem", { name: "Descending" }));
    expect(onSortChange).toHaveBeenCalledWith({ key: "due-date", direction: "descending" });
  });

  it("changes nothing when the current entry is re-picked", () => {
    const onSortChange = renderMenu();
    fireEvent.click(within(openMenu()).getByRole("menuitem", { name: "Name" }));
    expect(onSortChange).not.toHaveBeenCalled();
    fireEvent.click(within(openMenu()).getByRole("menuitem", { name: "Ascending" }));
    expect(onSortChange).not.toHaveBeenCalled();
  });

  it("offers the keys alone under Manual placement", () => {
    renderMenu({ key: "manual", direction: "ascending" });
    const menu = openMenu();
    expect(items(menu)).toEqual(["Name", "Progress", "Number of work items", "Due date", "Created date", "Manual"]);
    expect(within(menu).queryByRole("separator")).not.toBeInTheDocument();
  });

  it("drops the direction when Manual is picked", () => {
    const onSortChange = renderMenu({ key: "name", direction: "descending" });
    fireEvent.click(within(openMenu()).getByRole("menuitem", { name: "Manual" }));
    expect(onSortChange).toHaveBeenCalledWith({ key: "manual", direction: "ascending" });
  });
});

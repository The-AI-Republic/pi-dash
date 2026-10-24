// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { SpaceBoardState } from "@pidash/api-client";

import type { SpaceBoardFilters, SpaceLayout } from "../lib/address.js";
import { SpaceFilterPanel } from "./SpaceFilterPanel.js";

function stateWith(id: string, name: string): SpaceBoardState {
  return { id, name, color: "#888888", group: "unstarted", sequence: 1 };
}

const TODO = "11111111-1111-4111-8111-111111111111";
const DOING = "22222222-2222-4222-8222-222222222222";
const STATES = [stateWith(TODO, "Todo"), stateWith(DOING, "Doing")];

function renderPanel(
  overrides: { layout?: SpaceLayout; filters?: SpaceBoardFilters; states?: readonly SpaceBoardState[] } = {}
) {
  const onToggle = vi.fn();
  render(
    <SpaceFilterPanel
      layout={overrides.layout ?? "list"}
      filters={overrides.filters ?? {}}
      states={overrides.states ?? STATES}
      onToggle={onToggle}
    />
  );
  return { onToggle, user: userEvent.setup() };
}

function section(facet: string): HTMLElement {
  const found = screen.getAllByTestId("space-filter-facet").find((entry) => entry.getAttribute("data-facet") === facet);
  if (!found) throw new Error(`missing ${facet} section`);
  return found;
}

function optionIn(facet: string, value: string): HTMLElement {
  const found = within(section(facet))
    .getAllByTestId("space-filter-option")
    .find((row) => row.getAttribute("data-value") === value);
  if (!found) throw new Error(`missing ${facet} option ${value}`);
  return found;
}

function optionValues(facet: string): string[] {
  return within(section(facet))
    .queryAllByTestId("space-filter-option")
    .map((row) => row.getAttribute("data-value") ?? "");
}

async function openPanel(user: ReturnType<typeof userEvent.setup>): Promise<void> {
  await user.click(screen.getByTestId("space-filters-button"));
  await screen.findByTestId("space-filters-panel");
}

describe("SpaceFilterPanel (SPC-034)", () => {
  it("shows only the Filters control while the panel is closed", () => {
    renderPanel();
    expect(screen.getByTestId("space-filters-button")).toHaveTextContent("Filters");
    expect(screen.getByTestId("space-filters-button")).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByTestId("space-filters-panel")).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });

  it("opens priority and state sections with no label facet", async () => {
    const { user } = renderPanel();
    await openPanel(user);
    const facets = screen.getAllByTestId("space-filter-facet");
    expect(facets.map((entry) => entry.getAttribute("data-facet"))).toEqual(["priority", "state"]);
    expect(within(section("priority")).getByTestId("space-filter-section")).toHaveTextContent("Priority");
    expect(within(section("state")).getByTestId("space-filter-section")).toHaveTextContent("State");
    expect(screen.getAllByTestId("space-filter-facet")).toHaveLength(2);
  });

  it("lists the five priorities highest first, all unchecked", async () => {
    const { user } = renderPanel();
    await openPanel(user);
    const rows = within(section("priority")).getAllByTestId("space-filter-option");
    expect(rows.map((row) => row.textContent)).toEqual(["Urgent", "High", "Medium", "Low", "None"]);
    expect(rows.map((row) => row.getAttribute("data-value"))).toEqual(["urgent", "high", "medium", "low", "none"]);
    for (const row of rows) {
      expect(row).toHaveAttribute("aria-pressed", "false");
      expect(row).toHaveAttribute("data-checked", "false");
    }
    expect(
      within(section("priority"))
        .getAllByTestId("space-priority-marker")
        .map((marker) => marker.getAttribute("data-level"))
    ).toEqual(["urgent", "high", "medium", "low", "none"]);
  });

  it("lists one option per workflow state with its marker", async () => {
    const { user } = renderPanel();
    await openPanel(user);
    expect(optionValues("state")).toEqual([TODO, DOING]);
    expect(optionIn("state", TODO)).toHaveTextContent("Todo");
    expect(within(section("state")).getAllByTestId("space-state-marker")).toHaveLength(2);
  });

  it("marks applied options checked and counts them on the section header", async () => {
    const { user } = renderPanel({ filters: { priority: ["high"], state: [TODO] } });
    await openPanel(user);
    const high = optionIn("priority", "high");
    expect(high).toHaveAttribute("aria-pressed", "true");
    expect(high).toHaveAttribute("data-checked", "true");
    expect(within(high).getByTestId("space-filter-check")).toHaveAttribute("data-checked", "true");
    expect(within(section("priority")).getByTestId("space-filter-section")).toHaveTextContent("Priority (1)");
    expect(within(section("state")).getByTestId("space-filter-section")).toHaveTextContent("State (1)");
  });

  it("picks stay in the open panel and flow one value at a time", async () => {
    const { user, onToggle } = renderPanel();
    await openPanel(user);
    await user.click(optionIn("priority", "high"));
    expect(onToggle).toHaveBeenCalledWith("priority", "high");
    await user.click(optionIn("state", TODO));
    expect(onToggle).toHaveBeenCalledWith("state", TODO);
    expect(screen.getByTestId("space-filters-panel")).toBeInTheDocument();
  });

  it("collapses and re-expands a section, keeping the other open", async () => {
    const { user } = renderPanel();
    await openPanel(user);
    const header = within(section("priority")).getByTestId("space-filter-section");
    expect(header).toHaveAttribute("aria-expanded", "true");
    await user.click(header);
    expect(header).toHaveAttribute("aria-expanded", "false");
    expect(within(section("priority")).queryAllByTestId("space-filter-option")).toEqual([]);
    expect(within(section("state")).getAllByTestId("space-filter-option")).toHaveLength(2);
    await user.click(header);
    expect(header).toHaveAttribute("aria-expanded", "true");
    expect(within(section("priority")).getAllByTestId("space-filter-option")).toHaveLength(5);
  });

  it("caps long state lists at five with an expand control", async () => {
    const states = Array.from({ length: 7 }, (_, index) =>
      stateWith(`aaaaaaa${index}-1111-4111-8111-111111111111`, `State ${index}`)
    );
    const { user } = renderPanel({ states });
    await openPanel(user);
    expect(optionValues("state")).toHaveLength(5);
    const control = within(section("state")).getByTestId("space-filter-show-more");
    expect(control).toHaveAttribute("data-expanded", "false");
    expect(control).toHaveTextContent("Show more");
    await user.click(control);
    expect(optionValues("state")).toHaveLength(7);
    expect(control).toHaveAttribute("data-expanded", "true");
    expect(control).toHaveTextContent("Show less");
    await user.click(control);
    expect(optionValues("state")).toHaveLength(5);
  });

  it("offers no expand control when the states fit the preview", async () => {
    const { user } = renderPanel();
    await openPanel(user);
    expect(within(section("state")).queryByTestId("space-filter-show-more")).toBeNull();
    expect(within(section("priority")).queryByTestId("space-filter-show-more")).toBeNull();
  });

  it("derives its sections from the active layout", async () => {
    const { user } = renderPanel({ layout: "kanban" });
    await openPanel(user);
    expect(screen.getAllByTestId("space-filter-facet").map((entry) => entry.getAttribute("data-facet"))).toEqual([
      "priority",
      "state",
    ]);
  });
});

describe("panel search (SPC-039)", () => {
  it("narrows every facet at once and shows an empty line where nothing matches", async () => {
    const { user, onToggle } = renderPanel();
    await openPanel(user);
    const search = screen.getByTestId("space-filter-search");
    expect(search).toHaveAttribute("placeholder", "Search");
    await user.type(search, "hig");
    expect(
      within(section("priority"))
        .getAllByTestId("space-filter-option")
        .map((row) => row.textContent)
    ).toEqual(["High"]);
    expect(within(section("state")).queryAllByTestId("space-filter-option")).toEqual([]);
    expect(within(section("state")).getByTestId("space-filter-no-matches")).toHaveTextContent("No matches found");
    expect(within(section("priority")).queryByTestId("space-filter-no-matches")).toBeNull();
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("matches state names and clears back to the full lists", async () => {
    const { user } = renderPanel();
    await openPanel(user);
    const search = screen.getByTestId("space-filter-search");
    await user.type(search, "TODO");
    expect(
      within(section("state"))
        .getAllByTestId("space-filter-option")
        .map((row) => row.textContent)
    ).toEqual(["Todo"]);
    expect(within(section("priority")).queryAllByTestId("space-filter-option")).toEqual([]);
    await user.clear(search);
    expect(within(section("priority")).getAllByTestId("space-filter-option")).toHaveLength(5);
    expect(within(section("state")).getAllByTestId("space-filter-option")).toHaveLength(2);
    expect(screen.queryByTestId("space-filter-no-matches")).toBeNull();
  });

  it("resets the search and the expansion when the panel closes", async () => {
    const states = Array.from({ length: 6 }, (_, index) =>
      stateWith(`bbbbbb${index}b-2222-4222-8222-222222222222`, `Flow ${index}`)
    );
    const { user } = renderPanel({ states });
    await openPanel(user);
    await user.type(screen.getByTestId("space-filter-search"), "Flow 1");
    expect(within(section("state")).getAllByTestId("space-filter-option")).toHaveLength(1);
    await user.click(within(section("priority")).getByTestId("space-filter-section"));
    await user.click(screen.getByTestId("space-filters-button"));
    expect(screen.queryByTestId("space-filters-panel")).not.toBeInTheDocument();
    await openPanel(user);
    expect(screen.getByTestId("space-filter-search")).toHaveValue("");
    expect(within(section("priority")).getAllByTestId("space-filter-option")).toHaveLength(5);
    expect(optionValues("state")).toHaveLength(5);
  });
});

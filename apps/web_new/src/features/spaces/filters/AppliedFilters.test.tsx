// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { SpaceBoardState } from "@pidash/api-client";

import type { SpaceBoardFilters } from "../lib/address.js";
import { AppliedFilters } from "./AppliedFilters.js";

const TODO = "11111111-1111-4111-8111-111111111111";
const DOING = "22222222-2222-4222-8222-222222222222";
const LABEL = "33333333-3333-4333-8333-333333333333";
const UNKNOWN_STATE = "44444444-4444-4444-8444-444444444444";

const STATES: readonly SpaceBoardState[] = [
  { id: TODO, name: "Todo", color: "#888888", group: "unstarted", sequence: 1 },
  { id: DOING, name: "Doing", color: "#444444", group: "started", sequence: 2 },
];

function renderStrip(filters: SpaceBoardFilters, states: readonly SpaceBoardState[] = STATES) {
  const onRemoveValue = vi.fn();
  const onClearFacet = vi.fn();
  const onClearAll = vi.fn();
  render(
    <AppliedFilters
      filters={filters}
      states={states}
      onRemoveValue={onRemoveValue}
      onClearFacet={onClearFacet}
      onClearAll={onClearAll}
    />
  );
  return { onRemoveValue, onClearFacet, onClearAll, user: userEvent.setup() };
}

function shell(facet: string): HTMLElement {
  const found = screen
    .getAllByTestId("space-applied-facet")
    .find((entry) => entry.getAttribute("data-facet") === facet);
  if (!found) throw new Error(`missing ${facet} shell`);
  return found;
}

function removeIn(facet: string, value: string): HTMLElement {
  const found = within(shell(facet))
    .getAllByTestId("space-applied-remove")
    .find((entry) => entry.getAttribute("data-value") === value);
  if (!found) throw new Error(`missing ${facet} remove for ${value}`);
  return found;
}

describe("AppliedFilters (SPC-035/037)", () => {
  it("renders nothing when no facet applies", () => {
    renderStrip({});
    expect(screen.queryByTestId("space-applied-filters")).not.toBeInTheDocument();
    renderStrip({ priority: [], state: [], labels: [] });
    expect(screen.queryAllByTestId("space-applied-filters")).toEqual([]);
  });

  it("renders shells in priority, state, labels order with their labels", () => {
    renderStrip({ priority: ["high"], state: [TODO], labels: [LABEL] });
    expect(
      within(screen.getByTestId("space-applied-filters"))
        .getAllByTestId("space-applied-facet")
        .map((entry) => entry.getAttribute("data-facet"))
    ).toEqual(["priority", "state", "labels"]);
    expect(within(shell("priority")).getByText("Priority")).toBeInTheDocument();
    expect(within(shell("state")).getByText("State")).toBeInTheDocument();
    expect(within(shell("labels")).getByText("Labels")).toBeInTheDocument();
    expect(screen.getByTestId("space-applied-clear-all")).toHaveTextContent("Clear all");
  });

  it("renders one pill per priority value with its marker", () => {
    renderStrip({ priority: ["high", "urgent"] });
    const pills = within(shell("priority")).getAllByTestId("space-applied-pill");
    expect(pills.map((pill) => pill.getAttribute("data-value"))).toEqual(["high", "urgent"]);
    expect(pills.map((pill) => pill.textContent)).toEqual(["High", "Urgent"]);
    expect(
      within(shell("priority"))
        .getAllByTestId("space-priority-marker")
        .map((marker) => marker.getAttribute("data-level"))
    ).toEqual(["high", "urgent"]);
  });

  it("renders state pills by name with the state's marker", () => {
    renderStrip({ state: [TODO, DOING] });
    const pills = within(shell("state")).getAllByTestId("space-applied-pill");
    expect(pills.map((pill) => pill.getAttribute("data-value"))).toEqual(["Todo", "Doing"]);
    expect(pills.map((pill) => pill.getAttribute("data-state-id"))).toEqual([TODO, DOING]);
    expect(pills.map((pill) => pill.textContent)).toEqual(["Todo", "Doing"]);
    expect(within(shell("state")).getAllByTestId("space-state-marker")).toHaveLength(2);
  });

  it("renders the labels shell without pills and state ids it does not know without a pill", () => {
    renderStrip({ labels: [LABEL], state: [UNKNOWN_STATE] });
    expect(within(shell("labels")).queryAllByTestId("space-applied-pill")).toEqual([]);
    expect(within(shell("state")).queryAllByTestId("space-applied-pill")).toEqual([]);
    // Both shells still clear: an invisible filter must stay removable.
    expect(within(shell("labels")).getByTestId("space-applied-clear")).toBeInTheDocument();
    expect(within(shell("state")).getByTestId("space-applied-clear")).toBeInTheDocument();
  });

  it("removes one pill with its facet and raw value", async () => {
    const { user, onRemoveValue } = renderStrip({ priority: ["high", "urgent"], state: [TODO] });
    await user.click(removeIn("priority", "high"));
    expect(onRemoveValue).toHaveBeenCalledWith("priority", "high");
    await user.click(removeIn("state", "Todo"));
    expect(onRemoveValue).toHaveBeenCalledWith("state", TODO);
    expect(onRemoveValue).toHaveBeenCalledTimes(2);
  });

  it("clears one facet at a time", async () => {
    const { user, onClearFacet } = renderStrip({ priority: ["high"], labels: [LABEL] });
    await user.click(within(shell("labels")).getByTestId("space-applied-clear"));
    expect(onClearFacet).toHaveBeenCalledWith("labels");
    await user.click(within(shell("priority")).getByTestId("space-applied-clear"));
    expect(onClearFacet).toHaveBeenCalledWith("priority");
  });

  it("clears everything through the clear-all control", async () => {
    const { user, onClearAll } = renderStrip({ priority: ["high"], state: [TODO], labels: [LABEL] });
    await user.click(screen.getByTestId("space-applied-clear-all"));
    expect(onClearAll).toHaveBeenCalledTimes(1);
  });
});

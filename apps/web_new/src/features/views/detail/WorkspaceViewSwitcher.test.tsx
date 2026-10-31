// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { makeView } from "../testing.js";
import { WorkspaceViewSwitcher, workspaceSwitcherOptions } from "./WorkspaceViewSwitcher.js";

const CUSTOM_A = "223e4567-e89b-12d3-a456-426614174001";
const CUSTOM_B = "323e4567-e89b-12d3-a456-426614174002";

function renderSwitcher(overrides: Partial<React.ComponentProps<typeof WorkspaceViewSwitcher>> = {}) {
  const onPick = vi.fn();
  render(
    <WorkspaceViewSwitcher
      currentKey="all-issues"
      currentName="All work items"
      customs={[]}
      loading={false}
      failed={false}
      onPick={onPick}
      {...overrides}
    />
  );
  return { onPick };
}

describe("workspaceSwitcherOptions", () => {
  it("lists the four defaults first, then the customs in list order", () => {
    const options = workspaceSwitcherOptions([
      makeView({ id: CUSTOM_A, name: "Alpha" }),
      makeView({ id: CUSTOM_B, name: "Beta" }),
    ]);
    expect(options.map((option) => option.label)).toEqual([
      "All work items",
      "Assigned",
      "Created",
      "Subscribed",
      "Alpha",
      "Beta",
    ]);
    expect(options.map((option) => option.id)).toEqual([
      "all-issues",
      "assigned",
      "created",
      "subscribed",
      CUSTOM_A,
      CUSTOM_B,
    ]);
  });
});

describe("WorkspaceViewSwitcher", () => {
  it("lists defaults and customs with search and picks in the same tab", async () => {
    const alpha = makeView({ id: CUSTOM_A, name: "Alpha view" });
    const { onPick } = renderSwitcher({
      currentKey: CUSTOM_A,
      currentName: "Alpha view",
      customs: [alpha, makeView({ id: CUSTOM_B, name: "Beta view" })],
    });

    fireEvent.click(screen.getByRole("button", { name: "Alpha view" }));
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(6));
    expect(screen.getByRole("combobox", { name: "Search views" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Alpha view" })).toHaveAttribute("aria-selected", "true");

    await userEvent.type(screen.getByRole("combobox", { name: "Search views" }), "beta");
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(1));

    fireEvent.click(screen.getByRole("option", { name: "Beta view" }));
    expect(onPick).toHaveBeenCalledWith(CUSTOM_B);
  });

  it("picks a default by its route key", async () => {
    const { onPick } = renderSwitcher({
      currentKey: CUSTOM_A,
      currentName: "Alpha view",
      customs: [makeView({ id: CUSTOM_A, name: "Alpha view" })],
    });
    fireEvent.click(screen.getByRole("button", { name: "Alpha view" }));
    await waitFor(() => expect(screen.getByRole("option", { name: "Assigned" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("option", { name: "Assigned" }));
    expect(onPick).toHaveBeenCalledWith("assigned");
  });

  it("searches case-insensitively and reports an empty result", async () => {
    renderSwitcher({ customs: [makeView({ id: CUSTOM_A, name: "Alpha view" })] });
    fireEvent.click(screen.getByRole("button", { name: "All work items" }));
    await waitFor(() => expect(screen.getByRole("option", { name: "Alpha view" })).toBeInTheDocument());
    await userEvent.type(screen.getByRole("combobox", { name: "Search views" }), "ALPHA");
    expect(screen.getByRole("option", { name: "Alpha view" })).toBeInTheDocument();
    await userEvent.clear(screen.getByRole("combobox", { name: "Search views" }));
    await userEvent.type(screen.getByRole("combobox", { name: "Search views" }), "zzz");
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("No views match that name."));
  });

  it("marks the switcher busy while customs load", async () => {
    renderSwitcher({ customs: [], loading: true });
    fireEvent.click(screen.getByRole("button", { name: "All work items" }));
    expect(await screen.findAllByText("Loading views")).not.toHaveLength(0);
  });

  it("reports a customs failure in the switcher", async () => {
    renderSwitcher({ customs: [], loading: false, failed: true });
    fireEvent.click(screen.getByRole("button", { name: "All work items" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Views could not be loaded."));
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { makeView } from "../testing.js";
import { SavedViewHeader } from "./SavedViewHeader.js";
import { ViewSwitcherMark } from "./SavedViewSwitcher.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

function renderHeader(overrides: Partial<React.ComponentProps<typeof SavedViewHeader>> = {}) {
  const onOpenView = vi.fn();
  const onOpenProject = vi.fn();
  render(
    <SavedViewHeader
      workspaceSlug="acme"
      projectId={PROJECT}
      projectName="Website"
      view={makeView({ name: "Triage" })}
      views={[makeView({ name: "Triage" })]}
      viewsLoading={false}
      viewsFailed={false}
      onOpenProject={onOpenProject}
      onOpenView={onOpenView}
      {...overrides}
    />
  );
  return { onOpenView, onOpenProject };
}

describe("SavedViewHeader", () => {
  it("draws an emoji mark, a stored icon, and the views glyph for a bare record", async () => {
    const { rerender } = render(
      <ViewSwitcherMark view={makeView({ logo_props: { in_use: "emoji", emoji: { value: "🚀" } } })} />
    );
    expect(screen.getByText("🚀")).toBeInTheDocument();
    rerender(
      <ViewSwitcherMark view={makeView({ logo_props: { in_use: "icon", icon: { name: "anchor", color: null } } })} />
    );
    expect(screen.getByText("T")).toBeInTheDocument();
    rerender(<ViewSwitcherMark view={makeView({ logo_props: {} })} />);
    expect(document.querySelector("svg.lucide-layers")).not.toBeNull();
  });

  it("reads project, Views link and current view", () => {
    renderHeader();
    const trail = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(within(trail).getAllByRole("button")[0]).toHaveTextContent("Website");
    expect(within(trail).getByRole("link", { name: "Views" })).toHaveAttribute(
      "href",
      `/acme/projects/${PROJECT}/views`
    );
    expect(within(trail).getByRole("button", { name: "Triage" })).toHaveTextContent("Triage");
  });

  it("navigates to the project from the first crumb", () => {
    const { onOpenProject } = renderHeader();
    fireEvent.click(screen.getByRole("button", { name: "Website" }));
    expect(onOpenProject).toHaveBeenCalledTimes(1);
  });

  it("lists every view with search and navigates on pick", async () => {
    const alpha = makeView({ id: "view-a", name: "Alpha view" });
    const beta = makeView({ id: "view-b", name: "Beta view" });
    const { onOpenView } = renderHeader({ view: alpha, views: [alpha, beta] });

    fireEvent.click(screen.getByRole("button", { name: "Alpha view" }));
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(2));
    expect(screen.getByRole("combobox", { name: "Search views" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Alpha view" })).toHaveAttribute("aria-selected", "true");

    await userEvent.type(screen.getByRole("combobox", { name: "Search views" }), "beta");
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(1));
    expect(screen.getByRole("option", { name: "Beta view" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("option", { name: "Beta view" }));
    expect(onOpenView).toHaveBeenCalledWith("view-b");
  });

  it("searches case-insensitively and reports an empty result", async () => {
    const alpha = makeView({ id: "view-a", name: "Alpha view" });
    renderHeader({ view: alpha, views: [alpha] });
    fireEvent.click(screen.getByRole("button", { name: "Alpha view" }));
    await waitFor(() => expect(screen.getByRole("option", { name: "Alpha view" })).toBeInTheDocument());
    await userEvent.type(screen.getByRole("combobox", { name: "Search views" }), "ALPHA");
    expect(screen.getByRole("option", { name: "Alpha view" })).toBeInTheDocument();
    await userEvent.clear(screen.getByRole("combobox", { name: "Search views" }));
    await userEvent.type(screen.getByRole("combobox", { name: "Search views" }), "zzz");
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("No views match that name."));
  });

  it("marks the switcher busy while views load", async () => {
    const alpha = makeView({ id: "view-a", name: "Alpha view" });
    renderHeader({ view: alpha, views: [], viewsLoading: true });
    fireEvent.click(screen.getByRole("button", { name: "Alpha view" }));
    expect(await screen.findAllByText("Loading views")).not.toHaveLength(0);
  });

  it("reports a view-list failure in the switcher", async () => {
    const alpha = makeView({ id: "view-a", name: "Alpha view" });
    renderHeader({ view: alpha, views: [], viewsLoading: false, viewsFailed: true });
    fireEvent.click(screen.getByRole("button", { name: "Alpha view" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Views could not be loaded."));
  });

  it("shows the lock for an owner-only view and nothing for a shared one", () => {
    renderHeader({ view: makeView({ name: "Triage", access: 0 }) });
    expect(screen.getByTestId("saved-view-lock")).toHaveAttribute("aria-label", "Private");
  });

  it("renders no lock for a shared view", () => {
    renderHeader({ view: makeView({ name: "Triage", access: 1 }) });
    expect(screen.queryByTestId("saved-view-lock")).toBeNull();
  });
});

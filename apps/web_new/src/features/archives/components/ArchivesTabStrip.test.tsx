// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { selectArchivesTabs } from "../tabs.js";
import { ArchivesTabStrip } from "./ArchivesTabStrip.js";

function renderStrip(activeTab: "issues" | "cycles" | "modules", flags = { cycle_view: true, module_view: true }) {
  const tabs = selectArchivesTabs({ workspaceSlug: "acme", projectId: "p1", flags });
  const rootRoute = createRootRoute({ component: () => <ArchivesTabStrip tabs={tabs} activeTab={activeTab} /> });
  const childRoute = createRoute({ getParentRoute: () => rootRoute, path: "/", component: () => null });
  const router = createRouter({
    routeTree: rootRoute.addChildren([childRoute]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
  render(<RouterProvider router={router} />);
}

describe("ArchivesTabStrip", () => {
  it("links every tab to its own address", async () => {
    renderStrip("issues");

    const strip = await screen.findByTestId("archives-tab-strip");
    expect(strip).toHaveTextContent("Work items");
    expect(screen.getByRole("link", { name: "Work items" })).toHaveAttribute(
      "href",
      "/acme/projects/p1/archives/issues"
    );
    expect(screen.getByRole("link", { name: "Cycles" })).toHaveAttribute("href", "/acme/projects/p1/archives/cycles");
    expect(screen.getByRole("link", { name: "Modules" })).toHaveAttribute("href", "/acme/projects/p1/archives/modules");
  });

  it("marks only the active tab current", async () => {
    renderStrip("cycles");

    expect(await screen.findByRole("link", { name: "Cycles" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "Work items" })).not.toHaveAttribute("aria-current");
    expect(screen.getByRole("link", { name: "Modules" })).not.toHaveAttribute("aria-current");
  });

  it("renders only the gated tabs", async () => {
    renderStrip("issues", { cycle_view: false, module_view: false });

    expect(await screen.findByRole("link", { name: "Work items" })).not.toBeNull();
    expect(screen.queryByRole("link", { name: "Cycles" })).toBeNull();
    expect(screen.queryByRole("link", { name: "Modules" })).toBeNull();
  });
});

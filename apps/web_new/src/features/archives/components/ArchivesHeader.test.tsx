// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ARCHIVES_BACK_BREAKPOINT_PX } from "../lib/narrow.js";
import { ArchivesHeader, type ArchivesHeaderProps } from "./ArchivesHeader.js";

function stubViewport(matches: boolean) {
  vi.stubGlobal("matchMedia", (query: string) => {
    expect(query).toBe(`(max-width: ${ARCHIVES_BACK_BREAKPOINT_PX}px)`);
    return { matches, addEventListener: () => undefined, removeEventListener: () => undefined };
  });
}

function renderHeader(overrides: Partial<ArchivesHeaderProps> = {}) {
  const onSelectProject = vi.fn();
  const onBack = vi.fn();
  const props: ArchivesHeaderProps = {
    workspaceSlug: "acme",
    projectId: "p1",
    projectName: "Web",
    joinedProjects: [{ id: "p1", name: "Web" }],
    terminal: "Work items",
    count: 2,
    showBadge: true,
    onSelectProject,
    onBack,
    ...overrides,
  };
  const rootRoute = createRootRoute({ component: () => <ArchivesHeader {...props} /> });
  const childRoute = createRoute({ getParentRoute: () => rootRoute, path: "/", component: () => null });
  const router = createRouter({
    routeTree: rootRoute.addChildren([childRoute]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
  const rendered = render(<RouterProvider router={router} />);
  return { onSelectProject, onBack, unmount: rendered.unmount };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("ArchivesHeader", () => {
  it("trails project, archives and the active tab", async () => {
    stubViewport(false);
    renderHeader();

    const trail = await screen.findByRole("navigation", { name: "Breadcrumb" });
    const crumbs = trail.querySelectorAll("li");
    expect(crumbs).toHaveLength(3);
    expect(crumbs[0]?.textContent).toContain("Web");
    expect(crumbs[1]?.textContent).toBe("Archives");
    expect(crumbs[2]?.textContent).toBe("Work items");
    expect(screen.getByRole("link", { name: "Archives" })).toHaveAttribute("href", "/acme/projects/p1/archives/issues");
    expect(crumbs[2]?.querySelector("[aria-current='page']")).not.toBeNull();
  });

  it("ends the trail at Archives without a terminal", async () => {
    stubViewport(false);
    renderHeader({ terminal: null, showBadge: false });

    const crumbs = (await screen.findByRole("navigation", { name: "Breadcrumb" })).querySelectorAll("li");
    expect(crumbs).toHaveLength(2);
    expect(screen.queryByTestId("archives-count-badge")).toBeNull();
  });

  it("shows the badge on the work-items tab and hides it elsewhere", async () => {
    stubViewport(false);
    const first = renderHeader({ terminal: "Work items", showBadge: true });
    expect(await screen.findByTestId("archives-count-badge")).toHaveTextContent("2");
    first.unmount();

    renderHeader({ terminal: "Cycles", showBadge: false });
    expect(screen.queryByTestId("archives-count-badge")).toBeNull();
  });

  it("hides the back control on desktop viewports", async () => {
    stubViewport(false);
    const { onBack } = renderHeader();

    await screen.findByRole("navigation", { name: "Breadcrumb" });
    expect(screen.queryByTestId("archives-back")).toBeNull();
    expect(onBack).not.toHaveBeenCalled();
  });

  it("backs out through the narrow-viewport control", async () => {
    stubViewport(true);
    const { onBack } = renderHeader();

    fireEvent.click(await screen.findByTestId("archives-back"));
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it("switches projects through the switcher crumb", async () => {
    stubViewport(false);
    const { onSelectProject } = renderHeader({
      joinedProjects: [
        { id: "p1", name: "Web" },
        { id: "p2", name: "Mobile" },
      ],
    });

    fireEvent.click(await screen.findByTestId("project-switcher-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Mobile" }));
    expect(onSelectProject).toHaveBeenCalledWith("p2");
  });
});

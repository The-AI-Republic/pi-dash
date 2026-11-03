// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: (props: { to?: string; children?: React.ReactNode; [key: string]: unknown }) => {
    const { to, children, ...rest } = props;
    return (
      <a href={to} {...rest}>
        {children}
      </a>
    );
  },
}));

import { selectProjectTabs } from "../tabs.js";
import { ProjectTabChrome } from "./ProjectTabChrome.js";

const TABS = selectProjectTabs({
  workspaceSlug: "acme",
  projectId: "p1",
  flags: { cycle_view: true, module_view: true, issue_views_view: true, page_view: true, inbox_view: true },
  role: 20,
});

const PROJECT = { id: "p1", name: "Web" };
const JOINED = [
  { id: "p1", name: "Web" },
  { id: "p2", name: "Mobile" },
];

function setup(overrides: Partial<React.ComponentProps<typeof ProjectTabChrome>> = {}) {
  const onNavigate = vi.fn();
  const onToggleDefault = vi.fn();
  const onHideTab = vi.fn();
  const onShowTab = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ProjectTabChrome
        workspaceSlug="acme"
        project={PROJECT}
        joinedProjects={JOINED}
        tabs={TABS}
        activeKey="work_items"
        activeHref="/acme/projects/p1/issues"
        defaultTab="cycles"
        hiddenTabs={[]}
        isAdmin
        isAuthorized
        onNavigate={onNavigate}
        onToggleDefault={onToggleDefault}
        onHideTab={onHideTab}
        onShowTab={onShowTab}
        {...overrides}
      />
    </QueryClientProvider>
  );
  return { onNavigate, onToggleDefault, onHideTab, onShowTab };
}

describe("ProjectTabChrome", () => {
  it("composes the header and the tab strip in one region", () => {
    setup();
    const strip = screen.getByTestId("project-tab-strip");
    expect(strip).toHaveAttribute("aria-label", "Project tabs");
    expect(screen.getByTestId("project-switcher-trigger")).toHaveTextContent("Web");
    expect(screen.getByTestId("project-actions-trigger")).toBeInTheDocument();
    expect(screen.getAllByRole("link")).toHaveLength(7);
  });

  it("opens the picked project at the current default tab", () => {
    const { onNavigate } = setup();
    fireEvent.click(screen.getByTestId("project-switcher-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Mobile" }));
    expect(onNavigate).toHaveBeenCalledWith("/acme/projects/p2/cycles");
  });

  it("stays put when the current project is picked again", () => {
    const { onNavigate } = setup({ defaultTab: "work_items" });
    fireEvent.click(screen.getByTestId("project-switcher-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Web" }));
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it("forwards tab preference actions", () => {
    const { onHideTab } = setup();
    fireEvent.contextMenu(screen.getByRole("link", { name: "Pages" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Hide in more menu" }));
    expect(onHideTab).toHaveBeenCalledWith("pages");
  });
});

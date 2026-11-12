// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-ins for the router: navigation is TanStack's own tested
// behavior. What this suite owns is the one-state-at-a-time body.
vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
  useNavigate: () => vi.fn(),
}));

import { makeMember, makeView, VIEWS_OWNER_ID } from "../testing.js";
import { ProjectViewsListBody } from "./ProjectViewsListBody.js";

const BASE = {
  members: [makeMember(VIEWS_OWNER_ID)],
  workspaceSlug: "acme",
  projectId: "project-1",
  viewerId: VIEWS_OWNER_ID,
  projectRole: 15,
  hideCycle: false,
  hideModule: false,
  onRetry: () => {},
} as const;

describe("ProjectViewsListBody", () => {
  it("shows only the skeleton while loading", () => {
    render(<ProjectViewsListBody state="loading" views={[]} {...BASE} />);
    expect(screen.getByTestId("project-views-skeleton")).toBeInTheDocument();
    expect(screen.queryByTestId("project-views-list")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-views-zero")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-views-no-match")).not.toBeInTheDocument();
  });

  it("shows only the error state with its retry", () => {
    const onRetry = vi.fn();
    render(<ProjectViewsListBody state="error" views={[]} {...BASE} onRetry={onRetry} />);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("project-views-skeleton")).not.toBeInTheDocument();
  });

  it("shows only the zero-views state with its shortcut", () => {
    const onCreateView = vi.fn();
    render(<ProjectViewsListBody state="zero" views={[]} {...BASE} onCreateView={onCreateView} />);
    expect(screen.getByRole("heading", { name: /Save custom views for your project/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Create view" }));
    expect(onCreateView).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("project-views-skeleton")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-views-no-match")).not.toBeInTheDocument();
  });

  it("shows only the no-match state when filters exclude every view", () => {
    render(<ProjectViewsListBody state="no-match" views={[]} {...BASE} />);
    expect(screen.getByText("No matching results.")).toBeInTheDocument();
    expect(screen.queryByTestId("project-views-list")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-views-zero")).not.toBeInTheDocument();
  });

  it("shows one row per visible view with owner chips", () => {
    const first = makeView({ id: "view-1", name: "Alpha" });
    const second = makeView({ id: "view-2", name: "Beta", owned_by: "623e4567-e89b-12d3-a456-426614174005" });
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ProjectViewsListBody state="rows" views={[first, second]} {...BASE} />
      </QueryClientProvider>
    );
    const rows = screen.getAllByTestId("project-views-row");
    expect(rows).toHaveLength(2);
    expect(screen.getByRole("link", { name: /Alpha/ })).toHaveAttribute(
      "href",
      "/acme/projects/project-1/views/view-1"
    );
    expect(screen.getByRole("link", { name: /Beta/ })).toHaveAttribute("href", "/acme/projects/project-1/views/view-2");
    expect(screen.getAllByTestId("project-views-row-owner")).toHaveLength(1);
    expect(screen.queryByTestId("project-views-skeleton")).not.toBeInTheDocument();
  });

  it("threads the viewer's standing into every row's actions", () => {
    const owned = makeView({ id: "view-1", name: "Alpha", owned_by: VIEWS_OWNER_ID });
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ProjectViewsListBody state="rows" views={[owned]} {...BASE} projectRole={5} />
      </QueryClientProvider>
    );
    // A guest sees the row but no star (VIEW-012); the menu stays.
    expect(screen.getByTestId("project-views-row")).toBeInTheDocument();
    expect(screen.queryByTestId("project-views-row-star")).toBeNull();
    expect(screen.getByRole("button", { name: "Actions for Alpha" })).toBeInTheDocument();
  });
});

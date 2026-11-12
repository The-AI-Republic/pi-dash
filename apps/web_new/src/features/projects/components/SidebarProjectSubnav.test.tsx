// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import type { Project } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SidebarProjectExpandToggle, SidebarProjectSubnav } from "./SidebarProjectSubnav.js";

// The runners entry renders as a router Link (migrated route), so stand
// it in as a plain anchor and assert the navigation intent only.
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  Link: (props: { to?: string; children?: React.ReactNode } & Record<string, unknown>) => {
    const { to, children, ...rest } = props;
    return (
      <a href={to} {...rest}>
        {children}
      </a>
    );
  },
}));

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function project(overrides: Partial<Project> = {}): Project {
  return {
    id: PROJECT_ID,
    name: "Atlas",
    identifier: "ATL",
    sort_order: 10000,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: WS_ID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("SidebarProjectExpandToggle", () => {
  it("names the collapsed state after the project", () => {
    const onToggle = vi.fn();
    render(<SidebarProjectExpandToggle projectName="Atlas" expanded={false} onToggle={onToggle} />);
    const toggle = screen.getByRole("button", { name: "Expand Atlas" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("names the expanded state after the project", () => {
    const onToggle = vi.fn();
    render(<SidebarProjectExpandToggle projectName="Atlas" expanded onToggle={onToggle} />);
    const toggle = screen.getByRole("button", { name: "Collapse Atlas" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });
});

describe("SidebarProjectSubnav", () => {
  it("lists flag-gated entries with slashed addresses", () => {
    const onEntryClick = vi.fn();
    render(
      <SidebarProjectSubnav
        workspaceSlug="acme"
        project={project({ cycle_view: false, module_view: false, issue_views_view: false })}
        currentPath="/acme/"
        onEntryClick={onEntryClick}
      />
    );
    const links = screen.getAllByTestId("project-subnav-link");
    expect(screen.getByTestId("project-subnav")).toHaveAccessibleName("Atlas sections");
    expect(links.map((link) => link.textContent)).toEqual(
      expect.arrayContaining(["Work Items", "Pages", "Schedulers", "AI Workers"])
    );
    expect(links.map((link) => link.textContent)).not.toContain("Cycles");
    for (const link of links) {
      expect(link.getAttribute("href")).toContain(`/projects/${PROJECT_ID}/`);
    }
  });

  it("hides member-plus entries from project guests", () => {
    render(
      <SidebarProjectSubnav
        workspaceSlug="acme"
        project={project({ member_role: 5 })}
        currentPath="/acme/"
        onEntryClick={() => undefined}
      />
    );
    const texts = screen.getAllByTestId("project-subnav-link").map((link) => link.textContent);
    expect(texts).toEqual(expect.arrayContaining(["Work Items", "Pages", "Schedulers"]));
    expect(texts).not.toContain("Cycles");
    expect(texts).not.toContain("Modules");
    expect(texts).not.toContain("AI Workers");
  });

  it("marks the current entry and badges pending intake", () => {
    render(
      <SidebarProjectSubnav
        workspaceSlug="acme"
        project={project({ inbox_view: true, intake_count: 4 })}
        currentPath={`/acme/projects/${PROJECT_ID}/pages/`}
        onEntryClick={() => undefined}
      />
    );
    const entries = screen.getAllByTestId("project-subnav-link");
    const current = entries.find((link) => (link.textContent ?? "").includes("Pages"));
    expect(current).toHaveAttribute("aria-current", "page");
    const intake = entries.find((link) => (link.textContent ?? "").includes("Intake"));
    expect(intake?.textContent).toContain("4");
  });

  it("marks the runners entry current on its route", () => {
    render(
      <SidebarProjectSubnav
        workspaceSlug="acme"
        project={project()}
        currentPath={`/acme/projects/${PROJECT_ID}/runners/`}
        onEntryClick={() => undefined}
      />
    );
    const entries = screen.getAllByTestId("project-subnav-link");
    const runners = entries.find((link) => (link.textContent ?? "").includes("AI Workers"));
    expect(runners).toHaveAttribute("aria-current", "page");
  });

  it("marks no entry without a reported path", () => {
    render(
      <SidebarProjectSubnav
        workspaceSlug="acme"
        project={project()}
        currentPath={null}
        onEntryClick={() => undefined}
      />
    );
    expect(screen.getByTestId("project-subnav").querySelector('[aria-current="page"]')).toBeNull();
  });

  it("reports entry clicks with their addresses", () => {
    const onEntryClick = vi.fn();
    render(
      <SidebarProjectSubnav workspaceSlug="acme" project={project()} currentPath="/acme/" onEntryClick={onEntryClick} />
    );
    const entries = screen.getAllByTestId("project-subnav-link");
    const pages = entries.find((link) => (link.textContent ?? "").includes("Pages"));
    if (pages === undefined) throw new Error("pages entry missing");
    fireEvent.click(pages);
    expect(onEntryClick).toHaveBeenCalledTimes(1);
    expect(onEntryClick.mock.calls[0]?.[1]).toBe(`/acme/projects/${PROJECT_ID}/pages/`);
  });

  it("reports runners clicks without an entry address", () => {
    const onEntryClick = vi.fn();
    const onRunnersClick = vi.fn();
    render(
      <SidebarProjectSubnav
        workspaceSlug="acme"
        project={project()}
        currentPath="/acme/"
        onEntryClick={onEntryClick}
        onRunnersClick={onRunnersClick}
      />
    );
    const entries = screen.getAllByTestId("project-subnav-link");
    const runners = entries.find((link) => (link.textContent ?? "").includes("AI Workers"));
    if (runners === undefined) throw new Error("runners entry missing");
    fireEvent.click(runners);
    expect(onRunnersClick).toHaveBeenCalledTimes(1);
    expect(onEntryClick).not.toHaveBeenCalled();
  });
});

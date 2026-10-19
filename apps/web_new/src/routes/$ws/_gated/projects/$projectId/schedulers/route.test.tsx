// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project schedulers section shell (parity/specs/agents.md §5, AGT-021):
// List and Calendar tabs, exactly one marked current for a tab address and
// neither for an install detail underneath.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { router } = vi.hoisted(() => ({ router: { pathname: "/acme/projects/p1/schedulers/list" } }));

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: ({ to, children, ...rest }: { to: string; children?: React.ReactNode }) => (
      <a href={to} {...rest}>
        {children}
      </a>
    ),
    Outlet: () => <div data-testid="tab-body" />,
    useLocation: () => ({ pathname: router.pathname }),
  };
});

import { Route, ProjectSchedulersSectionComponent, schedulerSectionTabs } from "./route.js";

afterEach(() => {
  router.pathname = "/acme/projects/p1/schedulers/list";
});

const BASE = "/acme/projects/p1/schedulers";

describe("schedulerSectionTabs", () => {
  it("marks List current on the list address", () => {
    expect(schedulerSectionTabs(`${BASE}/list`, BASE)).toEqual([
      { key: "list", label: "List", href: `${BASE}/list`, active: true },
      { key: "calendar", label: "Calendar", href: `${BASE}/calendar`, active: false },
    ]);
  });

  it("marks Calendar current on the calendar address", () => {
    const tabs = schedulerSectionTabs(`${BASE}/calendar`, BASE);
    expect(tabs.map((tab) => tab.active)).toEqual([false, true]);
  });

  it("reads the bare section address as Calendar", () => {
    expect(schedulerSectionTabs(BASE, BASE).find((tab) => tab.active)?.key).toBe("calendar");
    expect(schedulerSectionTabs(`${BASE}/`, BASE).find((tab) => tab.active)?.key).toBe("calendar");
  });

  it("tolerates a trailing slash on the list address", () => {
    expect(schedulerSectionTabs(`${BASE}/list/`, BASE).find((tab) => tab.active)?.key).toBe("list");
  });

  it("marks neither tab on an install detail page", () => {
    expect(schedulerSectionTabs(`${BASE}/b1`, BASE).some((tab) => tab.active)).toBe(false);
  });
});

describe("ProjectSchedulersSectionComponent", () => {
  it("renders both tabs with the current one marked and the tab body below", () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: "p1" });
    router.pathname = "/acme/projects/p1/schedulers/list";
    render(<ProjectSchedulersSectionComponent />);
    const nav = screen.getByRole("navigation", { name: "Scheduler views" });
    const list = screen.getByRole("link", { name: "List" });
    const calendar = screen.getByRole("link", { name: "Calendar" });
    expect(nav).toContainElement(list);
    expect(list).toHaveAttribute("href", "/acme/projects/p1/schedulers/list");
    expect(list).toHaveAttribute("aria-current", "page");
    expect(calendar).toHaveAttribute("href", "/acme/projects/p1/schedulers/calendar");
    expect(calendar).not.toHaveAttribute("aria-current");
    expect(screen.getByTestId("tab-body")).toBeInTheDocument();
  });

  it("moves the current mark to Calendar on that address", () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: "p1" });
    router.pathname = "/acme/projects/p1/schedulers/calendar";
    render(<ProjectSchedulersSectionComponent />);
    expect(screen.getByRole("link", { name: "Calendar" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "List" })).not.toHaveAttribute("aria-current");
  });
});

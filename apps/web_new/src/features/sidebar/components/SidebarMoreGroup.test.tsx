// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/permissions.js";
import { MORE_SECTION_OPEN_KEY } from "../more.js";
import { SidebarMoreGroup } from "./SidebarMoreGroup.js";

// Migrated-route links (F-09, NEWFRONT-528) render through the workspace
// routes this component ships in. The suite has no router; the router
// link's navigation is TanStack's own tested behavior, so it stands in as
// an anchor and the assertions still read the href.
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

function setup(role: number | null = ROLE_MEMBER, currentPath: string | null = "/acme/") {
  return render(<SidebarMoreGroup workspaceSlug="acme" role={role} currentPath={currentPath} />);
}

function linkNames(): string[] {
  return screen.queryAllByTestId("more-section-link").map((link) => link.textContent ?? "");
}

function toggle(): HTMLElement {
  return screen.getByTestId("more-section-toggle");
}

beforeEach(() => {
  window.localStorage.clear();
});

describe("SidebarMoreGroup", () => {
  it("starts closed and opens on demand", () => {
    setup();
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByTestId("more-section-panel")).not.toBeInTheDocument();
    fireEvent.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(linkNames()).toEqual([
      "Projects",
      "AI Workers",
      "Prompts",
      "Schedulers",
      "AI Dev Machines",
      "Analytics",
      "Archives",
      "Active cycles",
    ]);
    fireEvent.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByTestId("more-section-panel")).not.toBeInTheDocument();
  });

  it("persists the toggle across mounts", () => {
    const first = setup();
    fireEvent.click(toggle());
    expect(window.localStorage.getItem(MORE_SECTION_OPEN_KEY)).toBe("1");
    first.unmount();
    setup();
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
  });

  it("auto-opens on a hosted member route and persists it", () => {
    setup(ROLE_MEMBER, "/acme/schedulers/");
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(window.localStorage.getItem(MORE_SECTION_OPEN_KEY)).toBe("1");
    const schedulers = screen.getByTestId("more-section-panel").querySelector('a[href="/acme/schedulers/"]');
    expect(schedulers).toHaveAttribute("aria-current", "page");
  });

  it("leaves the toggle alone off member routes and without a path", () => {
    const first = setup(ROLE_MEMBER, "/acme/");
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    first.unmount();
    setup(ROLE_MEMBER, null);
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
  });

  it("addresses every destination below the workspace slug", () => {
    setup();
    fireEvent.click(toggle());
    const hrefs = screen.getAllByTestId("more-section-link").map((link) => link.getAttribute("href"));
    expect(hrefs).toEqual([
      "/acme/projects/",
      "/acme/runners/",
      "/acme/prompts/",
      "/acme/schedulers/",
      "/acme/ai-dev-machines/",
      "/acme/analytics/",
      "/acme/projects/archives/",
      "/acme/active-cycles/",
    ]);
  });

  it("shows guests the shared rows only", () => {
    setup(ROLE_GUEST);
    fireEvent.click(toggle());
    expect(linkNames()).toEqual(["Projects", "Prompts", "Schedulers", "Active cycles"]);
  });
});

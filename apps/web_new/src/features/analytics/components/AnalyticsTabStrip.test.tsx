// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics tab strip tests (parity/specs/analytics.md §1, ANL-001).
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: navigation is TanStack's own tested
// behavior. What this suite owns is the strip composition.
vi.mock("@tanstack/react-router", () => ({
  Link: (props: {
    to?: string;
    search?: Record<string, unknown>;
    children?: React.ReactNode;
    role?: string;
    id?: string;
    ["aria-selected"]?: boolean;
    ["aria-controls"]?: string;
    ["data-tab-id"]?: string;
    ["data-testid"]?: string;
    className?: string;
  }) => (
    <a
      href={props.to}
      data-search={JSON.stringify(props.search ?? {})}
      role={props.role}
      id={props.id}
      aria-selected={props["aria-selected"]}
      aria-controls={props["aria-controls"]}
      data-tab-id={props["data-tab-id"]}
      data-testid={props["data-testid"]}
      className={props.className}
    >
      {props.children}
    </a>
  ),
}));

import { AnalyticsTabStrip } from "./AnalyticsTabStrip.js";

describe("AnalyticsTabStrip", () => {
  it("offers both tabs with the path segment selected", () => {
    render(<AnalyticsTabStrip workspaceSlug="acme" tabId="work-items" scopeIds={[]} />);
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.getAttribute("data-tab-id"))).toEqual(["overview", "work-items"]);
    expect(tabs.map((tab) => tab.textContent)).toEqual(["Overview", "Work items"]);
    expect(screen.getByTestId("analytics-tab-overview")).toHaveAttribute("href", "/acme/analytics/overview");
    expect(screen.getByTestId("analytics-tab-work-items")).toHaveAttribute("href", "/acme/analytics/work-items");
    expect(screen.getByTestId("analytics-tab-overview")).toHaveAttribute("aria-selected", "false");
    expect(screen.getByTestId("analytics-tab-work-items")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("analytics-tab-work-items")).toHaveAttribute(
      "aria-controls",
      "analytics-panel-work-items"
    );
  });

  it("carries the picker scope on both tab links", () => {
    render(<AnalyticsTabStrip workspaceSlug="acme" tabId="overview" scopeIds={["a"]} />);
    expect(screen.getByTestId("analytics-tab-overview")).toHaveAttribute(
      "data-search",
      JSON.stringify({ project_ids: ["a"] })
    );
    expect(screen.getByTestId("analytics-tab-work-items")).toHaveAttribute(
      "data-search",
      JSON.stringify({ project_ids: ["a"] })
    );
  });

  it("selects nothing for an unknown segment while keeping both tabs", () => {
    render(<AnalyticsTabStrip workspaceSlug="acme" tabId="no-such-tab" scopeIds={[]} />);
    expect(screen.getAllByRole("tab")).toHaveLength(2);
    expect(screen.getByTestId("analytics-tab-overview")).toHaveAttribute("aria-selected", "false");
    expect(screen.getByTestId("analytics-tab-work-items")).toHaveAttribute("aria-selected", "false");
  });
});

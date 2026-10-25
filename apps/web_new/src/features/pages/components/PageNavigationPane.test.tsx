// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Navigation pane shell (parity/specs/pages.md §6.5, §6.9, PAGE-080): three
// tabs with the address-selected one marked, the close control, and the
// info timeline rows whose hrefs carry `version`.

import { fireEvent, render, screen } from "@testing-library/react";
import type { PageVersion } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));

import { PAGE_CURRENT_VERSION_LABEL, PageNavigationPane, PAGE_PANE_CLOSE } from "./PageNavigationPane.js";

const VERSIONS: PageVersion[] = [
  {
    id: "11111111-1111-4111-8111-111111111111",
    workspace: "22222222-2222-4222-8222-222222222222",
    page: "33333333-3333-4333-8333-333333333333",
    last_saved_at: "2026-10-20T12:00:00Z",
    owned_by: "44444444-4444-4444-8444-444444444444",
    created_at: "2026-10-20T12:00:00Z",
    updated_at: "2026-10-20T12:00:00Z",
    created_by: null,
    updated_by: null,
  },
];

function renderPane(
  search: React.ComponentProps<typeof PageNavigationPane>["search"],
  versions: readonly PageVersion[] | null = VERSIONS
): {
  onSelectTab: ReturnType<typeof vi.fn>;
  onClose: ReturnType<typeof vi.fn>;
  onSelectVersion: ReturnType<typeof vi.fn>;
} {
  const onSelectTab = vi.fn();
  const onClose = vi.fn();
  const onSelectVersion = vi.fn();
  render(
    <PageNavigationPane
      workspaceSlug="acme"
      projectId="project-1"
      pageId="page-1"
      search={search}
      versions={versions === null ? undefined : versions}
      onSelectTab={onSelectTab}
      onClose={onClose}
      onSelectVersion={onSelectVersion}
    />
  );
  return { onSelectTab, onClose, onSelectVersion };
}

describe("PageNavigationPane", () => {
  it("marks the address's tab and reports tab picks", () => {
    const { onSelectTab } = renderPane({ paneTab: "assets" });
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual(["Outline", "Info", "Assets"]);
    expect(screen.getByRole("tab", { name: "Assets" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Info" })).toHaveAttribute("aria-selected", "false");
    fireEvent.click(screen.getByRole("tab", { name: "Outline" }));
    expect(onSelectTab).toHaveBeenCalledWith("outline");
  });

  it("closes from its own control", () => {
    const { onClose } = renderPane({ paneTab: "outline" });
    fireEvent.click(screen.getByRole("button", { name: PAGE_PANE_CLOSE }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("renders the timeline rows with version hrefs and picks one", () => {
    const { onSelectVersion } = renderPane({ paneTab: "info" });
    expect(screen.getByText(PAGE_CURRENT_VERSION_LABEL)).toBeInTheDocument();
    const link = screen.getByRole("link", { name: /Oct 20, 2026/ });
    expect(link.getAttribute("href")).toContain(`version=${VERSIONS[0]?.id ?? ""}`);
    fireEvent.click(link);
    expect(onSelectVersion).toHaveBeenCalledWith(VERSIONS[0]?.id);
  });

  it("shares the skeleton while the history read is in flight", () => {
    renderPane({ paneTab: "info" }, null);
    expect(screen.getAllByRole("status", { name: "Loading versions" }).length).toBe(3);
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("shows no timeline outside the info tab", () => {
    renderPane({ paneTab: "outline" });
    expect(screen.queryByText(PAGE_CURRENT_VERSION_LABEL)).not.toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("keeps an unreadable timestamp verbatim", () => {
    renderPane({ paneTab: "info" }, [{ ...(VERSIONS[0] as PageVersion), last_saved_at: "not a date" }]);
    expect(screen.getByRole("link", { name: "not a date" })).toBeInTheDocument();
  });

  it("marks the open snapshot as current", () => {
    renderPane({ paneTab: "info", version: VERSIONS[0]?.id });
    expect(screen.getByRole("link", { name: /Oct 20, 2026/ })).toHaveAttribute("aria-current", "true");
  });

  it("renders without a pane tab", () => {
    renderPane({});
    expect(screen.getByRole("complementary", { name: "Navigation pane" })).toBeInTheDocument();
    expect(screen.getAllByRole("tab").every((tab) => tab.getAttribute("aria-selected") === "false")).toBe(true);
  });
});

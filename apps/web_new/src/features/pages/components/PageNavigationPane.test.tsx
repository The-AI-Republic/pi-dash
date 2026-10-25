// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Navigation pane shell (parity/specs/pages.md §6.5, §6.9, PAGE-045/080):
// the always-mounted animated aside, its ordered tabs and the tab bodies,
// the close control, the version timeline, and the edition replacement.

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

const edition = vi.hoisted(() => ({ slots: {} as Record<string, unknown> }));
vi.mock("@pidash/edition", () => ({ edition }));

import {
  PAGE_CURRENT_VERSION_LABEL,
  PageNavigationPane,
  PAGE_NAVIGATION_LABEL,
  PAGE_PANE_CLOSE,
} from "./PageNavigationPane.js";
import { PAGE_PANE_WIDTH_PX, type PageActorRow, type PageDocument } from "../lib/pagePane.js";

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

const DOCUMENT: PageDocument = {
  headings: [
    { level: 1, text: "Top", levelIndex: 1 },
    { level: 2, text: "Middle", levelIndex: 1 },
  ],
  stats: { words: 4, characters: 20, paragraphs: 1 },
  assets: [
    {
      source: "https://cdn.test/sun.png",
      name: "sun.png",
      kind: "image",
      viewHref: "https://cdn.test/sun.png",
      downloadHref: "https://cdn.test/sun.png",
      thumbnailHref: "https://cdn.test/sun.png",
    },
  ],
};

const ACTORS: PageActorRow[] = [
  {
    role: "edited",
    userId: "u-ada",
    name: "Ada Byron",
    deactivated: false,
    avatarUrl: null,
    timeText: "3h ago",
    href: "/acme/profile/u-ada/",
  },
  {
    role: "created",
    userId: "u-ada",
    name: "Ada Byron",
    deactivated: false,
    avatarUrl: null,
    timeText: "Sep 1, 2026",
    href: "/acme/profile/u-ada/",
  },
];

type Props = React.ComponentProps<typeof PageNavigationPane>;

function renderPane(
  overrides: Partial<Props> = {},
  versions: readonly PageVersion[] | null = VERSIONS
): {
  onSelectTab: ReturnType<typeof vi.fn>;
  onClose: ReturnType<typeof vi.fn>;
  onSelectVersion: ReturnType<typeof vi.fn>;
  onSelectHeading: ReturnType<typeof vi.fn>;
} {
  const onSelectTab = vi.fn();
  const onClose = vi.fn();
  const onSelectVersion = vi.fn();
  const onSelectHeading = vi.fn();
  render(
    <PageNavigationPane
      workspaceSlug="acme"
      projectId="project-1"
      pageId="page-1"
      search={{ paneTab: "outline" }}
      document={DOCUMENT}
      actors={ACTORS}
      versions={versions === null ? undefined : versions}
      onSelectTab={onSelectTab}
      onClose={onClose}
      onSelectVersion={onSelectVersion}
      onSelectHeading={onSelectHeading}
      {...overrides}
    />
  );
  return { onSelectTab, onClose, onSelectVersion, onSelectHeading };
}

describe("PageNavigationPane", () => {
  it("stays mounted, slides by its width, and is open at margin zero", () => {
    renderPane({ search: { paneTab: "outline" } });
    const aside = screen.getByRole("complementary", { name: PAGE_NAVIGATION_LABEL });
    expect(aside).toHaveAttribute("data-open", "true");
    expect(aside.style.width).toBe(`${PAGE_PANE_WIDTH_PX}px`);
    expect(aside.style.marginRight).toBe("0px");
    expect(aside.className).toContain("transition-[margin]");
  });

  it("slides shut by the pane's own width and hides the tabs and bodies", () => {
    renderPane({ search: {} });
    const aside = screen.getByRole("complementary", { name: PAGE_NAVIGATION_LABEL });
    expect(aside).toHaveAttribute("data-open", "false");
    expect(aside.style.marginRight).toBe(`-${PAGE_PANE_WIDTH_PX}px`);
    expect(screen.queryAllByRole("tab")).toEqual([]);
    expect(screen.queryByTestId("page-outline")).not.toBeInTheDocument();
    // The close control stays mounted so the pane can be named when shut.
    expect(screen.getByRole("button", { name: PAGE_PANE_CLOSE })).toBeInTheDocument();
  });

  it("orders the tabs Outline, Info, Assets and reports picks", () => {
    const { onSelectTab } = renderPane({ search: { paneTab: "assets" } });
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual(["Outline", "Info", "Assets"]);
    expect(screen.getByRole("tab", { name: "Assets" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Info" })).toHaveAttribute("aria-selected", "false");
    fireEvent.click(screen.getByRole("tab", { name: "Outline" }));
    expect(onSelectTab).toHaveBeenCalledWith("outline");
  });

  it("closes from its own control", () => {
    const { onClose } = renderPane();
    fireEvent.click(screen.getByRole("button", { name: PAGE_PANE_CLOSE }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("shows the outline, and picks a heading", () => {
    const { onSelectHeading } = renderPane();
    expect(screen.getByTestId("page-outline")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Middle" }));
    expect(onSelectHeading).toHaveBeenCalledWith(DOCUMENT.headings[1]);
  });

  it("shows the statistics and attribution on the info tab with the timeline", () => {
    renderPane({ search: { paneTab: "info" } });
    expect(screen.getByTestId("page-document-stats")).toBeInTheDocument();
    expect(screen.getByText("Read time")).toBeInTheDocument();
    expect(screen.getByTestId("page-actors")).toBeInTheDocument();
    expect(screen.getByText(PAGE_CURRENT_VERSION_LABEL)).toBeInTheDocument();
  });

  it("shows the asset rows on the assets tab", () => {
    renderPane({ search: { paneTab: "assets" } });
    expect(screen.getByTestId("page-assets")).toBeInTheDocument();
    expect(screen.getByText("sun.png")).toBeInTheDocument();
  });

  it("passes the attachment renderer through to the assets tab", () => {
    renderPane({
      search: { paneTab: "assets" },
      document: {
        ...DOCUMENT,
        assets: [{ ...DOCUMENT.assets[0]!, kind: "attachment", thumbnailHref: null }],
      },
      renderAttachment: (asset) => <span>licensed {asset.name}</span>,
    });
    expect(screen.getByText("licensed sun.png")).toBeInTheDocument();
  });

  it("lets an edition replace the tab body while keeping the shell", () => {
    edition.slots = {
      pageNavigationPane: ({ tab }: { tab: string }) => <p>edition body {tab}</p>,
    };
    try {
      renderPane({ search: { paneTab: "info" } });
      expect(screen.getByText("edition body info")).toBeInTheDocument();
      expect(screen.queryByTestId("page-document-stats")).not.toBeInTheDocument();
      expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual(["Outline", "Info", "Assets"]);
      expect(screen.getByRole("button", { name: PAGE_PANE_CLOSE })).toBeInTheDocument();
    } finally {
      edition.slots = {};
    }
  });

  it("renders the timeline rows with version hrefs and picks one", () => {
    const { onSelectVersion } = renderPane({ search: { paneTab: "info" } });
    const link = screen.getByRole("link", { name: /Oct 20, 2026/ });
    expect(link.getAttribute("href")).toContain(`version=${VERSIONS[0]?.id ?? ""}`);
    fireEvent.click(link);
    expect(onSelectVersion).toHaveBeenCalledWith(VERSIONS[0]?.id);
  });

  it("shares the skeleton while the history read is in flight", () => {
    renderPane({ search: { paneTab: "info" } }, null);
    expect(screen.getAllByRole("status", { name: "Loading versions" }).length).toBe(3);
    expect(document.querySelector("[data-version-id]")).toBeNull();
  });

  it("shows no timeline outside the info tab", () => {
    renderPane({ search: { paneTab: "outline" } });
    expect(screen.queryByText(PAGE_CURRENT_VERSION_LABEL)).not.toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("keeps an unreadable timestamp verbatim", () => {
    renderPane({ search: { paneTab: "info" } }, [{ ...(VERSIONS[0] as PageVersion), last_saved_at: "not a date" }]);
    expect(screen.getByText("not a date")).toBeInTheDocument();
  });

  it("marks the open snapshot as current", () => {
    renderPane({ search: { paneTab: "info", version: VERSIONS[0]?.id } });
    expect(screen.getByRole("link", { name: /Oct 20, 2026/ })).toHaveAttribute("aria-current", "true");
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail breadcrumb and action strip (parity/specs/pages.md §4.3, §7,
// PAGE-024/055/056/057/058/059/060): the project crumb, the Pages link,
// the switcher, the shelved marker, the sync badge, the star, the lock
// control and the copy confirmation, plus the options menu in its pinned
// order.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { PageDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const router = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => router.navigate,
  Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));

vi.mock("../../../shared/shell/favorites.js", () => ({
  isFavoritesSectionOpen: () => true,
  setFavoritesSectionOpen: vi.fn(),
}));
vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyPending: () => ({ settle: vi.fn() }),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { PAGES_PAGE_ID, PAGES_PROJECT_ID, makePageRow } from "../testing.js";
import { PAGE_PREFERENCES_DEFAULTS } from "../lib/pagePreferences.js";
import { PAGE_FULL_WIDTH_LABEL, PAGE_STICKY_TOOLBAR_LABEL } from "../lib/pageDisplayMenu.js";
import { PAGE_COPY_MARKDOWN_LABEL, registerPageBodyActions, resetPageBodyActions } from "../lib/pageBodyActions.js";
import {
  PageDetailHeader,
  PAGE_ANCESTOR_TRAIL_LABEL,
  PAGE_COPIED_LINK_LABEL,
  PAGE_COPY_LINK_LABEL,
  PAGE_CRUMB,
  PAGE_HISTORY_ITEM,
  PAGE_PANE_TOGGLE_LABEL,
  PAGE_OPTIONS_LABEL,
} from "./PageDetailHeader.js";

const page: PageDetail = {
  ...makePageRow({ id: PAGES_PAGE_ID, name: "Launch notes" }),
  description_html: "",
};

function renderHeader(overrides: Partial<React.ComponentProps<typeof PageDetailHeader>> = {}): {
  onCopyLink: ReturnType<typeof vi.fn>;
  onOpenHistory: ReturnType<typeof vi.fn>;
  onDuplicate: ReturnType<typeof vi.fn>;
  onToggleArchive: ReturnType<typeof vi.fn>;
  onToggleVisibility: ReturnType<typeof vi.fn>;
  onRequestDelete: ReturnType<typeof vi.fn>;
  onOpenPane: ReturnType<typeof vi.fn>;
  onSelectSibling: ReturnType<typeof vi.fn>;
  onPreferenceChange: ReturnType<typeof vi.fn>;
} {
  const onCopyLink = vi.fn();
  const onOpenHistory = vi.fn();
  const onDuplicate = vi.fn();
  const onToggleArchive = vi.fn();
  const onToggleVisibility = vi.fn();
  const onRequestDelete = vi.fn();
  const onPreferenceChange = vi.fn();
  const onOpenPane = vi.fn();
  const onSelectSibling = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PageDetailHeader
        workspaceSlug="acme"
        projectId={PAGES_PROJECT_ID}
        projectName="Website relaunch"
        projectLogo={{}}
        projectIdentifier="WEB"
        page={page}
        siblings={[{ id: "other", name: "Other page", logoProps: {} }]}
        syncStatus="lost"
        showStar
        showLock
        canDuplicate
        canManage
        onSelectSibling={onSelectSibling}
        onCopyLink={onCopyLink}
        onOpenHistory={onOpenHistory}
        onDuplicate={onDuplicate}
        onToggleArchive={onToggleArchive}
        onToggleVisibility={onToggleVisibility}
        onRequestDelete={onRequestDelete}
        onOpenPane={onOpenPane}
        paneOpen={false}
        preferences={PAGE_PREFERENCES_DEFAULTS}
        onPreferenceChange={onPreferenceChange}
        {...overrides}
      />
    </QueryClientProvider>
  );
  return {
    onCopyLink,
    onOpenHistory,
    onDuplicate,
    onToggleArchive,
    onToggleVisibility,
    onRequestDelete,
    onOpenPane,
    onSelectSibling,
    onPreferenceChange,
  };
}

beforeEach(() => {
  createAppClient("https://api.test", () => new Promise<never>(() => undefined));
  router.navigate.mockReset();
});

afterEach(() => {
  resetAppClient();
  resetPageBodyActions();
});

describe("PageDetailHeader", () => {
  it("is the unique h-11 strip and leads with the project crumb", () => {
    renderHeader();
    const bar = screen.getByTestId("page-detail-header");
    expect(bar).toHaveClass("flex", "h-11");
    const buttons = screen.getAllByRole("button");
    expect(buttons[0]).toHaveTextContent("Website relaunch");
  });

  it("links back to the collection and to the project", () => {
    renderHeader();
    expect(screen.getByRole("link", { name: PAGE_CRUMB })).toHaveAttribute(
      "href",
      `/acme/projects/${PAGES_PROJECT_ID}/pages?type=public`
    );
    fireEvent.click(screen.getByRole("button", { name: /Website relaunch/ }));
    expect(router.navigate).toHaveBeenCalledWith({
      to: "/$ws/projects/$projectId",
      params: { ws: "acme", projectId: PAGES_PROJECT_ID },
    });
  });

  it("offers the searchable switcher and reports a pick", () => {
    const { onSelectSibling } = renderHeader();
    fireEvent.click(screen.getByRole("button", { name: /Switch page, current Launch notes/ }));
    fireEvent.click(screen.getByRole("option", { name: "Other page" }));
    expect(onSelectSibling).toHaveBeenCalledWith("other");
  });

  it("offers the navigation-pane toggle in the action strip", () => {
    const { onOpenPane } = renderHeader();
    const toggle = screen.getByRole("button", { name: PAGE_PANE_TOGGLE_LABEL });
    expect(toggle.querySelector("svg.lucide-panel-right")).not.toBeNull();
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(onOpenPane).toHaveBeenCalledTimes(1);
  });

  it("reports the pane as expanded while it is open", () => {
    renderHeader({ paneOpen: true });
    expect(screen.getByRole("button", { name: PAGE_PANE_TOGGLE_LABEL })).toHaveAttribute("aria-expanded", "true");
  });

  it("shows the sync badge and hides the star from guests", () => {
    renderHeader();
    expect(screen.getByText("Connection lost")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /favorite/i })).toBeInTheDocument();
  });

  it("never mounts the star for a guest", () => {
    renderHeader({ showStar: false });
    expect(screen.queryByRole("button", { name: /favorite/i })).not.toBeInTheDocument();
  });

  it("confirms the copy press before reverting", async () => {
    const { onCopyLink } = renderHeader();
    fireEvent.click(screen.getByRole("button", { name: PAGE_COPY_LINK_LABEL }));
    expect(onCopyLink).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: PAGE_COPIED_LINK_LABEL })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: PAGE_COPY_LINK_LABEL })).toBeInTheDocument(), {
      timeout: 4000,
    });
  });

  it("restarts the copy confirmation on a second press", async () => {
    const { onCopyLink } = renderHeader();
    fireEvent.click(screen.getByRole("button", { name: PAGE_COPY_LINK_LABEL }));
    fireEvent.click(screen.getByRole("button", { name: PAGE_COPIED_LINK_LABEL }));
    expect(onCopyLink).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("button", { name: PAGE_COPIED_LINK_LABEL })).toBeInTheDocument();
  });

  it("shows every ancestor top-down with each visible one linking to its page", () => {
    renderHeader({
      ancestors: [
        { id: "top", name: "Handbook", visible: true },
        { id: "middle", name: "Policies", visible: true },
      ],
    });
    const trail = screen.getByLabelText(PAGE_ANCESTOR_TRAIL_LABEL);
    const links = within(trail).getAllByRole("link");
    expect(links.map((link) => link.textContent)).toEqual(["Handbook", "Policies"]);
    expect(links[0]).toHaveAttribute("href", `/acme/projects/${PAGES_PROJECT_ID}/pages/top`);
    expect(links[1]).toHaveAttribute("href", `/acme/projects/${PAGES_PROJECT_ID}/pages/middle`);
  });

  it("renders an invisible ancestor as the restricted fallback without a link", () => {
    renderHeader({
      ancestors: [
        { id: "middle", name: "Restricted page", visible: false },
        { id: "top", name: "Handbook", visible: true },
      ],
    });
    const trail = screen.getByLabelText(PAGE_ANCESTOR_TRAIL_LABEL);
    expect(within(trail).getByText("Restricted page")).toBeInTheDocument();
    expect(within(trail).queryByRole("link", { name: "Restricted page" })).not.toBeInTheDocument();
    expect(
      within(trail)
        .getAllByRole("link")
        .map((link) => link.textContent)
    ).toEqual(["Handbook"]);
  });

  it("omits the trail for a top-level page", () => {
    renderHeader();
    expect(screen.queryByLabelText(PAGE_ANCESTOR_TRAIL_LABEL)).not.toBeInTheDocument();
  });

  it("orders the owner menu exactly as the oracle pins it and runs every entry", async () => {
    const onAddSubPage = vi.fn();
    const onExport = vi.fn();
    const { onOpenHistory, onDuplicate, onToggleArchive, onToggleVisibility } = renderHeader({
      onAddSubPage,
      onExport,
    });
    const copyMarkdown = vi.fn(async () => undefined);
    registerPageBodyActions({ copyMarkdownLabel: PAGE_COPY_MARKDOWN_LABEL, copyMarkdown });
    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    const items = (await screen.findAllByRole("menuitem")).map((item) => item.textContent);
    expect(items).toEqual([
      "Full width",
      "Sticky toolbar",
      PAGE_COPY_MARKDOWN_LABEL,
      PAGE_HISTORY_ITEM,
      "Add sub-page",
      "Make a copy",
      "Archive",
      "Make private",
      "Export",
    ]);
    fireEvent.click(screen.getByRole("menuitem", { name: PAGE_COPY_MARKDOWN_LABEL }));
    expect(copyMarkdown).toHaveBeenCalledTimes(1);
    for (const [name, handler] of [
      [PAGE_HISTORY_ITEM, onOpenHistory],
      ["Make a copy", onDuplicate],
      ["Archive", onToggleArchive],
      ["Make private", onToggleVisibility],
      ["Add sub-page", onAddSubPage],
      ["Export", onExport],
    ] as const) {
      fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
      fireEvent.click(await screen.findByRole("menuitem", { name }));
      expect(handler).toHaveBeenCalledTimes(1);
    }
  });

  it("reduces the menu for a member and hides the lock control", async () => {
    renderHeader({ canManage: false, showLock: false });
    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    const items = (await screen.findAllByRole("menuitem")).map((item) => item.textContent);
    expect(items).toEqual(["Full width", "Sticky toolbar", PAGE_HISTORY_ITEM, "Make a copy"]);
    expect(screen.queryByRole("button", { name: "Lock" })).not.toBeInTheDocument();
  });

  it("marks a shelved page and swaps its menu to restore plus delete", async () => {
    const { onToggleArchive, onRequestDelete } = renderHeader({
      page: { ...page, archived_at: "2026-03-04" },
    });
    expect(screen.getByTestId("page-archived-marker")).toHaveTextContent(/^Archived at .+, 2026$/);

    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    const items = (await screen.findAllByRole("menuitem")).map((item) => item.textContent);
    expect(items).toEqual(["Full width", PAGE_HISTORY_ITEM, "Make a copy", "Restore", "Delete"]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Restore" }));
    expect(onToggleArchive).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete" }));
    expect(onRequestDelete).toHaveBeenCalledTimes(1);
  });

  it("shows no marker and no delete entry on a live page", async () => {
    renderHeader();
    expect(screen.queryByTestId("page-archived-marker")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    await screen.findByRole("menuitem", { name: "Archive" });
    expect(screen.queryByRole("menuitem", { name: "Delete" })).not.toBeInTheDocument();
  });

  it("offers Make public for a restricted page", async () => {
    renderHeader({ page: { ...page, access: 1 } });
    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    expect(await screen.findByRole("menuitem", { name: "Make public" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Make private" })).not.toBeInTheDocument();
  });

  it("hides the sub-page entry from a guest", async () => {
    renderHeader();
    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    expect(await screen.findByRole("menuitem", { name: PAGE_HISTORY_ITEM })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Add sub-page" })).not.toBeInTheDocument();
  });

  it("runs the history shortcut from the options menu", async () => {
    const { onOpenHistory } = renderHeader();
    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    fireEvent.click(await screen.findByRole("menuitem", { name: PAGE_HISTORY_ITEM }));
    expect(onOpenHistory).toHaveBeenCalledTimes(1);
  });

  it("shows the lock control to a manager", () => {
    renderHeader();
    expect(screen.getByRole("button", { name: "Lock" })).toBeInTheDocument();
  });

  it("shows the locked state for a locked page", () => {
    renderHeader({ page: { ...page, is_locked: true } });
    expect(screen.getByRole("button", { name: "Locked" })).toBeInTheDocument();
  });

  it("adds the live editor's copy-markdown entry after registration (PAGE-044)", async () => {
    renderHeader();
    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    expect(screen.queryByRole("menuitem", { name: PAGE_COPY_MARKDOWN_LABEL })).not.toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });

    const copyMarkdown = vi.fn(async () => undefined);
    registerPageBodyActions({ copyMarkdownLabel: PAGE_COPY_MARKDOWN_LABEL, copyMarkdown });
    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    fireEvent.click(await screen.findByRole("menuitem", { name: PAGE_COPY_MARKDOWN_LABEL }));
    expect(copyMarkdown).toHaveBeenCalledTimes(1);
  });
});

describe("PageDetailHeader display toggles", () => {
  it("leads the options menu with the two display rows and writes one preference", async () => {
    const { onPreferenceChange } = renderHeader();
    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    const items = await screen.findAllByRole("menuitem");
    expect(items.slice(0, 2).map((item) => item.textContent)).toEqual([
      PAGE_FULL_WIDTH_LABEL,
      PAGE_STICKY_TOOLBAR_LABEL,
    ]);
    fireEvent.click(screen.getByRole("menuitem", { name: PAGE_FULL_WIDTH_LABEL }));
    expect(onPreferenceChange).toHaveBeenCalledWith({ full_width: true });
    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    fireEvent.click(await screen.findByRole("menuitem", { name: PAGE_STICKY_TOOLBAR_LABEL }));
    expect(onPreferenceChange).toHaveBeenCalledWith({ sticky_toolbar: false });
  });

  it("offers no typeface, size or shortcut row", async () => {
    renderHeader();
    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    const items = await screen.findAllByRole("menuitem");
    const labels = items.map((item) => item.textContent ?? "");
    expect(labels.some((label) => /font|typeface|typography|shortcut/i.test(label))).toBe(false);
  });
});

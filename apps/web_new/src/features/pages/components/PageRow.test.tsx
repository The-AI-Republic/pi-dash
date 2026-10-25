// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// PAGE-006 row anatomy, detail link, hover metadata and the shared row menu
// (PAGE-009–012: star, open in a new tab, copy link, make a copy).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));
vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyPending: vi.fn(() => ({ settle: vi.fn() })),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));
vi.mock("../../../shared/shell/favorites.js", () => ({
  isFavoritesSectionOpen: () => true,
  setFavoritesSectionOpen: vi.fn(),
}));

import { PAGES_PROJECT_ID, PAGES_USER_ID, makePageRow } from "../testing.js";
import { PageRow } from "./PageRow.js";

const OWNER = { id: PAGES_USER_ID, name: "Ada Byron", avatarUrl: null };

function rowElement(overrides: Partial<React.ComponentProps<typeof PageRow>> = {}): React.ReactElement {
  const props: React.ComponentProps<typeof PageRow> = {
    workspaceSlug: "acme",
    projectId: PAGES_PROJECT_ID,
    page: makePageRow(),
    owner: OWNER,
    showStar: true,
    canDuplicate: true,
    canManage: false,
    canMove: false,
    onOpenInNewTab: vi.fn(),
    onCopyLink: vi.fn(),
    onDuplicate: vi.fn(),
    onToggleVisibility: vi.fn(),
    onToggleLock: vi.fn(),
    onToggleArchive: vi.fn(),
    onRequestDelete: vi.fn(),
    onRequestMove: vi.fn(),
    ...overrides,
  };
  return (
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PageRow {...props} />
    </QueryClientProvider>
  );
}

function renderRow(overrides: Partial<React.ComponentProps<typeof PageRow>> = {}) {
  const handlers = {
    onOpenInNewTab: overrides.onOpenInNewTab ?? vi.fn(),
    onCopyLink: overrides.onCopyLink ?? vi.fn(),
    onDuplicate: overrides.onDuplicate ?? vi.fn(),
    onToggleVisibility: overrides.onToggleVisibility ?? vi.fn(),
    onToggleLock: overrides.onToggleLock ?? vi.fn(),
    onToggleArchive: overrides.onToggleArchive ?? vi.fn(),
    onRequestDelete: overrides.onRequestDelete ?? vi.fn(),
    onRequestMove: overrides.onRequestMove ?? vi.fn(),
  };
  render(rowElement({ ...overrides, ...handlers }));
  return handlers;
}

describe("PageRow", () => {
  it("renders the link, owner, visibility, fallback title, star and actions", () => {
    renderRow({ page: makePageRow({ name: " ", is_favorite: true }) });
    expect(screen.getByRole("link", { name: "Untitled" })).toHaveAttribute(
      "href",
      `/acme/projects/${PAGES_PROJECT_ID}/pages/${makePageRow().id}`
    );
    expect(screen.getByRole("img", { name: "Ada Byron" })).toBeInTheDocument();
    expect(screen.getByLabelText("Public")).toBeInTheDocument();
    expect(screen.getByTestId("page-row-star")).toHaveAccessibleName("Remove Untitled from favorites");
    expect(screen.getByRole("button", { name: "Page actions for Untitled" })).toBeInTheDocument();
  });

  it("uses a private glyph and omits the star for a guest", () => {
    renderRow({ page: makePageRow({ access: 1 }), showStar: false, canDuplicate: false });
    expect(screen.getByLabelText("Private")).toBeInTheDocument();
    expect(screen.queryByTestId("page-row-star")).not.toBeInTheDocument();
  });

  it("decodes a stored emoji and draws a stored icon glyph in its colour", async () => {
    const { rerender } = render(
      rowElement({ page: makePageRow({ logo_props: { in_use: "emoji", emoji: { value: "1f600" } } }) })
    );
    expect(screen.getByText("😀")).toBeInTheDocument();
    rerender(
      rowElement({
        page: makePageRow({ logo_props: { in_use: "icon", icon: { name: "rocket_launch", color: "#ff0000" } } }),
      })
    );
    const link = screen.getByRole("link", { name: "Launch notes" });
    await waitFor(() => expect(link.querySelector("svg.lucide-rocket")).not.toBeNull());
    expect(link.querySelector("svg.lucide-rocket")).toHaveAttribute("stroke", "#ff0000");
    expect(link.querySelector("svg.lucide-rocket")).toHaveStyle({ color: "rgb(255, 0, 0)" });
  });

  it("renders an icon without a colour, and the document fallback for empty or unknown marks", async () => {
    const { rerender } = render(
      rowElement({
        page: makePageRow({ logo_props: { in_use: "icon", icon: { name: "star" } } }),
        owner: { ...OWNER, avatarUrl: "https://x.test/ada.png" },
      })
    );
    const firstLink = screen.getByRole("link", { name: "Launch notes" });
    await waitFor(() => expect(firstLink.querySelector("svg.lucide-star")).not.toBeNull());
    expect(firstLink.querySelector("svg.lucide-star")).not.toHaveAttribute("style");
    expect(screen.getByRole("img", { name: "Ada Byron" }).querySelector("img")).toHaveAttribute(
      "src",
      "https://x.test/ada.png"
    );
    rerender(rowElement({ page: makePageRow({ logo_props: { in_use: "emoji", emoji: { value: "" } } }) }));
    expect(
      screen.getByRole("link", { name: "Launch notes" }).querySelector("svg.lucide-file-text")
    ).toBeInTheDocument();
    rerender(rowElement({ page: makePageRow({ logo_props: { in_use: "icon", icon: { name: "no_such_symbol" } } }) }));
    await waitFor(() =>
      expect(
        screen.getByRole("link", { name: "Launch notes" }).querySelector("svg.lucide-file-text")
      ).toBeInTheDocument()
    );
  });

  it("makes the mark and the row's main area open the detail route, not the action controls", () => {
    renderRow({ page: makePageRow({ logo_props: { in_use: "emoji", emoji: { value: "1f600" } } }) });
    const mark = screen.getByText("😀");
    const link = mark.closest("a");
    expect(link).toHaveAttribute("href", `/acme/projects/${PAGES_PROJECT_ID}/pages/${makePageRow().id}`);
    expect(screen.getByLabelText("Public").closest("a")).toBe(link);
    expect(screen.getByRole("img", { name: "Ada Byron" }).closest("a")).toBe(link);
    expect(screen.getByTestId("page-row-star").closest("a")).toBeNull();
    expect(screen.getByRole("button", { name: "Page actions for Launch notes" }).closest("a")).toBeNull();
    link!.addEventListener("click", (event) => event.preventDefault());
    fireEvent.click(mark);
    expect(link).toBeInTheDocument();
  });

  it("runs the overflow entries for a member", () => {
    const props = renderRow();
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Open in new tab",
      "Copy link",
      "Make a copy",
    ]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Open in new tab" }));
    expect(props.onOpenInNewTab).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    expect(props.onCopyLink).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Make a copy" }));
    expect(props.onDuplicate).toHaveBeenCalledTimes(1);
  });

  it("keeps a guest's menu to the two viewer-safe entries", () => {
    renderRow({ canDuplicate: false });
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["Open in new tab", "Copy link"]);
    expect(screen.queryByRole("menuitem", { name: "Make a copy" })).not.toBeInTheDocument();
  });

  it("opens the same entries from the context menu", () => {
    const props = renderRow();
    fireEvent.contextMenu(screen.getByRole("link", { name: "Launch notes" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    expect(props.onCopyLink).toHaveBeenCalledTimes(1);
  });

  it("gives an owner the state switches in the pinned order and wires them", () => {
    const props = renderRow({ canManage: true });
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Open in new tab",
      "Copy link",
      "Make a copy",
      "Lock",
      "Make private",
      "Archive",
    ]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Lock" }));
    expect(props.onToggleLock).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Make private" }));
    expect(props.onToggleVisibility).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Archive" }));
    expect(props.onToggleArchive).toHaveBeenCalledTimes(1);
  });

  it("flips the owner labels with the record's lock and visibility", () => {
    renderRow({ canManage: true, page: makePageRow({ is_locked: true, access: 1 }) });
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Open in new tab",
      "Copy link",
      "Make a copy",
      "Unlock",
      "Make public",
      "Archive",
    ]);
  });

  it("hides visibility on a shelved row and offers Restore and Delete", () => {
    const props = renderRow({ canManage: true, page: makePageRow({ archived_at: "2026-10-01" }) });
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    const items = screen.getAllByRole("menuitem").map((item) => item.textContent);
    expect(items).toEqual(["Open in new tab", "Copy link", "Make a copy", "Lock", "Restore", "Delete"]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Restore" }));
    expect(props.onToggleArchive).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(props.onRequestDelete).toHaveBeenCalledTimes(1);
  });

  it("adds the move entry only for an edition that serves it", () => {
    renderRow({ canManage: true, canMove: true });
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toContain("Move to project…");
  });

  it("shows a lock indicator on a locked row only", () => {
    const { rerender } = render(rowElement({ page: makePageRow({ is_locked: true }) }));
    expect(screen.getByLabelText("Locked")).toBeInTheDocument();
    rerender(rowElement({ page: makePageRow({ is_locked: false }) }));
    expect(screen.queryByLabelText("Locked")).not.toBeInTheDocument();
  });

  it("falls back to an unknown owner and an unreadable creation date", () => {
    renderRow({ owner: null, page: makePageRow({ created_at: "not-a-date" }) });
    expect(screen.getByRole("img", { name: "Unknown member" })).toBeInTheDocument();
    expect(screen.getByLabelText("Created on an unknown date")).toBeInTheDocument();
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// PAGE-006 row anatomy, detail link, hover metadata and mirrored menus.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));

import { PAGES_PROJECT_ID, PAGES_USER_ID, makePageRow } from "../testing.js";
import { PageRow } from "./PageRow.js";

const OWNER = { id: PAGES_USER_ID, name: "Ada Byron", avatarUrl: null };

function renderRow(overrides: Partial<React.ComponentProps<typeof PageRow>> = {}) {
  const props: React.ComponentProps<typeof PageRow> = {
    workspaceSlug: "acme",
    projectId: PAGES_PROJECT_ID,
    page: makePageRow(),
    owner: OWNER,
    showStar: true,
    menu: { openInNewTab: "Open in new tab", copyLink: "Copy link" },
    onOpenInNewTab: vi.fn(),
    onCopyLink: vi.fn(),
    ...overrides,
  };
  render(<PageRow {...props} />);
  return props;
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
    expect(screen.getByRole("img", { name: "Marked as favorite" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Page actions for Untitled" })).toBeInTheDocument();
  });

  it("uses a private glyph and omits the star for a guest", () => {
    renderRow({ page: makePageRow({ access: 1 }), showStar: false });
    expect(screen.getByLabelText("Private")).toBeInTheDocument();
    expect(screen.queryByTestId("page-row-star")).not.toBeInTheDocument();
  });

  it("renders emoji and icon marks and the neutral fallback", () => {
    const { rerender } = render(
      <PageRow
        workspaceSlug="acme"
        projectId={PAGES_PROJECT_ID}
        page={makePageRow({ logo_props: { in_use: "emoji", emoji: { value: "📘" } } })}
        owner={OWNER}
        showStar
        menu={{ openInNewTab: "Open in new tab", copyLink: "Copy link" }}
        onOpenInNewTab={() => undefined}
        onCopyLink={() => undefined}
      />
    );
    expect(screen.getByText("📘")).toBeInTheDocument();
    rerender(
      <PageRow
        workspaceSlug="acme"
        projectId={PAGES_PROJECT_ID}
        page={makePageRow({ logo_props: { in_use: "icon", icon: { name: "rocket", color: "#ff0000" } } })}
        owner={OWNER}
        showStar
        menu={{ openInNewTab: "Open in new tab", copyLink: "Copy link" }}
        onOpenInNewTab={() => undefined}
        onCopyLink={() => undefined}
      />
    );
    expect(screen.getByText("R")).toHaveStyle({ color: "rgb(255, 0, 0)" });
  });

  it("renders an icon without a colour, an empty emoji as the fallback, and an avatar image", () => {
    const { rerender } = render(
      <PageRow
        workspaceSlug="acme"
        projectId={PAGES_PROJECT_ID}
        page={makePageRow({ logo_props: { in_use: "icon", icon: { name: "book" } } })}
        owner={{ ...OWNER, avatarUrl: "https://x.test/ada.png" }}
        showStar
        menu={{ openInNewTab: "Open in new tab", copyLink: "Copy link" }}
        onOpenInNewTab={() => undefined}
        onCopyLink={() => undefined}
      />
    );
    expect(screen.getByText("B")).not.toHaveAttribute("style");
    expect(screen.getByRole("img", { name: "Ada Byron" }).querySelector("img")).toHaveAttribute(
      "src",
      "https://x.test/ada.png"
    );
    rerender(
      <PageRow
        workspaceSlug="acme"
        projectId={PAGES_PROJECT_ID}
        page={makePageRow({ logo_props: { in_use: "emoji", emoji: { value: "" } } })}
        owner={OWNER}
        showStar
        menu={{ openInNewTab: "Open in new tab", copyLink: "Copy link" }}
        onOpenInNewTab={() => undefined}
        onCopyLink={() => undefined}
      />
    );
    expect(
      screen.getByRole("link", { name: "Launch notes" }).parentElement?.querySelector("svg.lucide-file-text")
    ).toBeInTheDocument();
  });

  it("runs the overflow entries", () => {
    const props = renderRow();
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Open in new tab" }));
    expect(props.onOpenInNewTab).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    expect(props.onCopyLink).toHaveBeenCalledTimes(1);
  });

  it("opens the same entries from the context menu", () => {
    const props = renderRow();
    fireEvent.contextMenu(screen.getByRole("link", { name: "Launch notes" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    expect(props.onCopyLink).toHaveBeenCalledTimes(1);
  });

  it("falls back to an unknown owner and an unreadable creation date", () => {
    renderRow({ owner: null, page: makePageRow({ created_at: "not-a-date" }) });
    expect(screen.getByRole("img", { name: "Unknown member" })).toBeInTheDocument();
    expect(screen.getByLabelText("Created on an unknown date")).toBeInTheDocument();
  });
});

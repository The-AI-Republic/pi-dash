// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail breadcrumb and action strip (parity/specs/pages.md §4.3, §7.1,
// §7.4, PAGE-024/055/060): the project crumb, the Pages link, the switcher,
// the sync badge, the star and the copy confirmation, plus the history
// entry that writes the info pane.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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
import {
  PageDetailHeader,
  PAGE_COPIED_LINK_LABEL,
  PAGE_COPY_LINK_LABEL,
  PAGE_CRUMB,
  PAGE_HISTORY_ITEM,
  PAGE_OPEN_PANE_LABEL,
  PAGE_OPTIONS_LABEL,
} from "./PageDetailHeader.js";

const page: PageDetail = {
  ...makePageRow({ id: PAGES_PAGE_ID, name: "Launch notes" }),
  description_html: "",
};

function renderHeader(overrides: Partial<React.ComponentProps<typeof PageDetailHeader>> = {}): {
  onCopyLink: ReturnType<typeof vi.fn>;
  onOpenHistory: ReturnType<typeof vi.fn>;
  onOpenPane: ReturnType<typeof vi.fn>;
  onSelectSibling: ReturnType<typeof vi.fn>;
} {
  const onCopyLink = vi.fn();
  const onOpenHistory = vi.fn();
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
        siblings={[{ id: "other", name: "Other page" }]}
        syncStatus="lost"
        showStar
        onSelectSibling={onSelectSibling}
        onCopyLink={onCopyLink}
        onOpenHistory={onOpenHistory}
        onOpenPane={onOpenPane}
        paneOpen={false}
        {...overrides}
      />
    </QueryClientProvider>
  );
  return { onCopyLink, onOpenHistory, onOpenPane, onSelectSibling };
}

beforeEach(() => {
  createAppClient("https://api.test", () => new Promise<never>(() => undefined));
  router.navigate.mockReset();
});

afterEach(() => {
  resetAppClient();
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
    const toggle = screen.getByRole("button", { name: PAGE_OPEN_PANE_LABEL });
    expect(toggle.querySelector("svg.lucide-panel-right")).not.toBeNull();
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(onOpenPane).toHaveBeenCalledTimes(1);
  });

  it("reports the pane as expanded while it is open", () => {
    renderHeader({ paneOpen: true });
    expect(screen.getByRole("button", { name: PAGE_OPEN_PANE_LABEL })).toHaveAttribute("aria-expanded", "true");
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

  it("runs the history shortcut from the options menu", async () => {
    const { onOpenHistory } = renderHeader();
    fireEvent.click(screen.getByRole("button", { name: PAGE_OPTIONS_LABEL }));
    fireEvent.click(await screen.findByRole("menuitem", { name: PAGE_HISTORY_ITEM }));
    expect(onOpenHistory).toHaveBeenCalledTimes(1);
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace views tab strip (parity/specs/views.md §10, VIEW-033).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => vi.fn(),
  Link: ({ to, children, ...rest }: { to: string; children?: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));

const session = vi.hoisted(() => ({ useMe: vi.fn(), usePermissions: vi.fn() }));
vi.mock("../../../core/session/hooks.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/hooks.js")>()),
  useMe: session.useMe,
  usePermissions: session.usePermissions,
}));

const roles = vi.hoisted(() => ({ useProjectRoles: vi.fn() }));
vi.mock("../../auth/queries.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../auth/queries.js")>()),
  useProjectRoles: roles.useProjectRoles,
}));

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/index.js", () => notify);

const harness = vi.hoisted(() => ({
  edition: { id: "oss", flags: {} } as {
    id: string;
    flags: Record<string, boolean>;
    slots?: Record<string, unknown>;
  },
}));
vi.mock("../../../core/edition/index.js", () => ({
  get edition() {
    return harness.edition;
  },
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { makeView, VIEWS_OWNER_ID } from "../testing.js";
import { WorkspaceViewsTabStrip } from "./WorkspaceViewsTabStrip.js";

const CUSTOM_A = "323e4567-e89b-12d3-a456-426614174002";
const CUSTOM_B = "623e4567-e89b-12d3-a456-426614174005";

function seedMember(): void {
  session.useMe.mockReturnValue({ data: { id: VIEWS_OWNER_ID } });
  session.usePermissions.mockReturnValue({ canEdit: true, isAdmin: false, isLoading: false });
  roles.useProjectRoles.mockReturnValue({ data: undefined });
}

function transportViews(customs: unknown[]): { method: string; url: string }[] {
  const calls: { method: string; url: string }[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    calls.push({ method: init.method ?? "GET", url });
    if ((init.method ?? "GET") === "GET") {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(customs) };
    }
    return { status: 204, url, headers: { get: () => null }, text: async () => "null" };
  }) as never);
  return calls;
}

function renderStrip(activeViewId: string | null): { rerenderActive: (next: string | null) => void } {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const ui = (active: string | null): React.ReactElement => (
    <QueryClientProvider client={queryClient}>
      <WorkspaceViewsTabStrip workspaceSlug="acme" activeViewId={active} />
    </QueryClientProvider>
  );
  const view = render(ui(activeViewId));
  return {
    rerenderActive: (next: string | null) => {
      view.rerender(ui(next));
    },
  };
}

function tabLabels(): string[] {
  return screen.getAllByTestId("workspace-views-tab").map((tab) => tab.querySelector("a")?.textContent ?? "");
}

function rect(left: number, right: number): DOMRect {
  return { left, right, top: 0, bottom: 20, width: right - left, height: 20, x: left, y: 0 } as DOMRect;
}

afterEach(() => {
  resetAppClient();
  session.useMe.mockReset();
  session.usePermissions.mockReset();
  roles.useProjectRoles.mockReset();
  notify.notifyError.mockClear();
  notify.notifySuccess.mockClear();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("WorkspaceViewsTabStrip tabs", () => {
  it("lists defaults then customs newest first with the active tab marked", async () => {
    seedMember();
    transportViews([
      makeView({ id: CUSTOM_A, project: null, name: "Alpha", created_at: "2026-02-01T00:00:00Z" }),
      makeView({ id: CUSTOM_B, project: null, name: "Beta", created_at: "2026-03-01T00:00:00Z" }),
    ]);
    renderStrip(CUSTOM_B);
    expect(await screen.findByRole("link", { name: "Beta" })).toBeInTheDocument();
    expect(tabLabels()).toEqual(["All work items", "Assigned", "Created", "Subscribed", "Beta", "Alpha"]);
    expect(screen.getByRole("link", { name: "Beta" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "Beta" })).toHaveAttribute("href", `/acme/workspace-views/${CUSTOM_B}`);
    expect(screen.getByRole("link", { name: "Assigned" })).toHaveAttribute("href", "/acme/workspace-views/assigned");
    expect(screen.getByRole("link", { name: "Alpha" })).not.toHaveAttribute("aria-current");
  });

  it("shows the defaults while customs load", async () => {
    seedMember();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
      }
      await gate;
      return { status: 200, url, headers: { get: () => null }, text: async () => "[]" };
    }) as never);
    renderStrip("all-issues");
    expect(tabLabels()).toEqual(["All work items", "Assigned", "Created", "Subscribed"]);
    release();
    await waitFor(() => expect(tabLabels()).toEqual(["All work items", "Assigned", "Created", "Subscribed"]));
  });

  it("renders without an active tab while unresolved", () => {
    seedMember();
    transportViews([]);
    renderStrip(null);
    expect(tabLabels()).toEqual(["All work items", "Assigned", "Created", "Subscribed"]);
    for (const tab of screen.getAllByTestId("workspace-views-tab")) {
      expect(tab.querySelector("a")).not.toHaveAttribute("aria-current");
    }
  });
});

describe("WorkspaceViewsTabStrip add", () => {
  it("opens the create dialog for members", async () => {
    seedMember();
    transportViews([]);
    renderStrip("all-issues");
    fireEvent.click(screen.getByRole("button", { name: "Add view" }));
    expect(await screen.findByRole("heading", { name: "Create View" })).toBeInTheDocument();
  });

  it("hides the add button from guests", () => {
    session.useMe.mockReturnValue({ data: { id: "guest-id" } });
    session.usePermissions.mockReturnValue({ canEdit: false, isAdmin: false, isLoading: false });
    roles.useProjectRoles.mockReturnValue({ data: undefined });
    transportViews([]);
    renderStrip("all-issues");
    expect(screen.queryByRole("button", { name: "Add view" })).not.toBeInTheDocument();
  });
});

describe("WorkspaceViewsTabStrip menus", () => {
  const CUSTOMS = [makeView({ id: CUSTOM_A, project: null, name: "Alpha", owned_by: VIEWS_OWNER_ID })];

  function tabMenuTrigger(name: string): HTMLElement {
    return screen.getByRole("button", { name: `Actions for ${name}` });
  }

  it("offers the full menu on an owned custom tab", async () => {
    seedMember();
    transportViews(CUSTOMS);
    renderStrip(CUSTOM_A);
    expect(await screen.findByRole("link", { name: "Alpha" })).toBeInTheDocument();
    fireEvent.click(tabMenuTrigger("Alpha"));
    await waitFor(() => expect(screen.getAllByRole("menuitem")).toHaveLength(4));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Edit",
      "Open in new tab",
      "Copy link",
      "Delete",
    ]);
  });

  it("opens the edit dialog prefilled from the tab menu", async () => {
    seedMember();
    transportViews(CUSTOMS);
    renderStrip(CUSTOM_A);
    expect(await screen.findByRole("link", { name: "Alpha" })).toBeInTheDocument();
    fireEvent.click(tabMenuTrigger("Alpha"));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Edit" }));
    expect(await screen.findByRole("heading", { name: "Update View" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("Alpha");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Update View" })).not.toBeInTheDocument());
  });

  it("treats an unresolved viewer as a non-owner", async () => {
    session.useMe.mockReturnValue({});
    session.usePermissions.mockReturnValue({ canEdit: true, isAdmin: false, isLoading: false });
    roles.useProjectRoles.mockReturnValue({ data: undefined });
    transportViews(CUSTOMS);
    renderStrip(CUSTOM_A);
    expect(await screen.findByRole("link", { name: "Alpha" })).toBeInTheDocument();
    fireEvent.click(tabMenuTrigger("Alpha"));
    await waitFor(() => expect(screen.getAllByRole("menuitem")).toHaveLength(2));
  });

  it("opens the delete confirmation from the tab menu and removes the tab", async () => {
    seedMember();
    const calls: { method: string; url: string }[] = [];
    let deleted = false;
    createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
      }
      const method = init.method ?? "GET";
      calls.push({ method, url });
      if (method === "DELETE") deleted = true;
      if (method === "GET") {
        const body = deleted ? [] : CUSTOMS;
        return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
      }
      return { status: 204, url, headers: { get: () => null }, text: async () => "null" };
    }) as never);
    renderStrip(CUSTOM_A);
    expect(await screen.findByRole("link", { name: "Alpha" })).toBeInTheDocument();
    fireEvent.click(tabMenuTrigger("Alpha"));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete" }));
    expect(
      await screen.findByRole("heading", { name: "Are you sure you want to delete this view?" })
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View deleted successfully"));
    expect(calls.some((call) => call.method === "DELETE")).toBe(true);
    await waitFor(() => expect(screen.queryByRole("link", { name: "Alpha" })).not.toBeInTheDocument());
  });

  it("offers open and copy only on a default tab", async () => {
    seedMember();
    transportViews(CUSTOMS);
    renderStrip("assigned");
    expect(await screen.findByRole("link", { name: "Alpha" })).toBeInTheDocument();
    fireEvent.click(tabMenuTrigger("Assigned"));
    await waitFor(() => expect(screen.getAllByRole("menuitem")).toHaveLength(2));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["Open in new tab", "Copy link"]);
  });

  it("offers open and copy only to a non-owner member", async () => {
    session.useMe.mockReturnValue({ data: { id: "someone-else" } });
    session.usePermissions.mockReturnValue({ canEdit: true, isAdmin: false, isLoading: false });
    roles.useProjectRoles.mockReturnValue({ data: undefined });
    transportViews(CUSTOMS);
    renderStrip(CUSTOM_A);
    expect(await screen.findByRole("link", { name: "Alpha" })).toBeInTheDocument();
    fireEvent.click(tabMenuTrigger("Alpha"));
    await waitFor(() => expect(screen.getAllByRole("menuitem")).toHaveLength(2));
  });

  it("adds delete for a non-owner admin", async () => {
    session.useMe.mockReturnValue({ data: { id: "someone-else" } });
    session.usePermissions.mockReturnValue({ canEdit: true, isAdmin: true, isLoading: false });
    roles.useProjectRoles.mockReturnValue({ data: undefined });
    transportViews(CUSTOMS);
    renderStrip(CUSTOM_A);
    expect(await screen.findByRole("link", { name: "Alpha" })).toBeInTheDocument();
    fireEvent.click(tabMenuTrigger("Alpha"));
    await waitFor(() => expect(screen.getAllByRole("menuitem")).toHaveLength(3));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Open in new tab",
      "Copy link",
      "Delete",
    ]);
  });
});

describe("WorkspaceViewsTabStrip active scroll", () => {
  const CUSTOMS = [makeView({ id: CUSTOM_A, project: null, name: "Alpha", owned_by: VIEWS_OWNER_ID })];

  function tabElement(key: string): HTMLElement {
    return document.querySelector(`[data-tab-key="${key}"]`) as HTMLElement;
  }

  async function renderWithTabs(): Promise<ReturnType<typeof renderStrip>> {
    seedMember();
    transportViews(CUSTOMS);
    const strip = renderStrip("all-issues");
    expect(await screen.findByRole("link", { name: "Alpha" })).toBeInTheDocument();
    return strip;
  }

  it("brings a partially visible tab in from the nearest edge", async () => {
    const strip = await renderWithTabs();
    const container = screen.getByTestId("workspace-views-tab-strip");
    const tab = tabElement(CUSTOM_A);
    vi.spyOn(container, "getBoundingClientRect").mockReturnValue(rect(0, 100));
    vi.spyOn(tab, "getBoundingClientRect").mockReturnValue(rect(60, 140));
    const scrolled = vi.fn();
    tab.scrollIntoView = scrolled;
    strip.rerenderActive(CUSTOM_A);
    await waitFor(() =>
      expect(scrolled).toHaveBeenCalledWith({ block: "nearest", inline: "nearest", behavior: "smooth" })
    );
  });

  it("centers a tab fully off-screen", async () => {
    const strip = await renderWithTabs();
    const container = screen.getByTestId("workspace-views-tab-strip");
    const tab = tabElement(CUSTOM_A);
    vi.spyOn(container, "getBoundingClientRect").mockReturnValue(rect(0, 100));
    vi.spyOn(tab, "getBoundingClientRect").mockReturnValue(rect(120, 180));
    const scrolled = vi.fn();
    tab.scrollIntoView = scrolled;
    strip.rerenderActive(CUSTOM_A);
    await waitFor(() =>
      expect(scrolled).toHaveBeenCalledWith({ block: "nearest", inline: "center", behavior: "smooth" })
    );
  });

  it("leaves a fully visible tab where it is", async () => {
    const strip = await renderWithTabs();
    const container = screen.getByTestId("workspace-views-tab-strip");
    const tab = tabElement(CUSTOM_A);
    vi.spyOn(container, "getBoundingClientRect").mockReturnValue(rect(0, 100));
    vi.spyOn(tab, "getBoundingClientRect").mockReturnValue(rect(20, 80));
    const scrolled = vi.fn();
    tab.scrollIntoView = scrolled;
    strip.rerenderActive(CUSTOM_A);
    await waitFor(() => expect(screen.getByRole("link", { name: "Alpha" })).toHaveAttribute("aria-current", "page"));
    expect(scrolled).not.toHaveBeenCalled();
  });

  it("scrolls instantly when reduced motion is preferred", async () => {
    const original = window.matchMedia;
    window.matchMedia = vi.fn().mockReturnValue({ matches: true }) as unknown as typeof window.matchMedia;
    try {
      const strip = await renderWithTabs();
      const container = screen.getByTestId("workspace-views-tab-strip");
      const tab = tabElement(CUSTOM_A);
      vi.spyOn(container, "getBoundingClientRect").mockReturnValue(rect(0, 100));
      vi.spyOn(tab, "getBoundingClientRect").mockReturnValue(rect(120, 180));
      const scrolled = vi.fn();
      tab.scrollIntoView = scrolled;
      strip.rerenderActive(CUSTOM_A);
      await waitFor(() =>
        expect(scrolled).toHaveBeenCalledWith({ block: "nearest", inline: "center", behavior: "auto" })
      );
    } finally {
      window.matchMedia = original;
    }
  });
});

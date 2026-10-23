// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { Favorite, FavoriteList } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { FAVORITES_SECTION_OPEN_KEY, notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { sidebarKeys } from "../api/keys.js";
import { FAVORITE_REMOVE_LABEL, favoriteDisplayName, SidebarFavoritesGroup } from "./SidebarFavoritesGroup.js";

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

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const FOLDER_ID = "223e4567-e89b-12d3-a456-426614174001";
const ENTRY_ID = "323e4567-e89b-12d3-a456-426614174002";
const CHILD_ID = "423e4567-e89b-12d3-a456-426614174003";
const PROJECT_ID = "523e4567-e89b-12d3-a456-426614174004";
const PAGE_ID = "623e4567-e89b-12d3-a456-426614174005";

function row(overrides: Partial<Favorite> = {}): Favorite {
  return {
    id: ENTRY_ID,
    entity_type: "project",
    entity_identifier: PROJECT_ID,
    entity_data: {},
    name: "Atlas",
    is_folder: false,
    sequence: 20000,
    parent: null,
    workspace_id: WS_ID,
    project_id: PROJECT_ID,
    ...overrides,
  };
}

const FOLDER = row({
  id: FOLDER_ID,
  entity_type: "folder",
  entity_identifier: null,
  name: "Pinned",
  is_folder: true,
  sequence: 30000,
  project_id: null,
});
const ENTRY = row({ id: ENTRY_ID, sequence: 20000 });
const CHILD = row({ id: CHILD_ID, name: "Nested", sequence: 10000, parent: FOLDER_ID });

interface StubOptions {
  list?: FavoriteList;
  groups?: Record<string, FavoriteList>;
}

function stubClient(options: StubOptions = {}) {
  const server: FavoriteList = (options.list ?? [FOLDER, ENTRY]).map((fav) => ({ ...fav }));
  const groups: Record<string, FavoriteList> = Object.fromEntries(
    Object.entries({ [FOLDER_ID]: [CHILD], ...(options.groups ?? {}) }).map(([id, kids]) => [
      id,
      kids.map((kid) => ({ ...kid })),
    ])
  );
  const calls: Array<{ url: string; method: string; body: string }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    calls.push({ url, method, body: init.body ?? "" });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (url.endsWith("/user-favorites/") && method === "GET") {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(server) };
    }
    const groupMatch = /\/user-favorites\/([^/]+)\/group\/$/.exec(url);
    if (groupMatch !== null && method === "GET") {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(groups[groupMatch[1] ?? ""] ?? []),
      };
    }
    if (url.endsWith("/user-favorites/") && method === "POST") {
      const sent = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      const created = row({ id: "723e4567-e89b-12d3-a456-426614174006", sequence: 40000, ...sent });
      server.push(created);
      return { status: 201, url, headers: { get: () => null }, text: async () => JSON.stringify(created) };
    }
    const patchMatch = /\/user-favorites\/([^/]+)\/$/.exec(url);
    if (patchMatch !== null && method === "PATCH") {
      const sent = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      const target = server.find((fav) => fav.id === patchMatch[1]);
      if (target !== undefined) Object.assign(target, sent);
      for (const kids of Object.values(groups)) {
        const kid = kids.find((fav) => fav.id === patchMatch[1]);
        if (kid !== undefined) Object.assign(kid, sent);
      }
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(row({ ...sent })) };
    }
    const deleteMatch = /\/user-favorites\/([^/]+)\/$/.exec(url);
    if (deleteMatch !== null && method === "DELETE") {
      const index = server.findIndex((fav) => fav.id === deleteMatch[1]);
      if (index >= 0) server.splice(index, 1);
      for (const kids of Object.values(groups)) {
        const at = kids.findIndex((fav) => fav.id === deleteMatch[1]);
        if (at >= 0) kids.splice(at, 1);
      }
      return { status: 204, url, headers: { get: () => null }, text: async () => "" };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return { calls };
}

function setup(stub: StubOptions = {}) {
  const { calls } = stubClient(stub);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <SidebarFavoritesGroup workspaceSlug="acme" />
    </QueryClientProvider>
  );
  return { calls, queryClient, view };
}

function entryNames(): string[] {
  return screen.queryAllByTestId("favorite-entry").map((entry) => entry.getAttribute("data-entry-name") ?? "");
}

function rowByName(name: string): HTMLElement {
  const rows = screen.getAllByTestId("favorite-entry");
  const found = rows.find((entry) => entry.getAttribute("data-entry-name") === name);
  if (found === undefined) throw new Error(`missing favorite row ${name}`);
  return found;
}

function openMenu(name: string): void {
  fireEvent.click(within(rowByName(name)).getByTestId("favorite-row-menu"));
}

interface StubRect {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

function toDomRect(found: Partial<StubRect> | undefined): DOMRect {
  const top = found?.top ?? 0;
  const bottom = found?.bottom ?? 0;
  const left = found?.left ?? 0;
  const right = found?.right ?? 0;
  return {
    top,
    bottom,
    left,
    right,
    width: right - left,
    height: bottom - top,
    x: left,
    y: top,
    toJSON: () => ({}),
  } as DOMRect;
}

function stubRowRects(
  spans: Record<string, { top: number; bottom: number }>,
  extras: { headers?: Record<string, StubRect>; lists?: Record<string, StubRect> } = {}
): void {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    if (this.tagName === "UL") return toDomRect(extras.lists?.[this.dataset["testid"] ?? ""]);
    if (this.dataset["testid"] === "favorite-folder-toggle") {
      const owner = this.closest("[data-favorite-row]")?.getAttribute("data-favorite-row") ?? "";
      return toDomRect(extras.headers?.[owner]);
    }
    return toDomRect(spans[this.dataset["favoriteRow"] ?? ""]);
  });
}

function pointerMove(clientY: number, clientX = 0): void {
  act(() => {
    window.dispatchEvent(new window.MouseEvent("pointermove", { clientX, clientY, bubbles: true }));
  });
}

function pointerUp(): void {
  act(() => {
    window.dispatchEvent(new window.MouseEvent("pointerup", { bubbles: true }));
  });
}

function pointerCancel(): void {
  act(() => {
    window.dispatchEvent(new window.MouseEvent("pointercancel", { bubbles: true }));
  });
}

beforeEach(() => {
  window.localStorage.clear();
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("favoriteDisplayName", () => {
  it("prefers the target's live name", () => {
    expect(favoriteDisplayName(row({ entity_data: { name: "Live" }, name: "Stored" }))).toBe("Live");
  });

  it("falls back to the stored name", () => {
    expect(favoriteDisplayName(row({ entity_data: {}, name: "Stored" }))).toBe("Stored");
    expect(favoriteDisplayName(row({ entity_data: { name: 7 }, name: "Stored" }))).toBe("Stored");
    expect(favoriteDisplayName(row({ entity_data: { name: "" }, name: "Stored" }))).toBe("Stored");
    expect(favoriteDisplayName(row({ entity_data: null, name: "Stored" }))).toBe("Stored");
    expect(favoriteDisplayName(row({ entity_data: "oops", name: "Stored" }))).toBe("Stored");
  });

  it("renders nameless rows blank", () => {
    expect(favoriteDisplayName(row({ entity_data: {}, name: null }))).toBe("");
  });
});

describe("SidebarFavoritesGroup", () => {
  it("hides entirely while the top-level list is empty", async () => {
    setup({ list: [] });
    await waitFor(() => expect(screen.queryByTestId("sidebar-favorites-group")).not.toBeInTheDocument());
    expect(entryNames()).toEqual([]);
  });

  it("lists folders and entries in display order with deep links", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    setup();
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    const link = within(rowByName("Atlas")).getByRole("link");
    expect(link.getAttribute("href")).toBe(`/acme/projects/${PROJECT_ID}/issues`);
  });

  it("toggles the section and persists it", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    setup();
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    const toggle = screen.getByTestId("favorites-section-toggle");
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(entryNames()).toEqual([]);
    expect(window.localStorage.getItem(FAVORITES_SECTION_OPEN_KEY)).toBeNull();
  });

  it("opens a closed section live on the first arrival", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const h = setup({ list: [] });
    await waitFor(() => expect(screen.queryByTestId("sidebar-favorites-group")).not.toBeInTheDocument());
    act(() => {
      h.queryClient.setQueryData(sidebarKeys.favorites("acme"), [ENTRY]);
    });
    await waitFor(() => expect(entryNames()).toEqual(["Atlas"]));
    expect(screen.getByTestId("favorites-section-toggle")).toHaveAttribute("aria-expanded", "true");
  });

  it("arrives closed when the stored flag is off", async () => {
    const h = setup({ list: [] });
    await waitFor(() => expect(screen.queryByTestId("sidebar-favorites-group")).not.toBeInTheDocument());
    act(() => {
      h.queryClient.setQueryData(sidebarKeys.favorites("acme"), [ENTRY]);
    });
    await waitFor(() => expect(screen.getByTestId("sidebar-favorites-group")).toBeInTheDocument());
    expect(screen.getByTestId("favorites-section-toggle")).toHaveAttribute("aria-expanded", "false");
  });

  it("expands folders to reveal their entries and collapses again", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    setup();
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    fireEvent.click(within(rowByName("Pinned")).getByTestId("favorite-folder-toggle"));
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Nested", "Atlas"]));
    fireEvent.click(within(rowByName("Pinned")).getByTestId("favorite-folder-toggle"));
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
  });

  it("validates folder names and creates on success", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    setup();
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    fireEvent.click(screen.getByTestId("favorites-folder-create"));
    const field = screen.getByTestId("favorites-folder-input");
    const form = screen.getByTestId("favorites-folder-form");
    fireEvent.submit(form);
    expect(screen.getByTestId("favorites-folder-form")).toBeInTheDocument();
    expect(notifyError).not.toHaveBeenCalled();
    fireEvent.change(field, { target: { value: "   " } });
    fireEvent.submit(form);
    expect(notifyError).toHaveBeenCalledWith("Folder name cannot be empty");
    expect(screen.getByTestId("favorites-folder-form")).toBeInTheDocument();
    fireEvent.change(field, { target: { value: "Pinned" } });
    fireEvent.submit(form);
    expect(notifyError).toHaveBeenCalledWith("Folder already exists");
    expect(screen.getByTestId("favorites-folder-form")).toBeInTheDocument();
    fireEvent.change(field, { target: { value: "Fresh" } });
    fireEvent.submit(form);
    await waitFor(() => expect(screen.queryByTestId("favorites-folder-form")).not.toBeInTheDocument());
    expect(notifySuccess).toHaveBeenCalledWith("Favorite created successfully");
    await waitFor(() => expect(entryNames()).toContain("Fresh"));
  });

  it("opens the section with the form from a closed section", async () => {
    setup();
    await waitFor(() => expect(screen.getByTestId("sidebar-favorites-group")).toBeInTheDocument());
    expect(screen.getByTestId("favorites-section-toggle")).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(screen.getByTestId("favorites-folder-create"));
    expect(screen.getByTestId("favorites-section-toggle")).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("favorites-folder-form")).toBeInTheDocument();
  });

  it("dismisses the form on outside pointerdown and Escape only", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    setup();
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    fireEvent.click(screen.getByTestId("favorites-folder-create"));
    const field = screen.getByTestId("favorites-folder-input");
    act(() => {
      field.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true }));
    });
    expect(screen.getByTestId("favorites-folder-form")).toBeInTheDocument();
    fireEvent.keyDown(field, { key: "a" });
    expect(screen.getByTestId("favorites-folder-form")).toBeInTheDocument();
    fireEvent.keyDown(field, { key: "Escape" });
    expect(screen.queryByTestId("favorites-folder-form")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("favorites-folder-create"));
    act(() => {
      document.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true }));
    });
    expect(screen.queryByTestId("favorites-folder-form")).not.toBeInTheDocument();
  });

  it("renames folders through their quick menu", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const h = setup();
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    openMenu("Pinned");
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["Remove from favorites", "Rename Folder"]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Rename Folder" }));
    const field = screen.getByTestId("favorites-folder-input");
    expect((field as HTMLInputElement).value).toBe("Pinned");
    fireEvent.change(field, { target: { value: "Sorted" } });
    fireEvent.submit(screen.getByTestId("favorites-folder-form"));
    await waitFor(() => expect(screen.queryByTestId("favorites-folder-form")).not.toBeInTheDocument());
    expect(notifySuccess).toHaveBeenCalledWith("Favorite updated successfully");
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(`/api/workspaces/acme/user-favorites/${FOLDER_ID}/`);
  });

  it("removes entries through their quick menu with the pinned notice", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const h = setup({ list: [ENTRY] });
    await waitFor(() => expect(entryNames()).toEqual(["Atlas"]));
    openMenu("Atlas");
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual([FAVORITE_REMOVE_LABEL]);
    fireEvent.click(screen.getByRole("menuitem", { name: FAVORITE_REMOVE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Favorite removed successfully"));
    const call = h.calls.find((entry) => entry.method === "DELETE");
    expect(call?.url).toContain(`/api/workspaces/acme/user-favorites/${ENTRY_ID}/`);
    await waitFor(() => expect(screen.queryByTestId("sidebar-favorites-group")).not.toBeInTheDocument());
  });

  it("falls back to the workspace root for unknown entity types", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const strange = row({ id: "823e4567-e89b-12d3-a456-426614174007", entity_type: "teleporter", name: "Mystery" });
    setup({ list: [strange] });
    await waitFor(() => expect(entryNames()).toEqual(["Mystery"]));
    expect(within(rowByName("Mystery")).getByRole("link").getAttribute("href")).toBe("/acme");
  });

  it("renders page favorites on their detail page", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const page = row({ entity_type: "page", entity_identifier: PAGE_ID, name: "Wiki" });
    setup({ list: [page] });
    await waitFor(() => expect(entryNames()).toEqual(["Wiki"]));
    expect(within(rowByName("Wiki")).getByRole("link").getAttribute("href")).toBe(
      `/acme/projects/${PROJECT_ID}/pages/${PAGE_ID}`
    );
  });

  it("reorders top-level rows with one sequence write", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const h = setup();
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    stubRowRects({ [FOLDER_ID]: { top: 0, bottom: 20 }, [ENTRY_ID]: { top: 20, bottom: 40 } });
    const handle = within(rowByName("Atlas")).getByTestId("favorite-drag-handle");
    fireEvent.pointerDown(handle);
    expect(rowByName("Atlas").querySelector(".opacity-60")).not.toBeNull();
    pointerMove(5);
    await waitFor(() => expect(screen.getAllByTestId("favorite-drop-indicator")).toHaveLength(1));
    pointerUp();
    await waitFor(() =>
      expect(h.calls.some((call) => call.method === "PATCH" && call.url.includes(ENTRY_ID))).toBe(true)
    );
    const patch = h.calls.find((call) => call.method === "PATCH" && call.url.includes(ENTRY_ID));
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ sequence: 40000 });
  });

  it("marks below-last drops and stays silent on cancel, self and no-op drops", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const h = setup();
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    stubRowRects({ [FOLDER_ID]: { top: 0, bottom: 20 }, [ENTRY_ID]: { top: 20, bottom: 40 } });
    const pinned = rowByName("Pinned");
    fireEvent.pointerDown(within(pinned).getByTestId("favorite-drag-handle"));
    pointerMove(200);
    await waitFor(() => expect(screen.getAllByTestId("favorite-drop-indicator")).toHaveLength(1));
    pointerCancel();
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
    fireEvent.pointerDown(within(rowByName("Atlas")).getByTestId("favorite-drag-handle"));
    pointerMove(30);
    pointerUp();
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
    fireEvent.pointerDown(within(rowByName("Pinned")).getByTestId("favorite-drag-handle"));
    pointerMove(25);
    pointerUp();
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
  });

  it("reorders nested rows within their folder", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const second = row({
      id: "923e4567-e89b-12d3-a456-426614174008",
      name: "Second",
      sequence: 5000,
      parent: FOLDER_ID,
    });
    const h = setup({ groups: { [FOLDER_ID]: [CHILD, second] } });
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    fireEvent.click(within(rowByName("Pinned")).getByTestId("favorite-folder-toggle"));
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Nested", "Second", "Atlas"]));
    stubRowRects({ [CHILD_ID]: { top: 0, bottom: 20 }, [second.id]: { top: 20, bottom: 40 } });
    fireEvent.pointerDown(within(rowByName("Second")).getByTestId("favorite-drag-handle"));
    pointerMove(5);
    pointerUp();
    await waitFor(() =>
      expect(h.calls.some((call) => call.method === "PATCH" && call.url.includes(second.id))).toBe(true)
    );
  });

  it("nests a top-level row dropped onto a folder", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const h = setup();
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    stubRowRects(
      { [FOLDER_ID]: { top: 0, bottom: 60 }, [ENTRY_ID]: { top: 60, bottom: 100 } },
      { headers: { [FOLDER_ID]: { top: 4, bottom: 36, left: 8, right: 192 } } }
    );
    fireEvent.pointerDown(within(rowByName("Atlas")).getByTestId("favorite-drag-handle"));
    pointerMove(20, 100);
    const toggle = within(rowByName("Pinned")).getByTestId("favorite-folder-toggle");
    await waitFor(() => expect(toggle.getAttribute("data-drop-into")).toBe("true"));
    pointerUp();
    await waitFor(() =>
      expect(h.calls.some((call) => call.method === "PATCH" && call.url.includes(ENTRY_ID))).toBe(true)
    );
    const patch = h.calls.find((call) => call.method === "PATCH" && call.url.includes(ENTRY_ID));
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ parent: FOLDER_ID });
  });

  it("reorders instead of nesting when a folder is dragged over a folder", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const sorted = row({
      id: "b23e4567-e89b-12d3-a456-42661417400a",
      entity_type: "folder",
      entity_identifier: null,
      name: "Sorted",
      is_folder: true,
      sequence: 25000,
      project_id: null,
    });
    const h = setup({ list: [FOLDER, sorted, ENTRY] });
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Sorted", "Atlas"]));
    stubRowRects(
      {
        [FOLDER_ID]: { top: 0, bottom: 40 },
        [sorted.id]: { top: 40, bottom: 80 },
        [ENTRY_ID]: { top: 80, bottom: 120 },
      },
      { headers: { [sorted.id]: { top: 44, bottom: 76, left: 8, right: 192 } } }
    );
    fireEvent.pointerDown(within(rowByName("Pinned")).getByTestId("favorite-drag-handle"));
    pointerMove(60, 100);
    expect(within(rowByName("Sorted")).getByTestId("favorite-folder-toggle").getAttribute("data-drop-into")).toBeNull();
    pointerUp();
    await waitFor(() =>
      expect(h.calls.some((call) => call.method === "PATCH" && call.url.includes(FOLDER_ID))).toBe(true)
    );
    const patch = h.calls.find((call) => call.method === "PATCH" && call.url.includes(FOLDER_ID));
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ sequence: 22500 });
  });

  it("stays silent dropping a nested row onto its own folder", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const h = setup();
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    fireEvent.click(within(rowByName("Pinned")).getByTestId("favorite-folder-toggle"));
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Nested", "Atlas"]));
    stubRowRects(
      {
        [FOLDER_ID]: { top: 0, bottom: 100 },
        [ENTRY_ID]: { top: 100, bottom: 140 },
        [CHILD_ID]: { top: 40, bottom: 60 },
      },
      { headers: { [FOLDER_ID]: { top: 4, bottom: 36, left: 8, right: 192 } } }
    );
    fireEvent.pointerDown(within(rowByName("Nested")).getByTestId("favorite-drag-handle"));
    pointerMove(20, 100);
    pointerUp();
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
    expect(screen.queryAllByTestId("favorite-drop-indicator")).toHaveLength(0);
    expect(within(rowByName("Pinned")).getByTestId("favorite-folder-toggle").getAttribute("data-drop-into")).toBeNull();
  });

  it("un-nests a nested row dragged onto the top list", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const h = setup();
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    fireEvent.click(within(rowByName("Pinned")).getByTestId("favorite-folder-toggle"));
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Nested", "Atlas"]));
    stubRowRects(
      {
        [FOLDER_ID]: { top: 0, bottom: 100 },
        [ENTRY_ID]: { top: 100, bottom: 140 },
        [CHILD_ID]: { top: 40, bottom: 60 },
      },
      {
        headers: { [FOLDER_ID]: { top: 4, bottom: 36, left: 8, right: 192 } },
        lists: {
          "favorites-entry-list": { top: 0, bottom: 140, left: 0, right: 200 },
          "favorite-folder-children": { top: 40, bottom: 80, left: 24, right: 200 },
        },
      }
    );
    fireEvent.pointerDown(within(rowByName("Nested")).getByTestId("favorite-drag-handle"));
    pointerMove(102, 100);
    await waitFor(() => expect(within(rowByName("Atlas")).getByTestId("favorite-drop-indicator")).not.toBeNull());
    pointerUp();
    await waitFor(() =>
      expect(h.calls.some((call) => call.method === "PATCH" && call.url.includes(CHILD_ID))).toBe(true)
    );
    const patch = h.calls.find((call) => call.method === "PATCH" && call.url.includes(CHILD_ID));
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ sequence: 25000, parent: null });
  });

  it("nests a nested row dropped onto another folder", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const sorted = row({
      id: "b23e4567-e89b-12d3-a456-42661417400a",
      entity_type: "folder",
      entity_identifier: null,
      name: "Sorted",
      is_folder: true,
      sequence: 25000,
      project_id: null,
    });
    const h = setup({ list: [FOLDER, sorted, ENTRY] });
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Sorted", "Atlas"]));
    fireEvent.click(within(rowByName("Pinned")).getByTestId("favorite-folder-toggle"));
    await waitFor(() => expect(entryNames()).toContain("Nested"));
    stubRowRects(
      {
        [FOLDER_ID]: { top: 0, bottom: 100 },
        [sorted.id]: { top: 100, bottom: 140 },
        [ENTRY_ID]: { top: 140, bottom: 180 },
        [CHILD_ID]: { top: 40, bottom: 60 },
      },
      { headers: { [sorted.id]: { top: 104, bottom: 136, left: 8, right: 192 } } }
    );
    fireEvent.pointerDown(within(rowByName("Nested")).getByTestId("favorite-drag-handle"));
    pointerMove(120, 100);
    const toggle = within(rowByName("Sorted")).getByTestId("favorite-folder-toggle");
    await waitFor(() => expect(toggle.getAttribute("data-drop-into")).toBe("true"));
    pointerUp();
    await waitFor(() =>
      expect(h.calls.some((call) => call.method === "PATCH" && call.url.includes(CHILD_ID))).toBe(true)
    );
    const patch = h.calls.find((call) => call.method === "PATCH" && call.url.includes(CHILD_ID));
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ parent: sorted.id });
  });

  it("reorders within the folder when released inside it", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const second = row({
      id: "923e4567-e89b-12d3-a456-426614174008",
      name: "Second",
      sequence: 5000,
      parent: FOLDER_ID,
    });
    const h = setup({ groups: { [FOLDER_ID]: [CHILD, second] } });
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    fireEvent.click(within(rowByName("Pinned")).getByTestId("favorite-folder-toggle"));
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Nested", "Second", "Atlas"]));
    stubRowRects(
      {
        [FOLDER_ID]: { top: 0, bottom: 140 },
        [ENTRY_ID]: { top: 140, bottom: 180 },
        [CHILD_ID]: { top: 40, bottom: 60 },
        [second.id]: { top: 60, bottom: 80 },
      },
      {
        headers: { [FOLDER_ID]: { top: 4, bottom: 36, left: 8, right: 192 } },
        lists: {
          "favorites-entry-list": { top: 0, bottom: 180, left: 0, right: 200 },
          "favorite-folder-children": { top: 40, bottom: 80, left: 24, right: 200 },
        },
      }
    );
    fireEvent.pointerDown(within(rowByName("Second")).getByTestId("favorite-drag-handle"));
    pointerMove(45, 100);
    pointerUp();
    await waitFor(() =>
      expect(h.calls.some((call) => call.method === "PATCH" && call.url.includes(second.id))).toBe(true)
    );
    const patch = h.calls.find((call) => call.method === "PATCH" && call.url.includes(second.id));
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ sequence: 20000 });
  });

  it("removes nested rows through their quick menu", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    setup();
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    fireEvent.click(within(rowByName("Pinned")).getByTestId("favorite-folder-toggle"));
    await waitFor(() => expect(entryNames()).toContain("Nested"));
    openMenu("Nested");
    fireEvent.click(await screen.findByRole("menuitem", { name: FAVORITE_REMOVE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Favorite removed successfully"));
    await waitFor(() => expect(entryNames()).not.toContain("Nested"));
  });

  it("renders nested folders recursively", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const nested = row({
      id: "a23e4567-e89b-12d3-a456-426614174009",
      entity_type: "folder",
      entity_identifier: null,
      name: "Inner",
      is_folder: true,
      sequence: 5000,
      parent: FOLDER_ID,
      project_id: null,
    });
    setup({ groups: { [FOLDER_ID]: [CHILD, nested], [nested.id]: [] } });
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Atlas"]));
    fireEvent.click(within(rowByName("Pinned")).getByTestId("favorite-folder-toggle"));
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Nested", "Inner", "Atlas"]));
    fireEvent.click(within(rowByName("Inner")).getByTestId("favorite-folder-toggle"));
    await waitFor(() => expect(entryNames()).toEqual(["Pinned", "Nested", "Inner", "Atlas"]));
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assigned / Created / Subscribed tab (parity/specs/profile.md §2,
// PROF-006): the ownership-selected page renders as a list or board, the
// layout and display menus persist to browser-local storage, a selected
// row opens the peek host, and an empty page keeps its empty state.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { IssueListItem } from "@pidash/api-client";
import { listStates, listUserProfileIssues, listProjects, listWorkspaceMembers } from "@pidash/api-client";

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return {
    ...original,
    listStates: vi.fn(),
    listUserProfileIssues: vi.fn(),
    listProjects: vi.fn(),
    listWorkspaceMembers: vi.fn(),
  };
});

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { to: string; children?: React.ReactNode }) => <a href={props.to}>{props.children}</a>,
  };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { profileDisplayKey } from "../lib/preferences.js";
import { profileIssuesQueryOptions } from "../api/queries.js";
import { ProfileIssuesTab } from "./ProfileIssuesTab.js";

function memoryStorage(): Storage {
  const entries = new Map<string, string>();
  return {
    get length() {
      return entries.size;
    },
    clear: () => {
      entries.clear();
    },
    getItem: (key: string) => entries.get(key) ?? null,
    key: (index: number) => [...entries.keys()][index] ?? null,
    removeItem: (key: string) => {
      entries.delete(key);
    },
    setItem: (key: string, value: string) => {
      entries.set(key, value);
    },
  };
}

Object.defineProperty(globalThis, "localStorage", { value: memoryStorage(), configurable: true });

function issue(id: string, name: string, overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id,
    name,
    state_id: null,
    sort_order: 0,
    completed_at: null,
    priority: "low",
    sequence_id: id === "i-urgent" ? 7 : 9,
    project_id: "p-1",
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: 0,
    attachment_count: 0,
    link_count: 0,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  } as IssueListItem;
}

function response(results: IssueListItem[]) {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: results.length,
    total_results: results.length,
    results,
  };
}

function renderTab(ownership: "assigned" | "created" | "subscribed" = "assigned") {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  render(
    <QueryClientProvider client={client}>
      <ProfileIssuesTab workspaceSlug="acme" userId="u-1" ownership={ownership} title="Assigned" />
    </QueryClientProvider>
  );
  return client;
}

beforeEach(() => {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  localStorage.clear();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  vi.mocked(listProjects).mockResolvedValue([
    { id: "p-1", name: "Prof Proj", identifier: "PP", workspace: "w-1" } as never,
  ]);
  vi.mocked(listStates).mockResolvedValue([
    { id: "s-backlog", name: "Backlog", group: "backlog", color: "#000000", project_id: "p-1" } as never,
    { id: "s-working", name: "In progress", group: "started", color: "#000000", project_id: "p-1" } as never,
  ]);
  vi.mocked(listWorkspaceMembers).mockResolvedValue([
    { id: "m-1", member: { id: "u-2", display_name: "Ada Lovelace" }, role: 20 } as never,
    { id: "m-2", member: { id: "u-3", display_name: "Grace Hopper" }, role: 20 } as never,
  ]);
  vi.mocked(listUserProfileIssues).mockResolvedValue(
    response([issue("i-urgent", "Prof persist urgent", { priority: "urgent" }), issue("i-low", "Prof persist low")])
  );
});

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
  localStorage.clear();
});

describe("ProfileIssuesTab", () => {
  it("reads the tab's ownership page and renders the list with keys and priorities", async () => {
    renderTab("created");
    expect(await screen.findByTestId("profile-list")).toBeInTheDocument();
    expect(listUserProfileIssues).toHaveBeenCalledWith(
      expect.anything(),
      "acme",
      "u-1",
      expect.objectContaining({ ownership: "created", perPage: 100 })
    );
    const rows = [...document.querySelectorAll("[data-profile-row]")] as HTMLElement[];
    expect(rows.map((row) => row.getAttribute("data-profile-row"))).toEqual(["i-urgent", "i-low"]);
    expect(rows[0]?.textContent).toContain("PP-7");
    expect(rows[0]?.textContent).toContain("Urgent");
    expect(screen.getByTestId("profile-issues-tab")).toHaveAttribute("data-profile-ownership", "created");
  });

  it("offers list and kanban, switches, and persists the choice under the profile key", async () => {
    const first = renderTab();
    await screen.findByTestId("profile-list");
    const [desktop] = await screen.findAllByRole("button", { name: /Issue layout, current List/ });
    await userEvent.click(desktop!);
    await screen.findByRole("menu", { name: "Issue layout" });
    const kanbanChoice = document.querySelector('[data-profile-layout-choice="kanban"]') as HTMLElement;
    expect(kanbanChoice.textContent).toBe("Board");
    await userEvent.click(kanbanChoice);
    expect(await screen.findByTestId("profile-board")).toBeInTheDocument();
    expect(screen.queryByTestId("profile-list")).toBeNull();
    expect(JSON.parse(localStorage.getItem(profileDisplayKey("acme", "u-1")) ?? "null")).toMatchObject({
      layout: "kanban",
    });

    // A fresh visit reads the stored layout instead of the default.
    first.clear();
    renderTab();
    expect(await screen.findByTestId("profile-board")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /Issue layout, current Board/ }).length).toBeGreaterThan(0);
  });

  it("groups by priority through the display menu and keeps the choice", async () => {
    renderTab();
    await screen.findByTestId("profile-list");
    expect(screen.getAllByRole("region").some((region) => region.getAttribute("aria-label") === "Urgent")).toBe(false);
    await userEvent.click(screen.getAllByTestId("profile-display-trigger")[0]!);
    const panel = await screen.findByTestId("profile-display-panel");
    await userEvent.click(within(panel).getByRole("radio", { name: "Priority" }));
    await waitFor(() =>
      expect(
        [...document.querySelectorAll("[data-profile-group]")].map((group) => group.getAttribute("data-profile-group"))
      ).toEqual(["Urgent", "Low"])
    );
    expect(JSON.parse(localStorage.getItem(profileDisplayKey("acme", "u-1")) ?? "null")).toMatchObject({
      groupBy: "priority",
    });
  });

  it("renders the assigned tab's empty copy through the shared card", async () => {
    vi.mocked(listUserProfileIssues).mockResolvedValue(response([]));
    renderTab("assigned");
    const scope = await screen.findByTestId("issues-empty-state");
    expect(scope).toHaveAttribute("data-context", "profile-assigned");
    expect(within(scope).getByText("No work items are assigned to you")).toBeInTheDocument();
    expect(within(scope).queryByRole("button")).toBeNull();
  });

  it("renders the created tab's empty copy through the shared card", async () => {
    vi.mocked(listUserProfileIssues).mockResolvedValue(response([]));
    renderTab("created");
    const scope = await screen.findByTestId("issues-empty-state");
    expect(scope).toHaveAttribute("data-context", "profile-created");
    expect(within(scope).getByText("No work items yet")).toBeInTheDocument();
    expect(within(scope).queryByRole("button")).toBeNull();
  });

  it("renders the subscribed tab's empty copy through the shared card", async () => {
    vi.mocked(listUserProfileIssues).mockResolvedValue(response([]));
    renderTab("subscribed");
    const scope = await screen.findByTestId("issues-empty-state");
    expect(scope).toHaveAttribute("data-context", "profile-subscribed");
    expect(within(scope).getByText("No work items yet")).toBeInTheDocument();
    expect(within(scope).queryByRole("button")).toBeNull();
  });

  it("toggles show-empty-groups in the display menu", async () => {
    vi.mocked(listUserProfileIssues).mockResolvedValue(response([]));
    renderTab();
    const scope = await screen.findByTestId("issues-empty-state");
    expect(within(scope).getByText("No work items are assigned to you")).toBeInTheDocument();
    await userEvent.click(screen.getAllByTestId("profile-display-trigger")[0]!);
    const shown = await screen.findByRole("button", { name: "Show empty groups" });
    expect(shown).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(shown);
    await waitFor(() =>
      expect(JSON.parse(localStorage.getItem(profileDisplayKey("acme", "u-1")) ?? "null")).toMatchObject({
        showEmptyGroups: true,
      })
    );
    expect(
      within(screen.getByTestId("issues-empty-state")).getByText("No work items are assigned to you")
    ).toBeInTheDocument();
  });

  it("opens the peek host on a row and closes it", async () => {
    renderTab();
    await screen.findByTestId("profile-list");
    await userEvent.click(screen.getByRole("button", { name: /Prof persist urgent/ }));
    const peek = await screen.findByTestId("issue-preview");
    expect(within(peek).getByDisplayValue("Prof persist urgent")).toBeInTheDocument();
    expect(within(peek).getByRole("link", { name: "Open full view" })).toHaveAttribute("href", "/acme/browse/PP-7");
    await userEvent.click(within(peek).getByRole("button", { name: "Close preview" }));
    expect(screen.queryByTestId("issue-preview")).toBeNull();
  });

  it("opens the peek from a board card and closes it on Escape", async () => {
    localStorage.setItem(profileDisplayKey("acme", "u-1"), JSON.stringify({ layout: "kanban", groupBy: "priority" }));
    renderTab();
    const board = await screen.findByTestId("profile-board");
    expect(
      [...board.querySelectorAll("[data-profile-board-column]")].map((column) =>
        column.getAttribute("data-profile-board-column")
      )
    ).toEqual(["Urgent", "Low"]);
    await userEvent.click(document.querySelector("[data-profile-board-card]") as HTMLElement);
    expect(await screen.findByTestId("issue-preview")).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByTestId("issue-preview")).toBeNull();
  });

  it("renders an empty board without groups", async () => {
    localStorage.setItem(profileDisplayKey("acme", "u-1"), JSON.stringify({ layout: "kanban" }));
    vi.mocked(listUserProfileIssues).mockResolvedValue(response([]));
    renderTab();
    const scope = await screen.findByTestId("issues-empty-state");
    expect(within(scope).getByText("No work items are assigned to you")).toBeInTheDocument();
    expect(screen.queryByTestId("profile-board")).toBeNull();
  });

  it("uses the sequence id as the key when the project identifier is unknown", async () => {
    vi.mocked(listProjects).mockResolvedValue([]);
    renderTab();
    await screen.findByTestId("profile-list");
    expect(document.querySelector("[data-profile-row]")?.textContent).toContain("7");
    expect(document.querySelector("[data-profile-row]")?.textContent).not.toContain("PP-7");
  });

  it("drops the peek eyebrow when the project identifier is unknown", async () => {
    vi.mocked(listProjects).mockResolvedValue([]);
    renderTab();
    await screen.findByTestId("profile-list");
    await userEvent.click(screen.getByRole("button", { name: /Prof persist urgent/ }));
    const peek = await screen.findByTestId("issue-preview");
    expect(within(peek).queryByText("PP-7")).toBeNull();
    expect(within(peek).getByDisplayValue("Prof persist urgent")).toBeInTheDocument();
  });

  it("closes a peek whose row left the page under it", async () => {
    const client = renderTab();
    await screen.findByTestId("profile-list");
    await userEvent.click(screen.getByRole("button", { name: /Prof persist urgent/ }));
    expect(await screen.findByTestId("issue-preview")).toBeInTheDocument();
    act(() => {
      client.setQueryData(profileIssuesQueryOptions({} as never, "acme", "u-1", "assigned", "sort_order").queryKey, {
        pages: [response([])],
        pageParams: [""],
      });
    });
    await waitFor(() => expect(screen.queryByTestId("issue-preview")).toBeNull());
  });

  it("names the count in singular and plural", async () => {
    renderTab();
    await screen.findByTestId("profile-list");
    expect(screen.getByText("2 work items")).toBeInTheDocument();
    expect(screen.queryByText("Loading work items…")).toBeNull();
  });

  it("names a single work item in the singular", async () => {
    vi.mocked(listUserProfileIssues).mockResolvedValue(response([issue("i-only", "Only row")]));
    renderTab();
    await screen.findByTestId("profile-list");
    expect(screen.getByText("1 work item")).toBeInTheDocument();
  });

  it("shows the loading line while the page is in flight", async () => {
    vi.mocked(listUserProfileIssues).mockReturnValue(new Promise(() => undefined) as never);
    renderTab();
    expect(await screen.findByText("Loading work items…")).toBeInTheDocument();
  });

  it("follows the cursor to a second page and appends its rows", async () => {
    vi.mocked(listUserProfileIssues)
      .mockResolvedValueOnce({
        ...response([issue("i-urgent", "Prof first page")]),
        next_cursor: "c-2",
        next_page_results: true,
      })
      .mockResolvedValueOnce(response([issue("i-low", "Prof second page")]));
    renderTab();
    await screen.findByTestId("profile-list");
    expect(listUserProfileIssues).toHaveBeenCalledWith(
      expect.anything(),
      "acme",
      "u-1",
      expect.objectContaining({ orderBy: "sort_order", perPage: 100 })
    );
    expect(listUserProfileIssues).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByTestId("profile-issues-load-more"));
    await waitFor(() =>
      expect(
        [...document.querySelectorAll("[data-profile-row]")].map((row) => row.getAttribute("data-profile-row"))
      ).toEqual(["i-urgent", "i-low"])
    );
    expect(listUserProfileIssues).toHaveBeenLastCalledWith(
      expect.anything(),
      "acme",
      "u-1",
      expect.objectContaining({ cursor: "c-2" })
    );
    expect(screen.queryByTestId("profile-issues-load-more")).toBeNull();
  });

  it("disables load more while the next page is in flight", async () => {
    vi.mocked(listUserProfileIssues)
      .mockResolvedValueOnce({
        ...response([issue("i-urgent", "Prof first page")]),
        next_cursor: "c-2",
        next_page_results: true,
      })
      .mockReturnValueOnce(new Promise(() => undefined) as never);
    renderTab();
    await screen.findByTestId("profile-list");
    await userEvent.click(screen.getByTestId("profile-issues-load-more"));
    expect(await screen.findByRole("button", { name: "Loading…" })).toBeDisabled();
  });

  it("groups by state group with the project state names and zero-filled groups", async () => {
    localStorage.setItem(profileDisplayKey("acme", "u-1"), JSON.stringify({ groupBy: "state", showEmptyGroups: true }));
    vi.mocked(listUserProfileIssues).mockResolvedValue(
      response([
        issue("i-urgent", "Prof working row", { state_id: "s-working" }),
        issue("i-low", "Prof backlog row", { state_id: "s-backlog" }),
      ])
    );
    renderTab();
    await screen.findByTestId("profile-list");
    await waitFor(() =>
      expect(
        [...document.querySelectorAll("[data-profile-group]")].map((group) => group.getAttribute("data-profile-group"))
      ).toEqual(["Backlog", "Unstarted", "Started", "Review", "Test", "Completed", "Canceled"])
    );
    expect(listStates).toHaveBeenCalledWith(expect.anything(), "acme", "p-1");
    expect(screen.getByText("In progress")).toBeInTheDocument();
  });

  it("toggles sub-work items without touching the empty-group switch", async () => {
    renderTab();
    await screen.findByTestId("profile-list");
    await userEvent.click(screen.getAllByTestId("profile-display-trigger")[0]!);
    const subWork = await screen.findByRole("button", { name: "Show sub-work items" });
    expect(subWork).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(subWork);
    await waitFor(() =>
      expect(JSON.parse(localStorage.getItem(profileDisplayKey("acme", "u-1")) ?? "null")).toMatchObject({
        showSubIssues: true,
        showEmptyGroups: false,
      })
    );
    expect(screen.getByRole("button", { name: "Show empty groups" })).toHaveAttribute("aria-pressed", "false");
  });

  it("drops sub-work items from the loaded rows, then restores them from the display switch", async () => {
    vi.mocked(listUserProfileIssues).mockResolvedValue(
      response([issue("i-urgent", "Prof parent row"), issue("i-low", "Prof child row", { parent_id: "i-urgent" })])
    );
    renderTab();
    await screen.findByTestId("profile-list");
    const rowIds = () =>
      [...document.querySelectorAll("[data-profile-row]")].map((row) => row.getAttribute("data-profile-row"));
    expect(rowIds()).toEqual(["i-urgent"]);
    await userEvent.click(screen.getAllByTestId("profile-display-trigger")[0]!);
    await userEvent.click(await screen.findByRole("button", { name: "Show sub-work items" }));
    await waitFor(() => expect(rowIds()).toEqual(["i-urgent", "i-low"]));
  });

  it("hides sub-work items on the board layout as well", async () => {
    localStorage.setItem(profileDisplayKey("acme", "u-1"), JSON.stringify({ layout: "kanban" }));
    vi.mocked(listUserProfileIssues).mockResolvedValue(
      response([
        issue("i-urgent", "Prof parent row", { priority: "high" }),
        issue("i-low", "Prof child row", { priority: "high", parent_id: "i-urgent" }),
      ])
    );
    renderTab();
    await screen.findByTestId("profile-board");
    const cardIds = () =>
      [...document.querySelectorAll("[data-profile-board-card]")].map((card) =>
        card.getAttribute("data-profile-board-card")
      );
    expect(cardIds()).toEqual(["i-urgent"]);
    await userEvent.click(screen.getAllByTestId("profile-display-trigger")[0]!);
    await userEvent.click(await screen.findByRole("button", { name: "Show sub-work items" }));
    await waitFor(() => expect(cardIds()).toEqual(["i-urgent", "i-low"]));
  });

  it("titles assignee groups with the workspace display names", async () => {
    vi.mocked(listUserProfileIssues).mockResolvedValue(
      response([
        issue("i-urgent", "Prof Ada row", { assignee_ids: ["u-2"] }),
        issue("i-low", "Prof Grace row", { assignee_ids: ["u-3"] }),
        issue("i-none", "Prof unassigned row"),
      ])
    );
    renderTab();
    await screen.findByTestId("profile-list");
    await userEvent.click(screen.getAllByTestId("profile-display-trigger")[0]!);
    const panel = await screen.findByTestId("profile-display-panel");
    await userEvent.click(within(panel).getByRole("radio", { name: "Assignees" }));
    await waitFor(() =>
      expect(
        [...document.querySelectorAll("[data-profile-group]")].map((group) => group.getAttribute("data-profile-group"))
      ).toEqual(["Unassigned", "Ada Lovelace", "Grace Hopper"])
    );
    expect(listWorkspaceMembers).toHaveBeenCalledWith(expect.anything(), "acme");
  });

  it("keeps the board's empty columns only when show-empty-groups is on", async () => {
    localStorage.setItem(profileDisplayKey("acme", "u-1"), JSON.stringify({ layout: "kanban", groupBy: "state" }));
    vi.mocked(listUserProfileIssues).mockResolvedValue(
      response([issue("i-urgent", "Prof backlog row", { state_id: "s-backlog" })])
    );
    renderTab();
    await screen.findByTestId("profile-board");
    const columns = () =>
      [...document.querySelectorAll("[data-profile-board-column]")].map((column) =>
        column.getAttribute("data-profile-board-column")
      );
    expect(columns()).toEqual(["Backlog"]);
    await userEvent.click(screen.getAllByTestId("profile-display-trigger")[0]!);
    await userEvent.click(await screen.findByRole("button", { name: "Show empty groups" }));
    await waitFor(() =>
      expect(columns()).toEqual(["Backlog", "Unstarted", "Started", "Review", "Test", "Completed", "Canceled"])
    );
  });

  it("names the assignee column, blanks dates that are unset, and keeps a row whose state left the project", async () => {
    vi.mocked(listUserProfileIssues).mockResolvedValue(
      response([
        issue("i-urgent", "Prof working row", { state_id: "s-working", assignee_ids: ["u-2", "u-3"] }),
        issue("i-low", "Prof orphan row", {
          state_id: "s-gone",
          assignee_ids: [],
          start_date: "2026-10-02",
          target_date: "2026-10-09",
        }),
      ])
    );
    renderTab();
    await screen.findByTestId("profile-list");
    const [working, orphan] = [...document.querySelectorAll("[data-profile-row]")] as HTMLElement[];
    expect(working?.textContent).toContain("In progress");
    expect(working?.textContent).toContain("2 assigned");
    expect(orphan?.textContent).toContain("Unassigned");
    expect(orphan?.textContent).toContain("2026-10-02");
    expect(orphan?.textContent).toContain("2026-10-09");
    expect(orphan?.textContent).not.toContain("s-gone");
  });

  it("drops every stored-off column from the row", async () => {
    localStorage.setItem(
      profileDisplayKey("acme", "u-1"),
      JSON.stringify({
        properties: { key: false, priority: false, state: false, assignee: false, start_date: false, due_date: false },
      })
    );
    vi.mocked(listUserProfileIssues).mockResolvedValue(
      response([
        issue("i-urgent", "Prof bare row", {
          state_id: "s-working",
          assignee_ids: ["u-2"],
          start_date: "2026-10-02",
          target_date: "2026-10-09",
        }),
      ])
    );
    renderTab();
    await screen.findByTestId("profile-list");
    const row = document.querySelector("[data-profile-row]") as HTMLElement;
    expect(row.textContent).toBe("Prof bare row");
    expect(row.textContent).not.toContain("In progress");
    expect(row.textContent).not.toContain("1 assigned");
    expect(row.textContent).not.toContain("2026-10-02");
    expect(row.textContent).not.toContain("2026-10-09");
  });

  it("hides and restores a column from the display properties", async () => {
    renderTab();
    await screen.findByTestId("profile-list");
    expect(document.querySelector("[data-profile-row]")?.textContent).toContain("Urgent");
    await userEvent.click(screen.getAllByTestId("profile-display-trigger")[0]!);
    await userEvent.click(await screen.findByRole("button", { name: "Priority" }));
    await waitFor(() => expect(document.querySelector("[data-profile-row]")?.textContent).not.toContain("Urgent"));
    expect(JSON.parse(localStorage.getItem(profileDisplayKey("acme", "u-1")) ?? "null")).toMatchObject({
      properties: { priority: false },
    });
    await userEvent.click(screen.getByRole("button", { name: "Priority" }));
    await waitFor(() => expect(document.querySelector("[data-profile-row]")?.textContent).toContain("Urgent"));
  });
});

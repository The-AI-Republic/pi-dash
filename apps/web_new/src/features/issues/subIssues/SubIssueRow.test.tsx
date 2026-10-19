// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One sub-issue row (parity/specs/issues.md §15, ISS-174/176/177/238):
// identifier and name link to the child, the chevron lazily fetches the
// row's own children with its own load-more, the menu offers copy to
// everyone and the write entries to members, assignees draw member
// names, and a writing row shows its busy state.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { SubIssueItem, SubIssuesResponse, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { defaultDisplayProperties } from "./rules.js";
import { SubIssueRow, rowMenuItems, type SubIssueRowMenuActions } from "./SubIssueRow.js";
import type { SubIssueRowActions } from "./types.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const PARENT = "323e4567-e89b-12d3-a456-426614174002";
const CHILD = "423e4567-e89b-12d3-a456-426614174003";
const GRANDCHILD = "523e4567-e89b-12d3-a456-426614174004";
const NONE_JSON = "{}";

function row(overrides: Partial<SubIssueItem> = {}): SubIssueItem {
  return {
    id: CHILD,
    name: "Child one",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    start_date: null,
    target_date: null,
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: PARENT,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    state_group: "started",
    ...overrides,
  };
}

function page(rows: SubIssueItem[], nextCursor?: string): SubIssuesResponse {
  return {
    sub_issues: rows,
    state_distribution: {},
    ...(nextCursor === undefined ? {} : { next_cursor: nextCursor }),
  };
}

function ok(url: string, body: unknown): TransportResponse {
  return {
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  } as TransportResponse;
}

function stubChildren(pages: { rows: SubIssueItem[]; next?: string }[]) {
  let call = 0;
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return ok(url, { csrf_token: "csrf" });
    if (!url.includes("/sub-issues/")) return ok(url, NONE_JSON);
    const current = pages[Math.min(call, pages.length - 1)];
    call += 1;
    return ok(url, page(current?.rows ?? [], current?.next));
  }) as never);
}

function renderRow(
  options: {
    row?: SubIssueItem;
    editable?: boolean;
    busy?: boolean;
    isEpic?: boolean;
    properties?: ReturnType<typeof defaultDisplayProperties>;
    menu?: Partial<SubIssueRowMenuActions>;
    onOpen?: (row: SubIssueItem, identifier: string) => void;
    projectCodeFor?: (child: SubIssueItem) => string;
    memberIdentities?: Record<string, { label: string; avatarUrl: string | null }>;
  } = {}
) {
  const onOpen = options.onOpen ?? vi.fn();
  const menuActions: SubIssueRowMenuActions = {
    copyLink: vi.fn(),
    remove: vi.fn(),
    delete: vi.fn(),
    ...options.menu,
  };
  const actions: SubIssueRowActions = { isBusy: () => options.busy ?? false, write: vi.fn() };
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={client}>
      <SubIssueRow
        row={options.row ?? row()}
        workspaceSlug={WS}
        projectIdentifier="ATL"
        projectCodeFor={options.projectCodeFor ?? ((child) => (child.project_id === PROJECT ? "ATL" : "OTHER"))}
        memberIdentities={options.memberIdentities ?? {}}
        properties={options.properties ?? defaultDisplayProperties()}
        editable={options.editable ?? true}
        depth={0}
        ancestors={[PARENT]}
        isEpic={options.isEpic ?? false}
        actions={actions}
        onOpen={onOpen}
        menuActions={menuActions}
      />
    </QueryClientProvider>
  );
  return { onOpen, menuActions, ...view };
}

function menuButton(): HTMLElement {
  return screen.getByRole("button", { name: "Actions for ATL-7" });
}

beforeEach(() => {
  vi.clearAllMocks();
  stubChildren([{ rows: [] }]);
});

afterEach(() => {
  resetAppClient();
});

describe("row rendering", () => {
  it("shows the identifier, the name and a working link", () => {
    renderRow();
    expect(screen.getByText("ATL-7")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Child one/ })).toHaveAttribute("href", `/acme/browse/ATL-7`);
  });

  it("opens the child in place on a plain click and leaves modified clicks to the browser", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    renderRow({ onOpen });
    const link = screen.getByRole("link", { name: /Child one/ });
    await user.click(link);
    expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ id: CHILD }), "ATL-7");
    onOpen.mockClear();
    await user.keyboard("{Meta>}");
    link.dispatchEvent(new MouseEvent("click", { bubbles: true, metaKey: true, button: 0 }));
    await user.keyboard("{/Meta}");
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("hides a column the display turned off", () => {
    renderRow({ properties: { ...defaultDisplayProperties(), key: false, assignee: false } });
    expect(screen.queryByText("ATL-7")).not.toBeInTheDocument();
  });

  it("shows the saving spinner while a write is in flight", () => {
    renderRow({ busy: true });
    expect(screen.getByText("Saving sub-work item")).toBeInTheDocument();
  });

  it("draws the merged range only when both ends and both columns are set", () => {
    renderRow({ row: row({ start_date: "2026-10-01", target_date: "2026-10-10" }) });
    expect(screen.getByTestId("sub-issue-date-range")).toBeInTheDocument();
    expect(screen.queryByTestId("sub-issue-start-date")).not.toBeInTheDocument();
  });

  it("draws one date picker per visible column and no fallback date cell", () => {
    const { unmount } = renderRow({
      row: row({ start_date: "2026-10-01", target_date: "2026-10-10" }),
      properties: { ...defaultDisplayProperties(), due_date: false },
    });
    expect(screen.queryByTestId("sub-issue-date-range")).not.toBeInTheDocument();
    expect(screen.getByTestId("sub-issue-start-date")).toBeInTheDocument();
    expect(screen.queryByTestId("sub-issue-due-date")).not.toBeInTheDocument();
    // The start picker is the only place the single visible day appears.
    expect(screen.getAllByText("Oct 1, 2026")).toHaveLength(1);
    unmount();
  });

  it("draws no dates at all when both date columns are off", () => {
    renderRow({
      row: row({ start_date: "2026-10-01", target_date: "2026-10-10" }),
      properties: { ...defaultDisplayProperties(), start_date: false, due_date: false },
    });
    expect(screen.queryByTestId("sub-issue-date-range")).not.toBeInTheDocument();
    expect(screen.queryByTestId("sub-issue-start-date")).not.toBeInTheDocument();
    expect(screen.queryByTestId("sub-issue-due-date")).not.toBeInTheDocument();
    expect(screen.queryByText("Oct 1, 2026")).not.toBeInTheDocument();
  });

  it("gives no chevron to a row without children", () => {
    renderRow();
    expect(screen.queryByRole("button", { name: /Expand children/ })).not.toBeInTheDocument();
  });

  it("labels an epic child's remove entry as a work item", async () => {
    const user = userEvent.setup();
    renderRow({ isEpic: true });
    await user.click(menuButton());
    expect(await screen.findByRole("menuitem", { name: "Remove work item" })).toBeInTheDocument();
  });
});

describe("row menu", () => {
  it("offers the menu to members only", () => {
    const actions = { copyLink: vi.fn(), remove: vi.fn(), delete: vi.fn() };
    const guest = rowMenuItems(row(), false, false, actions);
    expect(guest).toEqual([]);
    const member = rowMenuItems(row(), true, false, actions);
    expect(member.map((entry) => ("label" in entry ? entry.label : "separator"))).toEqual([
      "Copy link",
      "separator",
      "Remove sub-work item",
      "Delete",
    ]);
  });

  it("draws no menu trigger for a guest row", () => {
    renderRow({ editable: false });
    expect(screen.queryByTestId("sub-issue-row-menu")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Actions for/ })).not.toBeInTheDocument();
  });

  it("runs the chosen entry through the section's actions", async () => {
    const user = userEvent.setup();
    const { menuActions } = renderRow();
    await user.click(menuButton());
    await user.click(await screen.findByRole("menuitem", { name: "Copy link" }));
    expect(menuActions.copyLink).toHaveBeenCalledWith(expect.objectContaining({ id: CHILD }));
    await user.click(menuButton());
    await user.click(await screen.findByRole("menuitem", { name: "Remove sub-work item" }));
    expect(menuActions.remove).toHaveBeenCalled();
    await user.click(menuButton());
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    expect(menuActions.delete).toHaveBeenCalled();
  });

  it("never navigates instead of editing: the edit modal is a later slice", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    renderRow({ onOpen });
    await user.click(menuButton());
    expect(screen.queryByRole("menuitem", { name: /Edit work item/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: /Open in new tab/ })).not.toBeInTheDocument();
    expect(onOpen).not.toHaveBeenCalled();
  });
});

describe("assignees", () => {
  it("draws member names and avatars from the identity map", () => {
    renderRow({
      row: row({ assignee_ids: ["923e4567-e89b-12d3-a456-426614174008"] }),
      memberIdentities: {
        "923e4567-e89b-12d3-a456-426614174008": { label: "Ada Lovelace", avatarUrl: "https://cdn.test/ada.png" },
      },
    });
    expect(screen.getByRole("img", { name: "Ada Lovelace" })).toBeInTheDocument();
    expect(screen.getByLabelText("Assigned to Ada Lovelace")).toBeInTheDocument();
    expect(screen.queryByText(/923e4567/)).not.toBeInTheDocument();
  });

  it("names several assignees and falls back for an unknown id", () => {
    renderRow({
      row: row({ assignee_ids: ["923e4567-e89b-12d3-a456-426614174008", "a23e4567-e89b-12d3-a456-426614174009"] }),
      memberIdentities: { "923e4567-e89b-12d3-a456-426614174008": { label: "Ada Lovelace", avatarUrl: null } },
    });
    expect(screen.getByLabelText("2 assignees: Ada Lovelace, Member")).toBeInTheDocument();
  });
});

describe("project codes", () => {
  it("labels a nested child from another project with its own code", async () => {
    const OTHER = "723e4567-e89b-12d3-a456-426614174006";
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) return ok(url, { csrf_token: "csrf" });
      return ok(
        url,
        page([row({ id: GRANDCHILD, name: "Elsewhere", sequence_id: 12, project_id: OTHER, sub_issues_count: null })])
      );
    }) as never);
    const user = userEvent.setup();
    renderRow({
      row: row({ sub_issues_count: 1 }),
      projectCodeFor: (child) => (child.project_id === PROJECT ? "ATL" : "BET"),
    });
    await user.click(screen.getByRole("button", { name: "Expand children of ATL-7" }));
    expect(await screen.findByText("BET-12")).toBeInTheDocument();
  });

  it("reads an epic row's children through the epics collection", async () => {
    const calls: string[] = [];
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) return ok(url, { csrf_token: "csrf" });
      calls.push(url);
      return ok(url, page([]));
    }) as never);
    const user = userEvent.setup();
    renderRow({ row: row({ sub_issues_count: 1 }), isEpic: true });
    await user.click(screen.getByRole("button", { name: "Expand children of ATL-7" }));
    await waitFor(() => expect(calls.some((url) => url.includes(`/epics/${CHILD}/issues/`))).toBe(true));
    expect(calls.some((url) => url.includes(`/sub-issues/`))).toBe(false);
  });
});

describe("lazy children", () => {
  it("fetches nothing until the chevron opens the row", async () => {
    const calls: string[] = [];
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) return ok(url, { csrf_token: "csrf" });
      if (!url.includes("/sub-issues/")) return ok(url, NONE_JSON);
      calls.push(url);
      return ok(url, page([]));
    }) as never);
    const user = userEvent.setup();
    renderRow({ row: row({ sub_issues_count: 1 }) });
    expect(calls.filter((url) => url.includes("/sub-issues/"))).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Expand children of ATL-7" }));
    await waitFor(() => expect(calls.some((url) => url.includes(`/issues/${CHILD}/sub-issues/`))).toBe(true));
    expect(await screen.findByText("No children yet.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Collapse children of ATL-7" })).toHaveAttribute("aria-expanded", "true");
  });

  it("renders the nested rows and a load-more row that appends the next page", async () => {
    stubChildren([
      { rows: [row({ id: GRANDCHILD, name: "Grandchild", sequence_id: 9, sub_issues_count: null })], next: "c2" },
      { rows: [row({ id: "623e4567-e89b-12d3-a456-426614174005", name: "Later child", sequence_id: 10 })] },
    ]);
    const user = userEvent.setup();
    renderRow({ row: row({ sub_issues_count: 2 }) });
    await user.click(screen.getByRole("button", { name: "Expand children of ATL-7" }));
    expect(await screen.findByText("Grandchild")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Load more" }));
    expect(await screen.findByText("Later child")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
  });

  it("shows the loading row while the first page resolves", async () => {
    let release: () => void = () => {};
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) return ok(url, { csrf_token: "csrf" });
      await new Promise<void>((resolve) => {
        release = resolve;
      });
      return ok(url, page([]));
    }) as never);
    const user = userEvent.setup();
    renderRow({ row: row({ sub_issues_count: 1 }) });
    await user.click(screen.getByRole("button", { name: "Expand children of ATL-7" }));
    expect(await screen.findByText("Loading children")).toBeInTheDocument();
    release();
    await waitFor(() => expect(screen.queryByText("Loading children")).not.toBeInTheDocument());
  });

  it("offers a retry when the children read fails", async () => {
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) return ok(url, { csrf_token: "csrf" });
      return { status: 500, url, headers: { get: () => null }, text: async () => NONE_JSON } as TransportResponse;
    }) as never);
    const user = userEvent.setup();
    renderRow({ row: row({ sub_issues_count: 1 }) });
    await user.click(screen.getByRole("button", { name: "Expand children of ATL-7" }));
    expect(await screen.findByText("Couldn't load the children.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });

  it("stops a cycle in bad data instead of recursing", async () => {
    // A child that names itself keeps returning the same id.
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) return ok(url, { csrf_token: "csrf" });
      return ok(url, page([row({ id: CHILD, name: "Loop", sub_issues_count: 1 })]));
    }) as never);
    const user = userEvent.setup();
    renderRow({ row: row({ sub_issues_count: 1 }) });
    await user.click(screen.getByRole("button", { name: "Expand children of ATL-7" }));
    const nested = await screen.findByText("This branch repeats itself.");
    expect(nested).toBeInTheDocument();
    const section = nested.closest("[data-sub-issue-row]") as HTMLElement;
    expect(within(section).getByText("Loop")).toBeInTheDocument();
  });
});

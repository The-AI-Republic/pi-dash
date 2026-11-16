// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { IssueListItem } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const queryMock = vi.hoisted(() => ({
  useQuery: vi.fn(),
  useQueryClient: vi.fn(),
  current: { data: undefined as unknown, isFetching: false },
}));

vi.mock("@tanstack/react-query", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-query")>()),
  useQuery: queryMock.useQuery,
  useQueryClient: queryMock.useQueryClient,
}));

vi.mock("./InlinePropertyStrip.js", () => ({
  InlinePropertyStrip: () => <div data-testid="strip" />,
}));

import type * as React from "react";

import { ListRow, MAX_INLINE_DEPTH, issueKey, rowBlockId, rowLinkId, type ListRowProps } from "./ListRow.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

function issue(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 42,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

function props(overrides: Partial<ListRowProps> = {}): ListRowProps {
  return {
    workspaceSlug: "acme",
    projectId: PROJECT,
    projectIdentifier: "WEB",
    issue: issue(),
    depth: 1,
    properties: {},
    flags: { hideCycle: false, hideModule: false },
    readOnly: false,
    states: [],
    stateNames: new Map(),
    assigneeNames: new Map(),
    labelNames: new Map(),
    cycleNames: new Map(),
    moduleNames: new Map(),
    openCycleIds: new Set(),
    estimateEnabled: false,
    highlightedId: null,
    onOpenPeek: vi.fn(),
    hrefFor: (row) => `/acme/browse/WEB-${row.sequence_id}`,
    ...overrides,
  };
}

/** Rows mount the shared quick-action hook, which needs a query client. */
function renderRow(node: React.ReactElement) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}>{node as never}</QueryClientProvider>);
}

/** The children read's answer; every other query returns idle data. */
function respond(value: { data?: unknown; isFetching?: boolean }) {
  queryMock.current = { data: undefined, isFetching: false, ...value };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", (() => Promise.reject(new Error("unexpected network"))) as never);
  queryMock.useQuery.mockImplementation((options: { queryKey: readonly unknown[] }) =>
    options.queryKey[0] === "ws" && options.queryKey.includes("projects")
      ? { data: [], isFetching: false }
      : queryMock.current
  );
  respond({});
  queryMock.useQueryClient.mockReturnValue({
    getQueryData: vi.fn(),
    invalidateQueries: vi.fn(),
    setQueryData: vi.fn(),
  });
});

afterEach(() => resetAppClient());

describe("ListRow", () => {
  it("renders a real browse anchor with the identifier and title", async () => {
    renderRow(<ListRow {...props()} />);
    const anchor = screen.getByLabelText("WEB-42");
    expect(anchor.tagName).toBe("A");
    expect(anchor).toHaveAttribute("href", "/acme/browse/WEB-42");
    expect(anchor.querySelector("p")).toHaveTextContent("Fix login");
    expect(anchor.querySelector("p")).toHaveAttribute("title", "Fix login");
    expect(document.getElementById(rowLinkId(ISSUE))).toBe(anchor);
    expect(document.getElementById(rowBlockId(ISSUE))).toContainElement(anchor);
    expect(await screen.findByTestId("strip")).toBeInTheDocument();
  });

  it("hides the identifier when the display ID property is off", () => {
    renderRow(<ListRow {...props({ properties: { key: false } })} />);
    const anchor = screen.getByLabelText("WEB-42");
    expect(anchor).toHaveTextContent("Fix login");
    expect(anchor).not.toHaveTextContent("WEB-42");
  });

  it("opens peek on a plain click and keeps modified clicks as links", () => {
    const onOpenPeek = vi.fn();
    renderRow(<ListRow {...props({ onOpenPeek })} />);
    const anchor = screen.getByLabelText("WEB-42");
    fireEvent.click(anchor, { ctrlKey: true });
    fireEvent.click(anchor, { metaKey: true });
    fireEvent.click(anchor, { shiftKey: true });
    fireEvent.click(anchor, { button: 1 });
    expect(onOpenPeek).not.toHaveBeenCalled();
    fireEvent.click(anchor);
    expect(onOpenPeek).toHaveBeenCalledWith(ISSUE, "Fix login");
  });

  it("marks a temp row as saving without the property strip", () => {
    const onOpenPeek = vi.fn();
    renderRow(<ListRow {...props({ issue: issue({ id: "temp-1", name: "Draft" }), onOpenPeek })} />);
    const anchor = screen.getByLabelText("Draft, saving");
    expect(anchor).toHaveTextContent("…");
    expect(anchor.querySelector("p")).toHaveTextContent("Draft");
    expect(screen.queryByTestId("strip")).not.toBeInTheDocument();
    expect(screen.getByTestId("list-row").parentElement).toHaveAttribute("data-temprow", "true");
    fireEvent.click(anchor);
    expect(onOpenPeek).not.toHaveBeenCalled();
  });

  it("hides the expander for items without children and for epics", () => {
    const first = renderRow(<ListRow {...props()} />);
    expect(screen.queryByRole("button", { name: /sub-work items/ })).not.toBeInTheDocument();
    first.unmount();
    renderRow(<ListRow {...props({ issue: issue({ is_epic: true, sub_issues_count: 2 }) })} />);
    expect(screen.queryByRole("button", { name: /sub-work items/ })).not.toBeInTheDocument();
  });

  it("loads children lazily, renders them, and shows the loading glyph", () => {
    const child = issue({ id: "child-1", name: "Child", sequence_id: 43, parent_id: ISSUE, sub_issues_count: null });
    respond({ data: { sub_issues: [child] }, isFetching: false });
    renderRow(<ListRow {...props({ issue: issue({ sub_issues_count: 1 }) })} />);
    expect(queryMock.useQuery.mock.calls[0]?.[0]).toMatchObject({ enabled: false });
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-work items of Fix login" }));
    expect(screen.getByRole("button", { name: "Collapse sub-work items of Fix login" })).toBeInTheDocument();
    expect(queryMock.useQuery.mock.calls.some(([options]) => options.enabled === true)).toBe(true);
    expect(screen.getByText("Child")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Collapse sub-work items of Fix login" })).toHaveAttribute(
      "aria-expanded",
      "true"
    );
  });

  it("renders expanded children outside the fixed-height row so rows stack", () => {
    const child = issue({ id: "child-1", name: "Child", sequence_id: 43, parent_id: ISSUE, sub_issues_count: null });
    respond({ data: { sub_issues: [child] }, isFetching: false });
    renderRow(<ListRow {...props({ issue: issue({ sub_issues_count: 1 }) })} />);
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-work items of Fix login" }));
    const [parentRow] = screen.getAllByTestId("list-row");
    const childBlock = document.getElementById(rowBlockId("child-1"));
    expect(childBlock).not.toBeNull();
    expect(parentRow?.contains(childBlock)).toBe(false);
    expect(screen.getByTestId("list-row-children")).toContainElement(childBlock);
  });

  it("shows a spinner while the children request runs", () => {
    respond({ data: { sub_issues: [] }, isFetching: true });
    renderRow(<ListRow {...props({ issue: issue({ sub_issues_count: 1 }) })} />);
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-work items of Fix login" }));
    expect(
      screen.getByRole("button", { name: "Collapse sub-work items of Fix login" }).querySelector("svg")
    ).toHaveClass("animate-spin");
  });

  it("hands nesting at the cap to peek", () => {
    const onOpenPeek = vi.fn();
    render(
      <ListRow
        {...props({ issue: issue({ sub_issues_count: 1, name: "Deep" }), depth: MAX_INLINE_DEPTH, onOpenPeek })}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Open sub-work items of Deep in preview" }));
    expect(onOpenPeek).toHaveBeenCalledWith(ISSUE, "Deep");
  });

  it("hides the expander when the display toggle is off", () => {
    respond({ data: { sub_issues: [] }, isFetching: false });
    renderRow(<ListRow {...props({ issue: issue({ sub_issues_count: 1 }), showSubIssues: false })} />);
    expect(screen.queryByRole("button", { name: /sub-work items/ })).not.toBeInTheDocument();
    expect(queryMock.useQuery.mock.calls[0]?.[0]).toMatchObject({ enabled: false });
  });

  it("exposes the deterministic key and row ids", () => {
    expect(issueKey("WEB", { sequence_id: 7 })).toBe("WEB-7");
    expect(rowLinkId("issue-1")).toBe("issue-issue-1");
    expect(rowBlockId("issue-1")).toBe("issue_issue-1");
  });

  it("carries the row menu on hover and on right-click (ISS-060)", async () => {
    renderRow(<ListRow {...props()} />);
    const trigger = screen.getByRole("button", { name: "Actions for WEB-42" });
    expect(trigger.closest("[data-row-menu='true']")).toHaveClass("opacity-0");
    fireEvent.contextMenu(screen.getByTestId("list-row"));
    expect(screen.getAllByRole("menuitem", { hidden: true }).length).toBeGreaterThan(0);
  });

  it("keeps a saving row and a row without a menu target free of triggers", () => {
    queryMock.useQuery.mockImplementation((options: { queryKey: readonly unknown[] }) =>
      options.queryKey[0] === "ws" && options.queryKey.includes("projects")
        ? { data: [], isFetching: false }
        : queryMock.current
    );
    respond({});
    renderRow(<ListRow {...props({ issue: issue({ id: "temp-9" }) })} />);
    expect(screen.queryByRole("button", { name: /^Actions for/ })).not.toBeInTheDocument();
  });

  it("suppresses the quick-action menu for a read-only surface (archived rows)", () => {
    renderRow(<ListRow {...props({ showMenu: false })} />);
    expect(screen.queryByRole("button", { name: /^Actions for/ })).not.toBeInTheDocument();
    expect(screen.queryAllByRole("menuitem", { hidden: true })).toHaveLength(0);
  });

  it("takes an explicit write standing and a change hook from the layout (ISS-063)", async () => {
    const onMenuChanged = vi.fn();
    renderRow(
      <ListRow
        {...props({
          menuContext: "module",
          menuModuleId: "523e4567-e89b-12d3-a456-426614174004",
          menuCanWrite: false,
          onMenuChanged,
        })}
      />
    );
    expect(screen.getByRole("button", { name: "Actions for WEB-42" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Actions for WEB-42" }));
    const titles = screen.getAllByRole("menuitem", { hidden: true }).map((item) => item.textContent ?? "");
    // A read-only standing keeps only the free pair, even in a module.
    expect(titles.some((title) => title.includes("Edit"))).toBe(false);
    expect(titles.some((title) => title.includes("Open in new tab"))).toBe(true);
    expect(onMenuChanged).not.toHaveBeenCalled();
  });

  it("opens the menu in the cycle context the layout hands it (ISS-062)", () => {
    renderRow(<ListRow {...props({ menuContext: "cycle", menuCycleId: "423e4567-e89b-12d3-a456-426614174003" })} />);
    fireEvent.click(screen.getByRole("button", { name: "Actions for WEB-42" }));
    expect(screen.getAllByRole("menuitem", { hidden: true }).length).toBeGreaterThan(0);
  });

  it("hands the menu an override address for its link entries (ARCH-010)", () => {
    const open = vi.fn();
    Object.defineProperty(globalThis, "open", { value: open, configurable: true, writable: true });
    const linkHref = `/acme/projects/${PROJECT}/archives/issues/${ISSUE}`;
    renderRow(<ListRow {...props({ menuContext: "archived", menuCanWrite: true, menuLinkHref: linkHref })} />);
    fireEvent.click(screen.getByRole("button", { name: "Actions for WEB-42" }));
    const entry = screen
      .getAllByRole("menuitem", { hidden: true })
      .find((item) => item.textContent?.includes("Open in new tab"));
    if (entry === undefined) throw new Error("expected the open entry");
    fireEvent.click(entry);
    expect(open).toHaveBeenCalledWith(linkHref, "_blank", "noopener,noreferrer");
  });
});

describe("ListRow selection", () => {
  it("renders no selection control without a toggle handler", () => {
    render(<ListRow {...props()} />);
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("toggles selection with the shift state from the row control", () => {
    const onSelectionToggle = vi.fn();
    render(<ListRow {...props({ onSelectionToggle, selected: true, active: true })} />);
    const box = screen.getByRole("checkbox", { name: "Select WEB-42" });
    expect(box).toBeChecked();
    expect(screen.getByTestId("list-row")).toHaveAttribute("data-active", "true");
    fireEvent.click(box, { shiftKey: true });
    expect(onSelectionToggle).toHaveBeenCalledWith(ISSUE, true);
  });

  it("renders the control on nested rows", () => {
    const onSelectionToggle = vi.fn();
    render(<ListRow {...props({ depth: 2, onSelectionToggle })} />);
    fireEvent.click(screen.getByRole("checkbox", { name: "Select WEB-42" }));
    expect(onSelectionToggle).toHaveBeenCalledWith(ISSUE, false);
  });

  it("replaces the control with a disabled hint for a foreign-project row", () => {
    render(<ListRow {...props({ onSelectionToggle: vi.fn(), selectionAvailable: false })} />);
    expect(screen.queryByRole("checkbox", { name: "Select WEB-42" })).not.toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Cannot select WEB-42" })).toBeInTheDocument();
  });

  it("reports expansion and loaded children to the layout", () => {
    const onExpandedChange = vi.fn();
    const onChildrenChange = vi.fn();
    const child = issue({ id: "child-1", name: "Child", sequence_id: 43, parent_id: ISSUE, sub_issues_count: null });
    queryMock.useQuery.mockImplementation((options: { queryKey: readonly unknown[] }) =>
      options.queryKey[0] === "ws" && options.queryKey.includes("projects")
        ? { data: [], isFetching: false }
        : { data: { sub_issues: [child] }, isFetching: false }
    );
    render(<ListRow {...props({ issue: issue({ sub_issues_count: 1 }), onExpandedChange, onChildrenChange })} />);
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-work items of Fix login" }));
    expect(onExpandedChange).toHaveBeenCalledWith(ISSUE, true);
    expect(onChildrenChange).toHaveBeenLastCalledWith(ISSUE, [child]);
    fireEvent.click(screen.getByRole("button", { name: "Collapse sub-work items of Fix login" }));
    expect(onExpandedChange).toHaveBeenLastCalledWith(ISSUE, false);
  });

  it("uses the shared expansion tree and per-row key resolver when supplied", () => {
    const child = issue({ id: "child-1", name: "Child", sequence_id: 43, parent_id: ISSUE, project_id: "other" });
    const onSelectionToggle = vi.fn();
    render(
      <ListRow
        {...props({
          issue: issue({ sub_issues_count: 1 }),
          onSelectionToggle,
          projectIdentifierFor: (row) => (row.id === "child-1" ? "OTHER" : "WEB"),
          selectionTree: {
            expanded: new Set([ISSUE]),
            children: new Map([[ISSUE, [child]]]),
          },
        })}
      />
    );
    expect(screen.getByText("Child")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Cannot select OTHER-43" })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "Select OTHER-43" })).not.toBeInTheDocument();
    expect(queryMock.useQuery.mock.calls.some(([options]) => options.enabled === true)).toBe(true);
  });

  it("reports an empty child list when the expanded payload omits sub-issues", () => {
    const onChildrenChange = vi.fn();
    queryMock.useQuery.mockImplementation((options: { queryKey: readonly unknown[] }) =>
      options.queryKey[0] === "ws" && options.queryKey.includes("projects")
        ? { data: [], isFetching: false }
        : { data: {}, isFetching: false }
    );
    render(<ListRow {...props({ issue: issue({ sub_issues_count: 1 }), onChildrenChange })} />);
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-work items of Fix login" }));
    expect(onChildrenChange).toHaveBeenCalledWith(ISSUE, []);
  });

  it("gives each nested row its own selected and active state", () => {
    const child = issue({ id: "child-1", name: "Child", sequence_id: 43, parent_id: ISSUE });
    render(
      <ListRow
        {...props({
          issue: issue({ sub_issues_count: 1 }),
          onSelectionToggle: vi.fn(),
          selected: false,
          active: false,
          selectionSelectedFor: (id) => id === "child-1",
          selectionActiveFor: (id) => id === "child-1",
          selectionTree: { expanded: new Set([ISSUE]), children: new Map([[ISSUE, [child]]]) },
        })}
      />
    );
    const boxes = screen.getAllByRole("checkbox");
    expect(boxes[0]).not.toBeChecked();
    expect(boxes[1]).toBeChecked();
    const childRow = screen.getByRole("checkbox", { name: "Select WEB-43" }).closest("[data-selection-id]");
    expect(childRow).toHaveAttribute("data-active", "true");
  });
});

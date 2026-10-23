// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { IssueListItem } from "@pidash/api-client";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const queryMock = vi.hoisted(() => ({ useQuery: vi.fn() }));

vi.mock("@tanstack/react-query", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-query")>()),
  useQuery: queryMock.useQuery,
}));

vi.mock("./InlinePropertyStrip.js", () => ({
  InlinePropertyStrip: () => <div data-testid="strip" />,
}));

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

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", (() => Promise.reject(new Error("unexpected network"))) as never);
  queryMock.useQuery.mockReturnValue({ data: undefined, isFetching: false });
});

afterEach(() => resetAppClient());

describe("ListRow", () => {
  it("renders a real browse anchor with the identifier and title", async () => {
    render(<ListRow {...props()} />);
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
    render(<ListRow {...props({ properties: { key: false } })} />);
    const anchor = screen.getByLabelText("WEB-42");
    expect(anchor).toHaveTextContent("Fix login");
    expect(anchor).not.toHaveTextContent("WEB-42");
  });

  it("opens peek on a plain click and keeps modified clicks as links", () => {
    const onOpenPeek = vi.fn();
    render(<ListRow {...props({ onOpenPeek })} />);
    const anchor = screen.getByLabelText("WEB-42");
    fireEvent.click(anchor, { ctrlKey: true });
    fireEvent.click(anchor, { metaKey: true });
    fireEvent.click(anchor, { shiftKey: true });
    fireEvent.click(anchor, { button: 1 });
    expect(onOpenPeek).not.toHaveBeenCalled();
    fireEvent.click(anchor);
    expect(onOpenPeek).toHaveBeenCalledWith("WEB-42", "Fix login");
  });

  it("marks a temp row as saving without the property strip", () => {
    const onOpenPeek = vi.fn();
    render(<ListRow {...props({ issue: issue({ id: "temp-1", name: "Draft" }), onOpenPeek })} />);
    const anchor = screen.getByLabelText("Draft, saving");
    expect(anchor).toHaveTextContent("…");
    expect(anchor.querySelector("p")).toHaveTextContent("Draft");
    expect(screen.queryByTestId("strip")).not.toBeInTheDocument();
    expect(screen.getByTestId("list-row").parentElement).toHaveAttribute("data-temprow", "true");
    fireEvent.click(anchor);
    expect(onOpenPeek).not.toHaveBeenCalled();
  });

  it("hides the expander for items without children and for epics", () => {
    const first = render(<ListRow {...props()} />);
    expect(screen.queryByRole("button", { name: /sub-work items/ })).not.toBeInTheDocument();
    first.unmount();
    render(<ListRow {...props({ issue: issue({ is_epic: true, sub_issues_count: 2 }) })} />);
    expect(screen.queryByRole("button", { name: /sub-work items/ })).not.toBeInTheDocument();
  });

  it("loads children lazily, renders them, and shows the loading glyph", () => {
    const child = issue({ id: "child-1", name: "Child", sequence_id: 43, parent_id: ISSUE, sub_issues_count: null });
    queryMock.useQuery.mockReturnValue({ data: { sub_issues: [child] }, isFetching: false });
    render(<ListRow {...props({ issue: issue({ sub_issues_count: 1 }) })} />);
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
    queryMock.useQuery.mockReturnValue({ data: { sub_issues: [child] }, isFetching: false });
    render(<ListRow {...props({ issue: issue({ sub_issues_count: 1 }) })} />);
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-work items of Fix login" }));
    const [parentRow] = screen.getAllByTestId("list-row");
    const childBlock = document.getElementById(rowBlockId("child-1"));
    expect(childBlock).not.toBeNull();
    expect(parentRow?.contains(childBlock)).toBe(false);
    expect(screen.getByTestId("list-row-children")).toContainElement(childBlock);
  });

  it("shows a spinner while the children request runs", () => {
    queryMock.useQuery.mockReturnValue({ data: { sub_issues: [] }, isFetching: true });
    render(<ListRow {...props({ issue: issue({ sub_issues_count: 1 }) })} />);
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
    expect(onOpenPeek).toHaveBeenCalledWith("WEB-42", "Deep");
  });

  it("hides the expander when the display toggle is off", () => {
    queryMock.useQuery.mockReturnValue({ data: { sub_issues: [] }, isFetching: false });
    render(<ListRow {...props({ issue: issue({ sub_issues_count: 1 }), showSubIssues: false })} />);
    expect(screen.queryByRole("button", { name: /sub-work items/ })).not.toBeInTheDocument();
    expect(queryMock.useQuery.mock.calls[0]?.[0]).toMatchObject({ enabled: false });
  });

  it("exposes the deterministic key and row ids", () => {
    expect(issueKey("WEB", { sequence_id: 7 })).toBe("WEB-7");
    expect(rowLinkId("issue-1")).toBe("issue-issue-1");
    expect(rowBlockId("issue-1")).toBe("issue_issue-1");
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
    queryMock.useQuery.mockReturnValue({ data: { sub_issues: [child] }, isFetching: false });
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
    queryMock.useQuery.mockReturnValue({ data: {}, isFetching: false });
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

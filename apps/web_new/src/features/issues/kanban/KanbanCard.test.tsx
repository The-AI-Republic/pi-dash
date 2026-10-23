// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board card (parity/specs/issues.md §5, ISS-033/034): the real anchor,
// the identifier, the clamped name with its tooltip, the hover quick
// actions, the property strip and the inert unconfirmed row.
import type { IssueListItem } from "@pidash/api-client";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const narrow = vi.hoisted(() => ({ value: false }));

vi.mock("../../../shared/shell/narrowViewport.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/shell/narrowViewport.js")>()),
  useNarrowViewport: () => narrow.value,
}));
vi.mock("../list/InlinePropertyStrip.js", () => ({
  InlinePropertyStrip: (props: { issue: IssueListItem; readOnly: boolean }) => (
    <div data-testid="strip" data-issue={props.issue.id} data-readonly={String(props.readOnly)} />
  ),
}));

import { cardDomId, cardKey, KanbanCard } from "./KanbanCard.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const STATE = "323e4567-e89b-12d3-a456-426614174002";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

function issue(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: STATE,
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

function setup(overrides: Partial<React.ComponentProps<typeof KanbanCard>> = {}) {
  const onOpen = vi.fn();
  const onCopyLink = vi.fn();
  const props: React.ComponentProps<typeof KanbanCard> = {
    workspaceSlug: "acme",
    projectId: PROJECT,
    projectIdentifier: "WEB",
    issue: issue(),
    groupKey: STATE,
    laneKey: null,
    properties: {},
    flags: { hideCycle: false, hideModule: false },
    readOnly: false,
    peeked: false,
    estimateEnabled: true,
    states: [],
    stateNames: new Map(),
    assigneeNames: new Map(),
    labelNames: new Map(),
    cycleNames: new Map(),
    moduleNames: new Map(),
    openCycleIds: new Set(),
    timeZone: null,
    href: "/acme/browse/WEB-42",
    onOpen,
    onCopyLink,
    ...overrides,
  };
  return { onOpen, onCopyLink, ...render(<KanbanCard {...props} />) };
}

describe("cardKey", () => {
  it("prints the project prefix with the sequence", () => {
    expect(cardKey("WEB", { sequence_id: 12 })).toBe("WEB-12");
  });

  it("spells the flat lane as null in the card id", () => {
    expect(cardDomId("abc", "todo", null)).toBe("issue_abc_todo_null");
    expect(cardDomId("abc", "todo", "bug")).toBe("issue_abc_todo_bug");
  });
});

/** jsdom has no router: stop plain anchor activation from navigating. */
function blockNavigation(): () => void {
  const stop = (event: MouseEvent): void => {
    event.preventDefault();
  };
  document.addEventListener("click", stop);
  return () => {
    document.removeEventListener("click", stop);
  };
}

describe("KanbanCard", () => {
  it("renders a real anchor with the identifier, name and tooltip", () => {
    narrow.value = false;
    setup();
    const link = screen.getByTestId("kanban-card-link");
    expect(link).toHaveAttribute("href", "/acme/browse/WEB-42");
    expect(link).toHaveAttribute("id", `issue_${ISSUE}_${STATE}_null`);
    expect(screen.getByText("WEB-42")).toBeInTheDocument();
    const name = screen.getByTestId("kanban-card-name");
    expect(name).toHaveTextContent("Fix login");
    expect(name).toHaveAttribute("title", "Fix login");
  });

  it("hides the identifier when the ID property is off", () => {
    setup({ properties: { key: false } });
    expect(screen.queryByText("WEB-42")).not.toBeInTheDocument();
  });

  it("opens peek on a plain click and keeps modifier clicks native", async () => {
    const unblock = blockNavigation();
    const user = userEvent.setup();
    const { onOpen } = setup();
    const link = screen.getByTestId("kanban-card-link");
    await user.click(link);
    expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ id: ISSUE }));
    onOpen.mockClear();
    fireEvent.click(link, { metaKey: true });
    fireEvent.click(link, { button: 1 });
    expect(onOpen).not.toHaveBeenCalled();
    unblock();
  });

  it("marks the peeked card and passes the read-only state to the strip", () => {
    setup({ peeked: true, readOnly: true });
    expect(screen.getByTestId("kanban-card")).toHaveAttribute("data-peeked", "true");
    expect(screen.getByTestId("strip")).toHaveAttribute("data-readonly", "true");
  });

  it("draws the resting and hover borders from kit tokens only", () => {
    setup();
    const className = screen.getByTestId("kanban-card").className;
    expect(className).toContain("border-(--border)");
    expect(className).toContain("hover:border-(--text-muted)");
    expect(className).not.toContain("--border-strong");
  });

  it("hides the quick actions until hover on a wide viewport and always shows them on narrow", () => {
    narrow.value = false;
    const wide = setup();
    expect(screen.getByTestId("kanban-card-actions").className).toContain("group-hover/card:block");
    wide.unmount();
    narrow.value = true;
    setup();
    expect(screen.getByTestId("kanban-card-actions").className).toContain("block");
  });

  it("offers open and copy-link actions", async () => {
    const user = userEvent.setup({ pointerEventsCheck: 0 });
    const { onOpen, onCopyLink } = setup();
    await user.click(screen.getByRole("button", { name: "Actions for Fix login" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Open work item" }));
    expect(onOpen).toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Actions for Fix login" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Copy link" }));
    expect(onCopyLink).toHaveBeenCalledWith(expect.objectContaining({ id: ISSUE }));
  });

  it("renders an unconfirmed create inert: no link, no actions, no strip", () => {
    setup({ issue: issue({ id: "temp-1" }) });
    const card = screen.getByTestId("kanban-card");
    expect(card).toHaveAttribute("data-unconfirmed", "true");
    const link = screen.getByTestId("kanban-card-link");
    expect(link).not.toHaveAttribute("href");
    expect(link).toHaveAttribute("aria-disabled", "true");
    expect(screen.queryByTestId("kanban-card-actions")).not.toBeInTheDocument();
    expect(screen.queryByTestId("strip")).not.toBeInTheDocument();
    expect(link).toHaveTextContent("…");
  });
});

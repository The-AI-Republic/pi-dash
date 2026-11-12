// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board card (parity/specs/issues.md §5, ISS-033/034, ISS-037): the real
// anchor, the identifier, the clamped name with its tooltip, the hover
// quick actions, the property strip, the inert unconfirmed row, and the
// drag press that hands a travelled card to the board.
import type { IssueListItem } from "@pidash/api-client";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

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

import { BOARD_DRAG_THRESHOLD_PX } from "./drag.js";
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

function pointer(type: string, clientX: number, clientY: number): MouseEvent {
  return new MouseEvent(type, { bubbles: true, cancelable: true, clientX, clientY });
}

describe("KanbanCard drag press", () => {
  afterEach(() => {
    delete (window.HTMLElement.prototype as { scrollIntoView?: unknown }).scrollIntoView;
  });

  it("hands a travelled press to the board and swallows the after-click", () => {
    const unblock = blockNavigation();
    const onDragStart = vi.fn();
    const { onOpen } = setup({ draggable: true, onDragStart });
    const card = screen.getByTestId("kanban-card");
    fireEvent(card, pointer("pointerdown", 10, 10));
    fireEvent(document, pointer("pointermove", 11, 11));
    expect(onDragStart).not.toHaveBeenCalled();
    fireEvent(document, pointer("pointermove", 10 + BOARD_DRAG_THRESHOLD_PX, 10));
    expect(onDragStart).toHaveBeenCalledTimes(1);
    expect(onDragStart).toHaveBeenCalledWith(expect.objectContaining({ id: ISSUE }), expect.anything());
    fireEvent.click(screen.getByTestId("kanban-card-link"));
    expect(onOpen).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("kanban-card-link"));
    expect(onOpen).toHaveBeenCalledTimes(1);
    unblock();
  });

  it("never arms without a handler, while disabled, or unconfirmed", () => {
    const unblock = blockNavigation();
    const { onOpen, unmount } = setup({ draggable: true });
    const card = screen.getByTestId("kanban-card");
    fireEvent(card, pointer("pointerdown", 10, 10));
    fireEvent(document, pointer("pointermove", 80, 80));
    fireEvent.click(screen.getByTestId("kanban-card-link"));
    expect(onOpen).toHaveBeenCalledTimes(1);
    unmount();
    const gated = setup({ draggable: false, onDragStart: vi.fn() });
    fireEvent(screen.getByTestId("kanban-card"), pointer("pointerdown", 10, 10));
    fireEvent(document, pointer("pointermove", 80, 80));
    gated.unmount();
    setup({ draggable: true, onDragStart: vi.fn(), issue: issue({ id: "temp-1" }) });
    fireEvent(screen.getByTestId("kanban-card"), pointer("pointerdown", 10, 10));
    fireEvent(document, pointer("pointermove", 80, 80));
    unblock();
  });

  it("ignores presses starting on the card's own controls", () => {
    const onDragStart = vi.fn();
    narrow.value = true;
    setup({ draggable: true, onDragStart });
    fireEvent(screen.getByRole("button", { name: "Actions for Fix login" }), pointer("pointerdown", 10, 10));
    fireEvent(document, pointer("pointermove", 80, 80));
    expect(onDragStart).not.toHaveBeenCalled();
    narrow.value = false;
  });

  it("ignores moves without a press", () => {
    const onDragStart = vi.fn();
    setup({ draggable: true, onDragStart });
    fireEvent(document, pointer("pointermove", 80, 80));
    expect(onDragStart).not.toHaveBeenCalled();
  });

  it("starts only once when a second press lands before the first ends", () => {
    const onDragStart = vi.fn();
    setup({ draggable: true, onDragStart });
    const card = screen.getByTestId("kanban-card");
    fireEvent(card, pointer("pointerdown", 10, 10));
    fireEvent(card, pointer("pointerdown", 10, 10));
    fireEvent(document, pointer("pointermove", 10 + BOARD_DRAG_THRESHOLD_PX, 10));
    expect(onDragStart).toHaveBeenCalledTimes(1);
  });

  it("stops tracking on release, cancel and unmount", () => {
    const onDragStart = vi.fn();
    const first = setup({ draggable: true, onDragStart });
    const card = screen.getByTestId("kanban-card");
    fireEvent(card, pointer("pointerdown", 10, 10));
    fireEvent(card, pointer("pointerup", 10, 10));
    fireEvent(document, pointer("pointermove", 80, 80));
    fireEvent(card, pointer("pointerdown", 10, 10));
    fireEvent(card, pointer("pointercancel", 10, 10));
    fireEvent(document, pointer("pointermove", 80, 80));
    fireEvent(card, pointer("pointerdown", 10, 10));
    fireEvent(document, pointer("pointerup", 10, 10));
    fireEvent(document, pointer("pointermove", 80, 80));
    expect(onDragStart).not.toHaveBeenCalled();
    fireEvent(card, pointer("pointerdown", 10, 10));
    first.unmount();
    fireEvent(document, pointer("pointermove", 80, 80));
    expect(onDragStart).not.toHaveBeenCalled();
  });

  it("kills the native link drag instead of arming the board's", () => {
    const onDragStart = vi.fn();
    setup({ draggable: true, onDragStart });
    const link = screen.getByTestId("kanban-card-link");
    expect(link).toHaveAttribute("draggable", "false");
    fireEvent(link, pointer("pointerdown", 10, 10));
    const event = fireEvent.dragStart(link);
    expect(event).toBe(false);
    fireEvent(document, pointer("pointermove", 80, 80));
    expect(onDragStart).not.toHaveBeenCalled();
  });

  it("marks the drag source and the dropped card", () => {
    const scrolled = vi.fn();
    window.HTMLElement.prototype.scrollIntoView = scrolled;
    const first = setup({ draggable: true, dragging: true });
    const card = screen.getByTestId("kanban-card");
    expect(card).toHaveAttribute("data-dragging", "true");
    expect(card.className).toContain("opacity-50");
    first.unmount();
    setup({ dropped: true });
    expect(screen.getByTestId("kanban-card").className).toContain("ring-(--accent)");
    expect(scrolled).toHaveBeenCalledWith({ block: "nearest" });
  });
});

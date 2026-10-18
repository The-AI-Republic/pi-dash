// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IssueListItem } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  CALENDAR_BLOCK_PREVIEW_DELAY_MS,
  CALENDAR_DRAG_THRESHOLD_PX,
  CalendarBlock,
  blockIdentifier,
  previewDateLine,
  stateGroupIcon,
} from "./CalendarBlock.js";

const ISSUE_ID = "323e4567-e89b-12d3-a456-426614174002";
const STATE_ID = "423e4567-e89b-12d3-a456-426614174003";

function row(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE_ID,
    name: "Fix login",
    state_id: STATE_ID,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 42,
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    parent_id: null,
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
    ...overrides,
  };
}

function renderBlock(
  issue = row(),
  highlighted = false,
  overrides: Partial<React.ComponentProps<typeof CalendarBlock>> = {}
) {
  const handlers = {
    onOpen: vi.fn(),
    onEdit: vi.fn(),
    onDelete: vi.fn(),
    onDragStart: vi.fn(),
  };
  render(
    <CalendarBlock
      issue={issue}
      workspaceSlug="acme"
      projectIdentifier="WEB"
      stateColor="#00ff00"
      highlighted={highlighted}
      draggable
      quickActions
      {...handlers}
      {...overrides}
    />
  );
  return handlers;
}

function pointerEvent(type: string, clientX: number, clientY: number): MouseEvent {
  return new MouseEvent(type, { bubbles: true, cancelable: true, clientX, clientY });
}

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("CalendarBlock", () => {
  it("shows the identifier, title, state edge, and deep-links on click", async () => {
    const handlers = renderBlock();
    const user = userEvent.setup();
    const link = screen.getByRole("link", { name: "WEB-42" });
    expect(link).toHaveAttribute("id", `issue-${ISSUE_ID}`);
    expect(link).toHaveAttribute("href", "/acme/browse/WEB-42");
    expect(link).toHaveTextContent("WEB-42");
    expect(link).toHaveTextContent("Fix login");
    expect(link.querySelector("span[aria-hidden='true']")).toHaveStyle({ backgroundColor: "#00ff00" });
    expect(blockIdentifier("WEB", row())).toBe("WEB-42");
    await user.click(link);
    expect(handlers.onOpen).toHaveBeenCalledWith(expect.objectContaining({ id: ISSUE_ID }));
  });

  it("reveals the preview after the hover delay and closes on leave", () => {
    vi.useFakeTimers();
    renderBlock();
    const link = screen.getByRole("link", { name: "WEB-42" });
    fireEvent.pointerEnter(link);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    act(() => vi.advanceTimersByTime(CALENDAR_BLOCK_PREVIEW_DELAY_MS));
    expect(screen.getByRole("tooltip")).toHaveTextContent("Fix login");
    fireEvent.pointerLeave(link);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("shows state, priority and dates in the hover preview", () => {
    vi.useFakeTimers();
    renderBlock(row({ priority: "high", start_date: "2026-10-01", target_date: "2026-10-20" }), false, {
      stateName: "In progress",
      stateGroup: "started",
    });
    fireEvent.pointerEnter(screen.getByRole("link", { name: "WEB-42" }));
    act(() => vi.advanceTimersByTime(CALENDAR_BLOCK_PREVIEW_DELAY_MS));
    const card = screen.getByRole("tooltip");
    expect(card).toHaveTextContent("In progress");
    expect(card).toHaveTextContent("high");
    expect(card).toHaveTextContent("2026-10-01 → 2026-10-20");
  });

  it("describes partial and absent date lines", () => {
    expect(previewDateLine(row({ start_date: "2026-10-01", target_date: null }))).toBe("Starts 2026-10-01");
    expect(previewDateLine(row({ start_date: null, target_date: "2026-10-20" }))).toBe("Due 2026-10-20");
    expect(previewDateLine(row({ start_date: null, target_date: null }))).toBeNull();
  });

  it("maps every state group to an icon", () => {
    for (const group of ["backlog", "unstarted", "started", "review", "test", "completed", "cancelled", null]) {
      expect(stateGroupIcon(group)).not.toBeNull();
    }
  });

  it("opens Edit and Delete from the hover quick actions", async () => {
    const handlers = renderBlock();
    fireEvent.click(screen.getByRole("button", { name: "Quick actions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit" }));
    expect(handlers.onEdit).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Quick actions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(handlers.onDelete).toHaveBeenCalledOnce();
  });

  it("starts a pointer drag after movement and marks a highlighted temp row", () => {
    const issue = row({ id: "temp-abc", name: "Draft" });
    const handlers = renderBlock(issue, true);
    const link = screen.getByRole("link", { name: "Draft, saving" });
    expect(link).toHaveClass("highlight");
    expect(link).toHaveAttribute("data-temprow", "true");
    expect(link.querySelector(".animate-pulse")).not.toBeNull();
    fireEvent(link, pointerEvent("pointerdown", 10, 10));
    fireEvent(link, pointerEvent("pointermove", 10 + CALENDAR_DRAG_THRESHOLD_PX, 10));
    expect(handlers.onDragStart).toHaveBeenCalledWith(issue, expect.anything());
  });

  it("keeps tracking a drag after the pointer leaves the block", () => {
    const handlers = renderBlock();
    fireEvent(screen.getByRole("link", { name: "WEB-42" }), pointerEvent("pointerdown", 10, 10));
    fireEvent(document, pointerEvent("pointermove", 10 + CALENDAR_DRAG_THRESHOLD_PX, 10));
    expect(handlers.onDragStart).toHaveBeenCalledWith(expect.objectContaining({ id: ISSUE_ID }), expect.anything());
  });

  it("stops document tracking after pointer release", () => {
    const handlers = renderBlock();
    fireEvent(screen.getByRole("link", { name: "WEB-42" }), pointerEvent("pointerdown", 10, 10));
    fireEvent(document, pointerEvent("pointerup", 10, 10));
    fireEvent(document, pointerEvent("pointermove", 80, 80));
    expect(handlers.onDragStart).not.toHaveBeenCalled();
  });

  it("does not start a drag for a click without movement", () => {
    const handlers = renderBlock();
    const link = screen.getByRole("link", { name: "WEB-42" });
    fireEvent(link, pointerEvent("pointerdown", 10, 10));
    fireEvent(link, pointerEvent("pointermove", 11, 11));
    fireEvent(link, pointerEvent("pointerup", 11, 11));
    fireEvent.click(link);
    expect(handlers.onDragStart).not.toHaveBeenCalled();
    expect(handlers.onOpen).toHaveBeenCalledOnce();
  });

  it("keeps a click that follows a drag from opening peek", () => {
    const handlers = renderBlock();
    const link = screen.getByRole("link", { name: "WEB-42" });
    fireEvent(link, pointerEvent("pointerdown", 10, 10));
    fireEvent(link, pointerEvent("pointermove", 30, 30));
    fireEvent(link, pointerEvent("pointerup", 30, 30));
    fireEvent.click(link);
    expect(handlers.onDragStart).toHaveBeenCalledOnce();
    expect(handlers.onOpen).not.toHaveBeenCalled();
  });

  it("cancels a native link drag and stops tracking it", () => {
    const handlers = renderBlock();
    const link = screen.getByRole("link", { name: "WEB-42" });
    fireEvent(link, pointerEvent("pointerdown", 10, 10));
    const nativeDrag = new Event("dragstart", { bubbles: true, cancelable: true });
    fireEvent(link, nativeDrag);
    expect(nativeDrag.defaultPrevented).toBe(true);
    fireEvent(document, pointerEvent("pointermove", 40, 10));
    expect(handlers.onDragStart).not.toHaveBeenCalled();

    fireEvent(link, pointerEvent("pointerdown", 10, 10));
    fireEvent(document, pointerEvent("pointermove", 40, 10));
    expect(handlers.onDragStart).toHaveBeenCalledOnce();
    fireEvent(link, new Event("dragend", { bubbles: true }));
    fireEvent.click(link);
    expect(handlers.onOpen).toHaveBeenCalledOnce();
  });

  it("does not start a drag when the block is not draggable", () => {
    const handlers = renderBlock(row(), false, { draggable: false });
    const link = screen.getByRole("link", { name: "WEB-42" });
    fireEvent(link, pointerEvent("pointerdown", 10, 10));
    fireEvent(link, pointerEvent("pointermove", 40, 40));
    expect(handlers.onDragStart).not.toHaveBeenCalled();
  });

  it("hides quick actions for read-only viewers", () => {
    renderBlock(row(), false, { quickActions: false });
    expect(screen.queryByRole("button", { name: "Quick actions" })).not.toBeInTheDocument();
  });

  it("omits the state edge colour and handles focus previews", () => {
    renderBlock(row({ state_id: null }));
    const link = screen.getByRole("link", { name: "WEB-42" });
    fireEvent.focus(link);
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
    fireEvent.blur(link);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("flips the preview above and to the end near the viewport edge", () => {
    const rect = { top: 700, bottom: 760, left: 800, right: 900 } as DOMRect;
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(rect);
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 800 });
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1000 });
    renderBlock();
    fireEvent.focus(screen.getByRole("link", { name: "WEB-42" }));
    expect(screen.getByRole("tooltip")).toHaveAttribute("data-placement", "top-end");
    expect(screen.getByRole("tooltip")).toHaveClass("bottom-full", "right-0");
  });
});

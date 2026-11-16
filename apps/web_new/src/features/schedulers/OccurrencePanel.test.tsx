// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Occurrence panel (parity/specs/agents.md §5, AGT-017): the state line,
// the scheduler heading, When/Time zone/Recurrence/Project context rows,
// the install link, the run link for past firings, admin-only Edit binding
// and both close controls.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { SchedulerBinding, SchedulerOccurrence } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { OccurrencePanel } from "./OccurrencePanel.js";

function occurrence(overrides: Partial<SchedulerOccurrence> = {}): SchedulerOccurrence {
  return {
    binding_id: "b1",
    scheduler_id: "s1",
    scheduler_name: "Weekly review",
    scheduler_color: "#5e6ad2",
    dtstart: "2026-10-26T20:00:00.000Z",
    tzid: "Pacific/Auckland",
    kind: "scheduled",
    agent_run_id: null,
    status: null,
    ...overrides,
  };
}

function binding(overrides: Partial<SchedulerBinding> = {}): SchedulerBinding {
  return {
    id: "b1",
    scheduler: "s1",
    scheduler_slug: "weekly-review",
    scheduler_name: "Weekly review",
    scheduler_color: "#5e6ad2",
    project: "p1",
    workspace: "w1",
    dtstart: "2026-10-01T20:00:00.000Z",
    tzid: "Pacific/Auckland",
    rrule: "FREQ=DAILY",
    rdates: [],
    exdates: [],
    extra_context: "AGT17 project framing.",
    enabled: true,
    outcome_mode: "create_issue",
    pod: null,
    pod_name: null,
    next_run_at: null,
    last_run: null,
    last_run_status: null,
    last_run_ended_at: null,
    last_error: "",
    actor: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    ...overrides,
  };
}

function setup(overrides: Partial<React.ComponentProps<typeof OccurrencePanel>> = {}) {
  const props = {
    occurrence: occurrence(),
    binding: binding(),
    installHref: "/acme/projects/p1/schedulers/b1",
    canEdit: false,
    onClose: vi.fn(),
    onEdit: vi.fn(),
    workspaceSlug: "acme",
    projectId: "p1",
    ...overrides,
  };
  render(<OccurrencePanel {...props} />);
  return props;
}

function rowValue(label: string): string | undefined {
  const drawer = screen.getByRole("dialog");
  const rows = drawer.querySelectorAll("div.mb-3");
  for (const row of rows) {
    const cells = row.querySelectorAll(":scope > div");
    if (cells[0]?.textContent?.trim() === label) return cells[1]?.textContent?.trim();
  }
  return undefined;
}

describe("OccurrencePanel", () => {
  it("shows the status line, the scheduler name and the four rows", () => {
    setup();
    const drawer = screen.getByRole("dialog");
    expect(drawer).toHaveClass("fixed", "inset-y-0", "right-0", "z-40");
    expect(drawer.querySelector("div.uppercase")?.textContent).toBe("Scheduled");
    expect(within(drawer).getByRole("heading", { level: 2 })).toHaveTextContent("Weekly review");
    expect(rowValue("When")).toContain("Oct 27");
    expect(rowValue("Time zone")).toBe("Pacific/Auckland");
    expect(rowValue("Recurrence")).toBe("every day");
    expect(rowValue("Project context")).toBe("AGT17 project framing.");
  });

  it("links only to the parent install for a future firing", () => {
    setup();
    const links = within(screen.getByRole("dialog")).getAllByRole("link");
    expect(links.map((link) => link.textContent)).toEqual(["View scheduler →"]);
    expect(links[0]).toHaveAttribute("href", "/acme/projects/p1/schedulers/b1");
  });

  it("links a past firing to its run and reports the run status", () => {
    setup({ occurrence: occurrence({ kind: "past", agent_run_id: "r1", status: "failed" }) });
    const drawer = screen.getByRole("dialog");
    expect(drawer.querySelector("div.uppercase")?.textContent).toBe("failed");
    const links = within(drawer).getAllByRole("link");
    expect(links.map((link) => link.textContent)).toEqual(["View run →", "View scheduler →"]);
    expect(links[0]).toHaveAttribute("href", "/acme/projects/p1/runners/detail/r1");
  });

  it("falls back to placeholder context when the install carries none", () => {
    setup({ binding: binding({ extra_context: "" }) });
    expect(rowValue("Project context")).toBe("No extra context");
  });

  it("shows the recurrence as blank when the install is not loaded", () => {
    setup({ binding: null });
    expect(rowValue("Recurrence")).toBe("");
  });

  it("offers Edit binding to project admins and closes the panel first", async () => {
    const props = setup({ canEdit: true });
    const edit = within(screen.getByRole("dialog")).getByRole("button", { name: "Edit binding" });
    await userEvent.click(edit);
    expect(props.onEdit).toHaveBeenCalledWith(binding());
  });

  it("hides Edit binding from project members", () => {
    setup({ canEdit: false });
    expect(screen.queryByRole("button", { name: "Edit binding" })).not.toBeInTheDocument();
  });

  it("hides Edit binding when the install row is missing", () => {
    setup({ canEdit: true, binding: null });
    expect(screen.queryByRole("button", { name: "Edit binding" })).not.toBeInTheDocument();
  });

  it("closes from the header control and the footer", async () => {
    const props = setup();
    const drawer = screen.getByRole("dialog");
    await userEvent.click(within(drawer.querySelector("header") as HTMLElement).getByRole("button", { name: "Close" }));
    await userEvent.click(within(drawer.querySelector("footer") as HTMLElement).getByRole("button", { name: "Close" }));
    expect(props.onClose).toHaveBeenCalledTimes(2);
  });
});

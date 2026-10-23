// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { IntakeIssueRow, WorkspaceMemberList } from "@pidash/api-client";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { INTAKE_IDENTITY_EMAIL } from "../lib/identity.js";
import { IntakeRow } from "./IntakeRow.js";

function row(overrides: Partial<IntakeIssueRow> = {}, issueOverrides = {}): IntakeIssueRow {
  return {
    id: "intake-i-1",
    status: -2,
    duplicate_to: null,
    snoozed_till: null,
    source: null,
    created_by: null,
    issue: {
      id: "i-1",
      name: "Fix the intake list",
      priority: "urgent",
      sequence_id: 7,
      project_id: "223e4567-e89b-12d3-a456-426614174001",
      created_at: "2026-10-01T00:00:00Z",
      label_ids: [],
      created_by: null,
      ...issueOverrides,
    },
    ...overrides,
  };
}

function directoryEntry(userId: string, displayName: string, email: string | undefined) {
  return {
    id: `workspace-${userId}`,
    member: {
      id: userId,
      first_name: displayName,
      last_name: "",
      avatar: "",
      avatar_url: "https://example.com/avatar.png",
      display_name: displayName,
      is_bot: false,
      ...(email === undefined ? {} : { email }),
    },
    role: 15,
    is_active: true,
  };
}

const DIRECTORY: WorkspaceMemberList = [
  directoryEntry("user-member", "Rin", "rin@example.com"),
  directoryEntry("user-system", "Intake", INTAKE_IDENTITY_EMAIL),
];

const LABELS = new Map([
  ["l-1", { name: "Alpha", color: "#ff0000" }],
  ["l-2", { name: "Beta", color: "#00ff00" }],
  ["l-3", { name: "Gamma", color: "#0000ff" }],
  ["l-4", { name: "Delta", color: "#ff00ff" }],
]);

const BASE = {
  projectIdentifier: "N256",
  labelById: LABELS,
  directory: DIRECTORY,
  selected: false,
  onSelect: () => {},
};

describe("IntakeRow (INT-006)", () => {
  it("reads the id, title and created date at a glance", () => {
    render(<IntakeRow {...BASE} row={row()} />);
    expect(screen.getByTestId("intake-row-id")).toHaveTextContent("N256-7");
    const title = screen.getByTestId("intake-row-title");
    expect(title).toHaveTextContent("Fix the intake list");
    expect(title).toHaveAttribute("title", "Fix the intake list");
    expect(screen.getByTestId("intake-row-date")).toHaveTextContent("Oct 1, 2026");
  });

  it("names the row by identifier, title and status", () => {
    render(<IntakeRow {...BASE} row={row({ status: -1 })} />);
    expect(screen.getByTestId("intake-row")).toHaveAttribute("aria-label", "N256-7 Fix the intake list, Declined");
  });

  it("names an unknown status without a chip", () => {
    render(<IntakeRow {...BASE} row={row({ status: 99 })} />);
    expect(screen.getByTestId("intake-row")).toHaveAttribute("aria-label", "N256-7 Fix the intake list, Unknown");
    expect(screen.queryByTestId("intake-row-status")).not.toBeInTheDocument();
  });

  it("hides the status chip while pending", () => {
    render(<IntakeRow {...BASE} row={row({ status: -2 })} />);
    expect(screen.queryByTestId("intake-row-status")).not.toBeInTheDocument();
  });

  it.each([
    [-1, "Declined"],
    [0, "Snoozed"],
    [1, "Accepted"],
    [2, "Duplicate"],
  ])("chips a %s row as %s", (status, label) => {
    render(<IntakeRow {...BASE} row={row({ status })} />);
    expect(screen.getByTestId("intake-row-status")).toHaveTextContent(label);
  });

  it.each([
    ["urgent", "Priority: Urgent"],
    ["high", "Priority: High"],
    ["medium", "Priority: Medium"],
    ["low", "Priority: Low"],
    ["none", "Priority: None"],
  ] as const)("marks the %s priority with its key", (priority, name) => {
    render(<IntakeRow {...BASE} row={row({}, { priority })} />);
    const marker = screen.getByTestId("intake-row-priority");
    expect(marker).toHaveAttribute("data-priority-key", priority);
    expect(marker).toHaveAttribute("aria-label", name);
  });

  it("shows no labels without carrying any", () => {
    render(<IntakeRow {...BASE} row={row()} />);
    expect(screen.queryByTestId("intake-row-labels")).not.toBeInTheDocument();
  });

  it("shows no labels when the annotation is absent", () => {
    render(<IntakeRow {...BASE} row={row({}, { label_ids: undefined })} />);
    expect(screen.queryByTestId("intake-row-labels")).not.toBeInTheDocument();
  });

  it("chips up to three labels by name", () => {
    render(<IntakeRow {...BASE} row={row({}, { label_ids: ["l-1", "l-2"] })} />);
    expect(screen.getAllByTestId("intake-row-label").map((node) => node.textContent)).toEqual(["Alpha", "Beta"]);
  });

  it("collapses four or more labels to one count chip", () => {
    render(<IntakeRow {...BASE} row={row({}, { label_ids: ["l-1", "l-2", "l-3", "l-4"] })} />);
    const chips = screen.getAllByTestId("intake-row-label");
    expect(chips).toHaveLength(1);
    expect(chips[0]).toHaveTextContent("4 labels");
  });

  it("skips labels the project no longer knows", () => {
    render(<IntakeRow {...BASE} row={row({}, { label_ids: ["l-1", "l-gone"] })} />);
    expect(screen.getAllByTestId("intake-row-label").map((node) => node.textContent)).toEqual(["Alpha"]);
  });

  it("shows no avatar without a known creator", () => {
    render(<IntakeRow {...BASE} row={row()} />);
    expect(screen.queryByTestId("intake-row-avatar")).not.toBeInTheDocument();
    render(<IntakeRow {...BASE} row={row({}, { created_by: "user-unknown" })} />);
    expect(screen.queryAllByTestId("intake-row-avatar")).toHaveLength(0);
  });

  it("shows no avatar while the directory loads", () => {
    render(<IntakeRow {...BASE} directory={undefined} row={row({}, { created_by: "user-member" })} />);
    expect(screen.queryByTestId("intake-row-avatar")).not.toBeInTheDocument();
  });

  it("shows the member avatar for member-authored rows", () => {
    render(<IntakeRow {...BASE} row={row({}, { created_by: "user-member" })} />);
    expect(screen.getByTestId("intake-row-avatar")).toHaveAttribute("data-avatar-kind", "member");
  });

  it("shows the intake identity for system-authored rows", () => {
    render(<IntakeRow {...BASE} row={row({}, { created_by: "user-system" })} />);
    expect(screen.getByTestId("intake-row-avatar")).toHaveAttribute("data-avatar-kind", "intake");
  });

  it("marks the selected row and reports selection", () => {
    const onSelect = vi.fn();
    render(<IntakeRow {...BASE} row={row()} selected onSelect={onSelect} />);
    const node = screen.getByTestId("intake-row");
    expect(node).toHaveAttribute("aria-current", "true");
    expect(node).toHaveAttribute("data-issue-id", "i-1");
    fireEvent.click(node);
    expect(onSelect).toHaveBeenCalledWith("i-1");
  });

  it("leaves an unselected row unmarked", () => {
    render(<IntakeRow {...BASE} row={row()} />);
    expect(screen.getByTestId("intake-row")).not.toHaveAttribute("aria-current");
  });
});

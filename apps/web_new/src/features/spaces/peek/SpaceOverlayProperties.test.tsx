// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceBoardIssueDetail, SpaceBoardState } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { boardIssue, STATES, TODO_ID } from "../layouts/testFixtures.js";
import { SpaceOverlayProperties, spaceOverlayPriority, spaceOverlayState } from "./SpaceOverlayProperties.js";

const DONE_ID = "33333333-3333-4333-8333-333333333333";

function detail(overrides: Partial<SpaceBoardIssueDetail> = {}): SpaceBoardIssueDetail {
  return {
    ...boardIssue(7, TODO_ID, "Public issue"),
    description_json: null,
    description_html: "<p>Words</p>",
    description_stripped: "Words",
    description_binary: null,
    ...overrides,
  };
}

function renderPanel(issue: SpaceBoardIssueDetail, states: readonly SpaceBoardState[] = STATES) {
  return render(<SpaceOverlayProperties issue={issue} states={states} />);
}

describe("SpaceOverlayProperties (SPC-044)", () => {
  it("shows state, priority and due date with their labels", () => {
    renderPanel(detail({ priority: "high", target_date: "2030-06-30" }));
    expect(screen.getByText("State")).toBeInTheDocument();
    expect(screen.getByText("Todo")).toBeInTheDocument();
    expect(screen.getByText("Priority")).toBeInTheDocument();
    expect(screen.getByText("High")).toBeInTheDocument();
    expect(screen.getByText("Due date")).toBeInTheDocument();
    expect(screen.getByTestId("space-peek-due")).toHaveTextContent("Jun 30, 2030");
    expect(screen.getByTestId("space-peek-due")).not.toHaveAttribute("data-overdue");
  });

  it("emphasises a past due date on an open issue", () => {
    renderPanel(detail({ target_date: "2020-01-15" }));
    const due = screen.getByTestId("space-peek-due");
    expect(due).toHaveTextContent("Jan 15, 2020");
    expect(due).toHaveAttribute("data-overdue", "true");
  });

  it("never emphasises a completed issue's past date", () => {
    renderPanel(detail({ state_id: DONE_ID, target_date: "2020-01-15" }));
    expect(screen.getByText("Done")).toBeInTheDocument();
    expect(screen.getByTestId("space-peek-due")).not.toHaveAttribute("data-overdue");
  });

  it("prints the explicit empty marker for a missing due date and priority", () => {
    renderPanel(detail({ priority: null, target_date: null }));
    expect(screen.getByTestId("space-peek-due-empty")).toHaveTextContent("Empty");
    expect(screen.getAllByText("None")).toHaveLength(1);
  });

  it("falls back to the empty state marker when the states fetch has no match", () => {
    renderPanel(detail({ state_id: null }));
    expect(screen.getByTestId("space-peek-properties")).toHaveTextContent("Empty");
  });

  it("resolves a state by id and defaults an unset priority to none", () => {
    expect(spaceOverlayState(STATES, TODO_ID)?.name).toBe("Todo");
    expect(spaceOverlayState(STATES, null)).toBeNull();
    expect(spaceOverlayState(STATES, "ffffffff-ffff-4fff-8fff-ffffffffffff")).toBeNull();
    expect(spaceOverlayPriority(null)).toBe("none");
    expect(spaceOverlayPriority("urgent")).toBe("urgent");
  });
});

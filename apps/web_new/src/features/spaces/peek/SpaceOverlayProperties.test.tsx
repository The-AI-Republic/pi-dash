// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceBoardIssueDetail, SpaceBoardState } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { LANGUAGE_STORAGE_KEY, registerLocale, resetLocalesForTests } from "../../../core/i18n/locales.js";
import { boardIssue, STATES, TODO_ID } from "../layouts/testFixtures.js";
import { registerSpaceMessages, resetSpaceMessagesForTests } from "../lib/spaceMessages.js";
import { SpaceOverlayProperties, spaceOverlayPriority, spaceOverlayState } from "./SpaceOverlayProperties.js";

const DONE_ID = "33333333-3333-4333-8333-333333333333";

afterEach(() => {
  globalThis.localStorage?.clear();
  resetSpaceMessagesForTests();
  resetLocalesForTests();
});

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

/** Render before the board's states fetch has produced anything. */
function renderPendingStates(issue: SpaceBoardIssueDetail) {
  return render(<SpaceOverlayProperties issue={issue} states={undefined} />);
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

  it("renders priority as a marker chip carrying the translated label (SPC-044)", () => {
    registerLocale({ tag: "fr", label: "Francais" });
    globalThis.localStorage.setItem(LANGUAGE_STORAGE_KEY, "fr");
    registerSpaceMessages("fr", { "filters.priority.high": "Haute" });
    renderPanel(detail({ priority: "high" }));
    const chip = screen.getByTestId("space-peek-priority");
    expect(chip).toHaveAttribute("data-level", "high");
    expect(chip).toContainElement(screen.getByTestId("space-priority-marker"));
    expect(chip).toHaveTextContent("Haute");
    expect(screen.queryByText("High")).not.toBeInTheDocument();
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

  it("holds the state row pending while the states fetch is unknown (SPC-043/046)", () => {
    renderPendingStates(detail({ state_id: TODO_ID }));
    const stateRow = screen.getByTestId("space-peek-properties").querySelector('[data-property="state"]');
    expect(stateRow).not.toBeNull();
    expect(screen.getByTestId("space-peek-state-pending")).toBeInTheDocument();
    expect(stateRow).not.toHaveTextContent("Empty");
    expect(stateRow).not.toHaveTextContent("Todo");
  });

  it("prints the empty state marker only once the fetch settles without the id", () => {
    renderPanel(detail({ state_id: "ffffffff-ffff-4fff-8fff-ffffffffffff" }), STATES);
    const stateRow = screen.getByTestId("space-peek-properties").querySelector('[data-property="state"]');
    expect(screen.queryByTestId("space-peek-state-pending")).not.toBeInTheDocument();
    expect(stateRow).toHaveTextContent("Empty");
  });

  it("keeps a settled null state empty even while the states fetch is unknown", () => {
    renderPendingStates(detail({ state_id: null }));
    expect(screen.queryByTestId("space-peek-state-pending")).not.toBeInTheDocument();
    expect(screen.getByTestId("space-peek-properties").querySelector('[data-property="state"]')).toHaveTextContent(
      "Empty"
    );
  });

  it("resolves a state by id and defaults an unset priority to none", () => {
    expect(spaceOverlayState(STATES, TODO_ID)?.name).toBe("Todo");
    expect(spaceOverlayState(STATES, null)).toBeNull();
    expect(spaceOverlayState(STATES, "ffffffff-ffff-4fff-8fff-ffffffffffff")).toBeNull();
    expect(spaceOverlayPriority(null)).toBe("none");
    expect(spaceOverlayPriority("urgent")).toBe("urgent");
  });
});

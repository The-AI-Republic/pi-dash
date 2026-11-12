// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The project list's lazy empty-card boundary (ISS-068): the route awaits
// the chunk, the component then follows the visit rule for the card, and
// calling it before the chunk resolves fails loudly.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { loadProjectEmptyState, ProjectIssuesEmptyState } from "./lazy.js";
import { NO_MATCH_TITLE } from "./cards.js";

const FILTERED = { hasCompleteCondition: true } as const;

describe("project empty-state boundary", () => {
  it("refuses to render before the chunk resolves", () => {
    const spies = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(() => render(<ProjectIssuesEmptyState {...props()} />)).toThrow(/await loadProjectEmptyState/);
    spies.mockRestore();
  });

  it("loads the chunk and shows the first-work-item card", async () => {
    await loadProjectEmptyState();
    render(<ProjectIssuesEmptyState {...props()} />);
    expect(screen.getByRole("heading", { name: /^Start with your first work item\./ })).toBeInTheDocument();
  });

  it("shows the no-results card once a condition was added this visit", async () => {
    await loadProjectEmptyState();
    render(<ProjectIssuesEmptyState {...props({ ...FILTERED, addedThisVisit: true })} />);
    expect(screen.getByTestId("issues-empty-state")).toHaveAttribute("data-context", "project-filtered");
    expect(screen.getByText(NO_MATCH_TITLE)).toBeInTheDocument();
  });

  it("keeps the first-work-item card for a stored-only expression", async () => {
    await loadProjectEmptyState();
    render(<ProjectIssuesEmptyState {...props({ ...FILTERED, addedThisVisit: false })} />);
    expect(screen.getByRole("heading", { name: /^Start with your first work item\./ })).toBeInTheDocument();
  });
});

function props(overrides: Partial<React.ComponentProps<typeof ProjectIssuesEmptyState>> = {}) {
  return {
    hasCompleteCondition: false,
    addedThisVisit: false,
    visibleCount: 0,
    canAct: true,
    onClear: () => {},
    onCreate: () => {},
    ...overrides,
  };
}

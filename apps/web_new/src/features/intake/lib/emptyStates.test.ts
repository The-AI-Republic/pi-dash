// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { selectIntakeEmptyState } from "./emptyStates.js";

describe("selectIntakeEmptyState (INT-008, bug NEWFRONT-262)", () => {
  it("guides an untouched empty Open tab", () => {
    expect(selectIntakeEmptyState({ tab: "open", rowCount: 0, userFilterCount: 0, statusNarrowed: false })).toBe(
      "open-guidance"
    );
  });

  it("informs an untouched empty Closed tab", () => {
    expect(selectIntakeEmptyState({ tab: "closed", rowCount: 0, userFilterCount: 0, statusNarrowed: false })).toBe(
      "closed-informational"
    );
  });

  it("shows no-results when the user added a filter", () => {
    expect(selectIntakeEmptyState({ tab: "open", rowCount: 0, userFilterCount: 1, statusNarrowed: false })).toBe(
      "filtered"
    );
    expect(selectIntakeEmptyState({ tab: "closed", rowCount: 0, userFilterCount: 2, statusNarrowed: false })).toBe(
      "filtered"
    );
  });

  it("shows no-results when the status selection left the tab default", () => {
    expect(selectIntakeEmptyState({ tab: "open", rowCount: 0, userFilterCount: 0, statusNarrowed: true })).toBe(
      "filtered"
    );
  });
});

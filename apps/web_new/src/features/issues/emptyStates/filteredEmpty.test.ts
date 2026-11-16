// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The filtered-empty rule (parity/specs/issues.md §8, ISS-068–071): an
// empty list offers clearing only when its expression holds a complete
// condition and the context counts that expression as a search. The
// completeness answer arrives from the caller, so that the parser stays
// out of this module's import graph (Quality gates, NEWFRONT-637).
import { describe, expect, it } from "vitest";

import { isFilteredEmpty } from "./filteredEmpty.js";

describe("isFilteredEmpty", () => {
  it("offers clearing when a stored expression empties the list", () => {
    expect(
      isFilteredEmpty({ mode: "stored", hasCompleteCondition: true, addedThisVisit: false, visibleCount: 0 })
    ).toBe(true);
  });

  it("offers clearing on a visit only after a condition was added", () => {
    expect(isFilteredEmpty({ mode: "visit", hasCompleteCondition: true, addedThisVisit: false, visibleCount: 0 })).toBe(
      false
    );
    expect(isFilteredEmpty({ mode: "visit", hasCompleteCondition: true, addedThisVisit: true, visibleCount: 0 })).toBe(
      true
    );
  });

  it("never reports a filtered list that still has rows", () => {
    expect(isFilteredEmpty({ mode: "stored", hasCompleteCondition: true, addedThisVisit: true, visibleCount: 3 })).toBe(
      false
    );
  });

  it("treats an empty or half-built expression as no filter", () => {
    expect(
      isFilteredEmpty({ mode: "stored", hasCompleteCondition: false, addedThisVisit: true, visibleCount: 0 })
    ).toBe(false);
  });
});

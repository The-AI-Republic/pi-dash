// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail de-dupe shaping (ISS-231): the view's near-duplicate rows, the
// viewed item excluded, capped, and addressed by their own project key.
import type { IssueSearchHit } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { DUPLICATE_ROW_CAP, duplicateHref, duplicateRows } from "./dedupeModel.js";

function hit(id: string, sequence = 1, identifier = "WEB"): IssueSearchHit {
  return {
    id,
    name: `Item ${id}`,
    start_date: null,
    sequence_id: sequence,
    project__name: "Web",
    project__identifier: identifier,
    project_id: "323e4567-e89b-12d3-a456-426614174002",
    workspace__slug: "acme",
    state__name: "Todo",
    state__group: "unstarted",
    state__color: "#5e6ad2",
  };
}

describe("duplicateRows", () => {
  it("drops the viewed item and keeps the rest in order", () => {
    const rows = duplicateRows([hit("a"), hit("b"), hit("c")], "b");
    expect(rows.map((row) => row.id)).toEqual(["a", "c"]);
  });

  it("caps the list at the popover's row limit", () => {
    const many = Array.from({ length: DUPLICATE_ROW_CAP + 3 }, (_, index) => hit(`id-${index}`));
    expect(duplicateRows(many, "none")).toHaveLength(DUPLICATE_ROW_CAP);
  });

  it("leaves nothing when the only hit is the viewed item", () => {
    expect(duplicateRows([hit("a")], "a")).toEqual([]);
  });
});

describe("duplicateHref", () => {
  it("builds the browse address from the hit's own project key", () => {
    expect(duplicateHref("acme", hit("a", 42, "ATL"))).toBe("/acme/browse/ATL-42");
  });
});

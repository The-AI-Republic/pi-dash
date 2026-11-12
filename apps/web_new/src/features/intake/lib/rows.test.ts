// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { IntakeIssueRow } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { intakeRowIdLabel, uniqueIntakeRows } from "./rows.js";

function row(issueId: string, intakeId = `intake-${issueId}`): IntakeIssueRow {
  return {
    id: intakeId,
    status: -2,
    duplicate_to: null,
    snoozed_till: null,
    source: null,
    created_by: null,
    issue: {
      id: issueId,
      name: issueId,
      priority: "none",
      sequence_id: 7,
      project_id: "223e4567-e89b-12d3-a456-426614174001",
      created_at: "2026-10-01T00:00:00Z",
      label_ids: [],
      created_by: null,
    },
  };
}

describe("intake rows (INT-006/007)", () => {
  it("labels rows with the project prefix plus sequence number", () => {
    expect(intakeRowIdLabel("N256", 7)).toBe("N256-7");
  });

  it("keeps server order and drops replayed requests", () => {
    const first = row("i-1");
    const second = row("i-2");
    expect(uniqueIntakeRows([first, second, row("i-1", "intake-replay")])).toEqual([first, second]);
  });

  it("passes distinct rows through untouched", () => {
    expect(uniqueIntakeRows([])).toEqual([]);
    const only = row("i-1");
    expect(uniqueIntakeRows([only])).toEqual([only]);
  });
});

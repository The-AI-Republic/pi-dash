// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { tempIssueRow } from "../freshness/tempRows.js";
import { addPendingRow, removePendingRow } from "./pending.js";

describe("pending calendar rows", () => {
  it("adds a temp block to its day and keeps other days", () => {
    const first = addPendingRow({}, "2026-10-05", tempIssueRow("temp-a", "A"));
    const second = addPendingRow(first, "2026-10-06", tempIssueRow("temp-b", "B"));
    expect(second["2026-10-05"]?.map((row) => row.id)).toEqual(["temp-a"]);
    expect(second["2026-10-06"]?.map((row) => row.id)).toEqual(["temp-b"]);
  });

  it("ignores a duplicate temp id on the same day", () => {
    const row = tempIssueRow("temp-a", "A");
    const current = addPendingRow({}, "2026-10-05", row);
    expect(addPendingRow(current, "2026-10-05", row)).toBe(current);
  });

  it("removes the settled temp block and drops empty days", () => {
    const current = {
      "2026-10-05": [tempIssueRow("temp-a", "A"), tempIssueRow("temp-b", "B")],
      "2026-10-06": [tempIssueRow("temp-c", "C")],
    };
    const next = removePendingRow(current, "temp-a");
    expect(next["2026-10-05"]?.map((row) => row.id)).toEqual(["temp-b"]);
    expect(removePendingRow(next, "temp-b")).toEqual({ "2026-10-06": [expect.objectContaining({ id: "temp-c" })] });
  });

  it("returns the same map when the temp id is absent", () => {
    const current = { "2026-10-05": [tempIssueRow("temp-a", "A")] };
    expect(removePendingRow(current, "temp-missing")).toBe(current);
  });
});

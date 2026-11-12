// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer eligibility and destination options (parity/specs/cycles.md
// §12, CYC-031): the two gates with their snapshot difference, the
// open-only destination rule with search narrowing, and the pills.
import { describe, expect, it } from "vitest";

import {
  canTransferFromBanner,
  canTransferFromRow,
  isTransferableStatus,
  isTransferArchived,
  isTransferSnapshotEmpty,
  transferableWorkCount,
  transferStatusPill,
  transferTargetOptions,
  type TransferableCycle,
} from "./cycleTransfer.js";

function finished(overrides: Partial<TransferableCycle> = {}): TransferableCycle {
  return {
    status: "COMPLETED",
    archived_at: null,
    total_issues: 3,
    cancelled_issues: 0,
    completed_issues: 1,
    progress_snapshot: null,
    ...overrides,
  };
}

describe("isTransferableStatus", () => {
  it("matches the completed marker case-insensitively", () => {
    expect(isTransferableStatus("COMPLETED")).toBe(true);
    expect(isTransferableStatus("completed")).toBe(true);
    expect(isTransferableStatus("Completed")).toBe(true);
    expect(isTransferableStatus("current")).toBe(false);
    expect(isTransferableStatus(undefined)).toBe(false);
  });
});

describe("isTransferArchived", () => {
  it("reads the stamped row as archived", () => {
    expect(isTransferArchived({ archived_at: "2026-03-01T00:00:00Z" })).toBe(true);
    expect(isTransferArchived({ archived_at: null })).toBe(false);
    expect(isTransferArchived({})).toBe(false);
  });
});

describe("transferableWorkCount", () => {
  it("subtracts cancelled and completed from the total", () => {
    expect(transferableWorkCount(finished())).toBe(2);
    expect(transferableWorkCount(finished({ total_issues: 2, cancelled_issues: 1, completed_issues: 1 }))).toBe(0);
  });

  it("reads missing counters as zero and floors a negative residue", () => {
    expect(transferableWorkCount({ status: "completed" })).toBe(0);
    expect(transferableWorkCount(finished({ total_issues: 1, cancelled_issues: 1, completed_issues: 1 }))).toBe(0);
  });
});

describe("isTransferSnapshotEmpty", () => {
  it("treats absent, null and entry-less snapshots as empty", () => {
    expect(isTransferSnapshotEmpty(undefined)).toBe(true);
    expect(isTransferSnapshotEmpty(null)).toBe(true);
    expect(isTransferSnapshotEmpty({})).toBe(true);
    expect(isTransferSnapshotEmpty({ total_issues: 3 })).toBe(false);
  });
});

describe("canTransferFromRow", () => {
  it("opens for a finished live cycle holding work", () => {
    expect(canTransferFromRow(finished())).toBe(true);
  });

  it("closes for unfinished, archived, or drained cycles", () => {
    expect(canTransferFromRow(finished({ status: "current" }))).toBe(false);
    expect(canTransferFromRow(finished({ archived_at: "2026-03-01T00:00:00Z" }))).toBe(false);
    expect(canTransferFromRow(finished({ total_issues: 1, completed_issues: 1 }))).toBe(false);
    expect(canTransferFromRow(finished({ status: undefined }))).toBe(false);
  });

  it("ignores the snapshot", () => {
    expect(canTransferFromRow(finished({ progress_snapshot: { total_issues: 3 } }))).toBe(true);
  });
});

describe("canTransferFromBanner", () => {
  it("adds the empty-snapshot clause to the row gate", () => {
    expect(canTransferFromBanner(finished())).toBe(true);
    expect(canTransferFromBanner(finished({ progress_snapshot: {} }))).toBe(true);
    expect(canTransferFromBanner(finished({ progress_snapshot: { total_issues: 3 } }))).toBe(false);
    expect(canTransferFromBanner(finished({ status: "current" }))).toBe(false);
  });
});

describe("transferTargetOptions", () => {
  const cycles = [
    { id: "source", name: "Source", status: "COMPLETED" },
    { id: "a", name: "Alpha current", status: "CURRENT" },
    { id: "b", name: "Beta upcoming", status: "UPCOMING" },
    { id: "c", name: "Gamma draft", status: "DRAFT" },
    { id: "d", name: "Delta done", status: "COMPLETED" },
  ];

  it("offers only open cycles, never the source or a finished one", () => {
    expect(transferTargetOptions(cycles, "source", "").map((cycle) => cycle.id)).toEqual(["a", "b", "c"]);
  });

  it("narrows by name, case-insensitively, trimming the query", () => {
    expect(transferTargetOptions(cycles, "source", "beta").map((cycle) => cycle.id)).toEqual(["b"]);
    expect(transferTargetOptions(cycles, "source", "  ALPHA  ").map((cycle) => cycle.id)).toEqual(["a"]);
    expect(transferTargetOptions(cycles, "source", "zzz")).toEqual([]);
  });

  it("keeps rows with a missing marker", () => {
    const rows = [{ id: "x", name: "Unmarked" }, ...cycles];
    expect(transferTargetOptions(rows, "source", "").map((cycle) => cycle.id)).toEqual(["x", "a", "b", "c"]);
  });
});

describe("transferStatusPill", () => {
  it("capitalises the open markers and reads a missing one as draft", () => {
    expect(transferStatusPill("CURRENT")).toBe("Current");
    expect(transferStatusPill("upcoming")).toBe("Upcoming");
    expect(transferStatusPill("draft")).toBe("Draft");
    expect(transferStatusPill(undefined)).toBe("Draft");
    expect(transferStatusPill("mystery")).toBe("Draft");
  });
});

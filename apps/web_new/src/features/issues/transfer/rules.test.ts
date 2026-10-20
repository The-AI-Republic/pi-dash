// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  canTransferCycleIssues,
  isCompletedCycleStatus,
  showsCompletedCycleNote,
  transferableIssueCount,
  transferTargetOptions,
  type TransferableCycleFields,
} from "./rules.js";

const COMPLETED = { status: "COMPLETED", backlog_issues: 1 };

describe("isCompletedCycleStatus", () => {
  it("matches any casing and rejects other statuses", () => {
    expect(isCompletedCycleStatus("COMPLETED")).toBe(true);
    expect(isCompletedCycleStatus("completed")).toBe(true);
    expect(isCompletedCycleStatus("ACTIVE")).toBe(false);
    expect(isCompletedCycleStatus(undefined)).toBe(false);
  });
});

describe("transferableIssueCount", () => {
  it("sums backlog, unstarted and started, defaulting to zero", () => {
    expect(transferableIssueCount(undefined)).toBe(0);
    expect(transferableIssueCount({})).toBe(0);
    expect(transferableIssueCount({ backlog_issues: 2, unstarted_issues: 3, started_issues: 1 })).toBe(6);
  });
});

describe("canTransferCycleIssues", () => {
  it("offers a move on a completed cycle that still holds work", () => {
    expect(canTransferCycleIssues(COMPLETED)).toBe(true);
    expect(canTransferCycleIssues({ ...COMPLETED, unstarted_issues: 4 })).toBe(true);
  });

  it("refuses active cycles and cycles with no transferable work", () => {
    expect(canTransferCycleIssues({ ...COMPLETED, status: "ACTIVE" })).toBe(false);
    expect(canTransferCycleIssues({ status: "COMPLETED" })).toBe(false);
    expect(canTransferCycleIssues(null)).toBe(false);
  });

  it("keeps offering the move after a transfer froze the source counts", () => {
    // The oracle shows the source keeping its pre-move counts and the
    // action staying up; the snapshot is not part of the gate.
    const frozen: TransferableCycleFields & { progress_snapshot: Record<string, unknown> } = {
      ...COMPLETED,
      progress_snapshot: { total_issues: 1, backlog_issues: 1 },
    };
    expect(canTransferCycleIssues(frozen)).toBe(true);
  });
});

describe("showsCompletedCycleNote", () => {
  it("renders the note for completed cycles only", () => {
    expect(showsCompletedCycleNote(COMPLETED)).toBe(true);
    expect(showsCompletedCycleNote({ status: "ACTIVE" })).toBe(false);
    expect(showsCompletedCycleNote(undefined)).toBe(false);
  });
});

describe("transferTargetOptions", () => {
  const cycles = [
    { id: "source", name: "Source", status: "COMPLETED" },
    { id: "active", name: "Sprint 5", status: "ACTIVE" },
    { id: "draft", name: "Sprint 6", status: "DRAFT" },
    { id: "plain", name: "Backlog cycle" },
  ];

  it("lists incomplete cycles except the source", () => {
    expect(transferTargetOptions(cycles, "source", "").map((cycle) => cycle.id)).toEqual(["active", "draft", "plain"]);
  });

  it("narrows by a case-insensitive name search", () => {
    expect(transferTargetOptions(cycles, "source", "spring").map((cycle) => cycle.id)).toEqual([]);
    expect(transferTargetOptions(cycles, "source", "SPRINT").map((cycle) => cycle.id)).toEqual(["active", "draft"]);
    expect(transferTargetOptions(cycles, "active", "  backlog ").map((cycle) => cycle.id)).toEqual(["plain"]);
  });
});

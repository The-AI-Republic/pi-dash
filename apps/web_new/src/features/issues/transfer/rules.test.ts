// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  canTransferCycleIssues,
  hasEmptyProgressSnapshot,
  isCompletedCycleStatus,
  showsCompletedCycleNote,
  transferableIssueCount,
  transferFieldsOf,
  transferTargetOptions,
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

describe("hasEmptyProgressSnapshot", () => {
  it("treats absent, null and {} as empty", () => {
    expect(hasEmptyProgressSnapshot(undefined)).toBe(true);
    expect(hasEmptyProgressSnapshot(null)).toBe(true);
    expect(hasEmptyProgressSnapshot({})).toBe(true);
    expect(hasEmptyProgressSnapshot({ total_issues: 1 })).toBe(false);
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
  it("offers transfer on a completed cycle with empty snapshot and work left", () => {
    expect(canTransferCycleIssues(COMPLETED)).toBe(true);
    expect(canTransferCycleIssues({ ...COMPLETED, progress_snapshot: null })).toBe(true);
    expect(canTransferCycleIssues({ ...COMPLETED, progress_snapshot: {} })).toBe(true);
  });

  it("refuses active cycles and cycles with no transferable work", () => {
    expect(canTransferCycleIssues({ ...COMPLETED, status: "ACTIVE" })).toBe(false);
    expect(canTransferCycleIssues({ status: "COMPLETED" })).toBe(false);
    expect(canTransferCycleIssues(null)).toBe(false);
  });

  it("refuses after a transfer froze the snapshot", () => {
    expect(canTransferCycleIssues({ ...COMPLETED, progress_snapshot: { total_issues: 1 } })).toBe(false);
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

describe("transferFieldsOf", () => {
  it("narrows a detail row and passes undefined through", () => {
    expect(transferFieldsOf(undefined)).toBeNull();
    expect(
      transferFieldsOf({
        id: "c",
        project_id: "p",
        name: "C",
        start_date: null,
        end_date: null,
        status: "COMPLETED",
        backlog_issues: 2,
      } as never)
    ).toEqual({
      status: "COMPLETED",
      progress_snapshot: undefined,
      backlog_issues: 2,
      unstarted_issues: undefined,
      started_issues: undefined,
    });
  });
});

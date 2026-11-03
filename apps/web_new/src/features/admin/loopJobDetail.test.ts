// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loop job detail pure helpers: the pinned copy (headers, placeholder,
// skeleton, stat labels, delete notices), the skip-reason options, and
// the target-cell formatters.
import { describe, expect, it } from "vitest";

import {
  formatSkipOptionLabel,
  formatTargetLastRunStatus,
  formatTargetNextRun,
  formatTargetSkipReason,
  formatTargetTokens,
  LOOP_JOB_DELETED_MESSAGE,
  LOOP_JOB_DELETED_TITLE,
  LOOP_JOB_DELETE_FAILED_MESSAGE,
  LOOP_JOB_DELETE_FAILED_TITLE,
  LOOP_JOB_DESCRIPTION_FALLBACK,
  LOOP_JOB_DETAIL_SKELETON_HEIGHTS,
  LOOP_JOB_DETAIL_SKELETON_TITLE,
  LOOP_JOB_STAT_LABELS,
  LOOP_JOBS_LIST_PATH,
  LOOP_SKIP_FILTER_ALL_LABEL,
  LOOP_SKIP_FILTER_LABEL,
  LOOP_SKIP_FILTER_OPTIONS,
  LOOP_TARGET_MISSING_TEXT,
  LOOP_TARGETS_EMPTY_TEXT,
  LOOP_TARGETS_HEADERS,
  resolveLoopJobDescription,
} from "./loopJobDetail.js";

describe("loop job detail copy", () => {
  it("pins the skeleton title and bar heights", () => {
    expect(LOOP_JOB_DETAIL_SKELETON_TITLE).toBe("Loop job");
    expect([...LOOP_JOB_DETAIL_SKELETON_HEIGHTS]).toEqual(["48px", "120px"]);
  });

  it("pins the targets table headers and empty placeholder", () => {
    expect([...LOOP_TARGETS_HEADERS]).toEqual(["Workspace", "User", "Next run", "Last run", "Tokens", "Last skip"]);
    expect(LOOP_TARGETS_EMPTY_TEXT).toBe("No targets yet.");
    expect(LOOP_TARGET_MISSING_TEXT).toBe("—");
  });

  it("pins the skip-filter label, all label, and option values", () => {
    expect(LOOP_SKIP_FILTER_LABEL).toBe("Skip reason");
    expect(LOOP_SKIP_FILTER_ALL_LABEL).toBe("all");
    expect([...LOOP_SKIP_FILTER_OPTIONS]).toEqual([
      "",
      "user_disabled",
      "master_paused",
      "min_role",
      "llm_config_missing",
      "membership_gone",
      "turn_active",
      "dispatch_error",
    ]);
  });

  it("pins the stat tile labels", () => {
    expect(LOOP_JOB_STAT_LABELS).toEqual({
      targets: "Targets",
      completed: "Completed (24h)",
      failed: "Failed (24h)",
      skipped: "Skipped (24h)",
    });
  });

  it("pins the delete notices and the list return path", () => {
    expect(LOOP_JOB_DELETED_TITLE).toBe("Deleted");
    expect(LOOP_JOB_DELETED_MESSAGE).toBe("Loop job deleted.");
    expect(LOOP_JOB_DELETE_FAILED_TITLE).toBe("Delete failed");
    expect(LOOP_JOB_DELETE_FAILED_MESSAGE).toBe("Could not delete the job.");
    expect(LOOP_JOBS_LIST_PATH).toBe("/god-mode/loop/");
  });

  it("falls back to the detail description only when blank", () => {
    expect(LOOP_JOB_DESCRIPTION_FALLBACK).toBe("Loop job detail");
    expect(resolveLoopJobDescription("")).toBe("Loop job detail");
    expect(resolveLoopJobDescription("Nightly triage")).toBe("Nightly triage");
  });

  it("labels the blank skip option as all", () => {
    expect(formatSkipOptionLabel("")).toBe("all");
    expect(formatSkipOptionLabel("min_role")).toBe("min_role");
  });
});

describe("loop target cells", () => {
  it("renders a missing next run as the missing marker", () => {
    expect(formatTargetNextRun(null)).toBe("—");
    expect(formatTargetNextRun("")).toBe("—");
  });

  it("localizes a set next run", () => {
    const stamp = "2026-02-03T04:05:06Z";
    expect(formatTargetNextRun(stamp)).toBe(new Date(stamp).toLocaleString());
  });

  it("renders a missing last run as the missing marker", () => {
    expect(formatTargetLastRunStatus(null)).toBe("—");
    expect(
      formatTargetLastRunStatus({
        status: "completed",
        error_code: "",
        model_used: "probe-model",
        total_tokens: 12,
        completed_at: "2026-02-03T04:05:06Z",
      })
    ).toBe("completed");
  });

  it("renders missing tokens as the missing marker and zero as zero", () => {
    expect(formatTargetTokens(null)).toBe("—");
    expect(
      formatTargetTokens({
        status: "completed",
        error_code: "",
        model_used: "probe-model",
        total_tokens: null,
        completed_at: "2026-02-03T04:05:06Z",
      })
    ).toBe("—");
    expect(
      formatTargetTokens({
        status: "completed",
        error_code: "",
        model_used: "probe-model",
        total_tokens: 0,
        completed_at: "2026-02-03T04:05:06Z",
      })
    ).toBe("0");
    expect(
      formatTargetTokens({
        status: "completed",
        error_code: "",
        model_used: "probe-model",
        total_tokens: 42,
        completed_at: "2026-02-03T04:05:06Z",
      })
    ).toBe("42");
  });

  it("renders a blank skip reason as the missing marker", () => {
    expect(formatTargetSkipReason("")).toBe("—");
    expect(formatTargetSkipReason("user_disabled")).toBe("user_disabled");
  });
});

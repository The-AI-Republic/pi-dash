// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail-page presentation rules (parity/specs/project-settings-part-f.md
// §PSET-059): config rows, run-row text, empty-history sentence, durations
// and the series-completion read.

import type { AgentRun, SchedulerBindingDetail } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  configRows,
  displayTimeZone,
  emptyHistoryText,
  formatDuration,
  formatMoment,
  isSeriesCompleted,
  pagerTotalPages,
  runResultExcerpt,
  runStartedLabel,
  runStatusLabel,
  runStatusTone,
  installedByName,
  sourceLabel,
} from "./bindingDetail.js";

function binding(overrides: Partial<SchedulerBindingDetail> = {}): SchedulerBindingDetail {
  return {
    id: "b1",
    scheduler: "s1",
    scheduler_slug: "weekly-review",
    scheduler_name: "Weekly review",
    scheduler_color: "#22c55e",
    project: "p1",
    workspace: "w1",
    dtstart: "2026-10-20T09:00:00Z",
    tzid: "Europe/Paris",
    rrule: "FREQ=WEEKLY;BYDAY=FR",
    rdates: [],
    exdates: [],
    extra_context: "",
    enabled: true,
    outcome_mode: "apply_fix",
    pod: null,
    pod_name: null,
    next_run_at: "2026-10-23T09:00:00Z",
    last_run: null,
    last_run_status: null,
    last_run_ended_at: null,
    last_error: "",
    actor: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T12:30:00Z",
    resolved_prompt: "Audit the project.",
    run_count: 0,
    scheduler_source: "builtin",
    scheduler_is_enabled: true,
    ...overrides,
  } as SchedulerBindingDetail;
}

function run(overrides: Partial<AgentRun> = {}): AgentRun {
  return {
    id: "r1",
    status: "completed",
    error: "",
    done_payload: null,
    started_at: "2026-10-20T09:00:00Z",
    ended_at: "2026-10-20T09:03:12Z",
    created_at: "2026-10-20T08:59:00Z",
    pod_detail: { id: "pod1", name: "Main pool" },
    ...overrides,
  } as AgentRun;
}

describe("labels", () => {
  it("renders the raw definition source and defaults a blank zone", () => {
    expect(sourceLabel("builtin")).toBe("builtin");
    expect(sourceLabel("manifest")).toBe("manifest");
    expect(displayTimeZone("")).toBe("UTC");
    expect(displayTimeZone("  ")).toBe("UTC");
    expect(displayTimeZone("America/New_York")).toBe("America/New_York");
  });

  it("names the installer from the directory, keeping an unknown actor", () => {
    const directory = [
      {
        id: "m1",
        member: {
          id: "u1",
          first_name: "Ada",
          last_name: "Byron",
          avatar: "",
          avatar_url: null,
          display_name: "Ada B",
          is_bot: false,
          email: "ada@example.test",
        },
        role: 20,
      },
      {
        id: "m2",
        member: {
          id: "u2",
          first_name: "Grace",
          last_name: "Hopper",
          avatar: "",
          avatar_url: null,
          display_name: "",
          is_bot: false,
          email: "grace@example.test",
        },
        role: 20,
      },
    ];
    expect(installedByName(null, directory)).toBeNull();
    expect(installedByName("u1", directory)).toBe("Ada B");
    expect(installedByName("u2", directory)).toBe("grace");
    expect(installedByName("u9", directory)).toBe("u9");
  });

  it("keeps the raw id for a member with neither a display name nor an email", () => {
    const directory = [
      {
        id: "m3",
        member: {
          id: "u3",
          first_name: "",
          last_name: "",
          avatar: "",
          avatar_url: null,
          display_name: "",
          is_bot: false,
          email: null,
        },
        role: 20,
      },
    ] as unknown as Parameters<typeof installedByName>[1];
    expect(installedByName("u3", directory)).toBe("u3");
  });

  it("keeps the raw id when the email carries no local part", () => {
    const directory = [
      {
        id: "m4",
        member: {
          id: "u4",
          first_name: "",
          last_name: "",
          avatar: "",
          avatar_url: null,
          display_name: "",
          is_bot: false,
          email: "@example.test",
        },
        role: 20,
      },
    ] as unknown as Parameters<typeof installedByName>[1];
    expect(installedByName("u4", directory)).toBe("u4");
  });

  it("reads the never mark for empty or invalid moments", () => {
    expect(formatMoment(null)).toBe("(never)");
    expect(formatMoment("")).toBe("(never)");
    expect(formatMoment("not-a-date")).toBe("(never)");
    expect(formatMoment("2026-10-20T09:00:00Z")).not.toBe("(never)");
    expect(formatMoment(null, "—")).toBe("—");
  });
});

describe("durations and run rows", () => {
  it("formats hours, minutes and seconds with a dash for open runs", () => {
    expect(formatDuration("2026-10-20T09:00:00Z", "2026-10-20T09:00:45Z")).toBe("45s");
    expect(formatDuration("2026-10-20T09:00:00Z", "2026-10-20T09:03:12Z")).toBe("3m 12s");
    expect(formatDuration("2026-10-20T09:00:00Z", "2026-10-20T11:05:00Z")).toBe("2h 5m");
    expect(formatDuration(null, "2026-10-20T09:03:12Z")).toBe("—");
    expect(formatDuration("2026-10-20T09:00:00Z", "")).toBe("—");
    expect(formatDuration("2026-10-20T09:03:12Z", "2026-10-20T09:00:00Z")).toBe("—");
    expect(formatDuration("garbage", "2026-10-20T09:00:00Z")).toBe("—");
  });

  it("prefers a failure excerpt and falls back to the done summary", () => {
    expect(runResultExcerpt(run({ error: "  boom  " }))).toBe("boom");
    expect(runResultExcerpt(run({ done_payload: { summary: "Filed 2 issues." } }))).toBe("Filed 2 issues.");
    expect(runResultExcerpt(run({ done_payload: { summary: 3 } }))).toBe("");
    expect(runResultExcerpt(run())).toBe("");
  });

  it("marks a queued run with its created moment", () => {
    expect(runStartedLabel(run({ started_at: null }))).toContain("Queued");
    expect(runStartedLabel(run({ started_at: "2026-10-20T09:00:00Z" }))).not.toContain("Queued");
  });

  it("labels statuses and buckets their tone", () => {
    expect(runStatusLabel("completed")).toBe("Completed");
    expect(runStatusLabel("waiting_for_worktree")).toBe("Waiting for checkout");
    expect(runStatusLabel("mystery")).toBe("mystery");
    expect(runStatusTone("completed")).toBe("success");
    expect(runStatusTone("failed")).toBe("danger");
    expect(runStatusTone("refused")).toBe("danger");
    expect(runStatusTone("running")).toBe("accent");
    expect(runStatusTone("assigned")).toBe("accent");
    expect(runStatusTone("queued")).toBe("neutral");
  });
});

describe("configuration grid", () => {
  it("lists the core rows in reading order with the raw rule as tooltip", () => {
    const rows = configRows(binding(), null);
    expect(rows.map((row) => row.label)).toEqual([
      "Schedule",
      "Starts at",
      "Time zone",
      "Next run",
      "Outcome mode",
      "Pod",
    ]);
    expect(rows[0]?.value).toContain("Friday");
    expect(rows[0]?.title).toBe("FREQ=WEEKLY;BYDAY=FR");
    expect(rows[2]?.value).toBe("Europe/Paris");
    expect(rows[4]?.value).toBe("apply_fix");
    expect(rows[5]?.value).toBe("(default pod)");
  });

  it("adds extra, skipped and installer rows only when present", () => {
    const rows = configRows(
      binding({
        rdates: ["2026-10-24T09:00:00Z"],
        exdates: ["2026-10-31T09:00:00Z"],
        pod_name: "Main pool",
      }),
      "owner"
    );
    expect(rows.find((row) => row.label === "Extra dates")?.value).not.toBe("");
    expect(rows.find((row) => row.label === "Skipped dates")?.value).not.toBe("");
    expect(rows.find((row) => row.label === "Installed by")?.value).toBe("owner");
    expect(rows.find((row) => row.label === "Pod")?.value).toBe("Main pool");
  });

  it("names a blank zone as UTC", () => {
    expect(configRows(binding({ tzid: "" }), null).find((row) => row.label === "Time zone")?.value).toBe("UTC");
  });

  it("reads a blank stored rule as the dialog's single firing", () => {
    const row = configRows(binding({ rrule: "" }), null).find((entry) => entry.label === "Schedule");
    expect(row?.value).toBe("Fires once at the start time");
    expect(row?.title).toBe("");
  });
});

describe("empty history and completion", () => {
  it("explains disabled, scheduled and plain empty histories", () => {
    expect(emptyHistoryText({ enabled: false, next_run_at: null })).toContain("Scheduler is disabled");
    expect(emptyHistoryText({ enabled: true, next_run_at: "2026-10-23T09:00:00Z" })).toContain("next run at");
    expect(emptyHistoryText({ enabled: true, next_run_at: null })).toBe("No runs yet.");
  });

  it("badges completion only when the API reports the series exhausted", () => {
    expect(isSeriesCompleted({})).toBe(false);
    expect(isSeriesCompleted({ series_exhausted: false })).toBe(false);
    expect(isSeriesCompleted({ series_exhausted: true })).toBe(true);
  });

  it("never reports fewer than one page", () => {
    expect(pagerTotalPages(undefined)).toBe(1);
    expect(pagerTotalPages({ total_pages: 0 })).toBe(1);
    expect(pagerTotalPages({ total_pages: 3 })).toBe(3);
  });
});

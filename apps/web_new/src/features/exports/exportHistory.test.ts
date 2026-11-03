// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import type { WorkspaceExport } from "@pidash/api-client";

import {
  EXPORT_HISTORY_EXPIRY_MS,
  EXPORT_HISTORY_POLL_MS,
  historyDownload,
  historyFormatLabel,
  historyInitiator,
  historyQuery,
  historyRowFacts,
  historyStartedLabel,
  historyStatus,
  isExpired,
  shouldPoll,
  type ExportHistoryStatus,
} from "./exportHistory.js";

const NOW = new Date("2024-04-12T12:00:00.000Z");

function daysAgo(days: number): string {
  return new Date(NOW.getTime() - days * 86_400_000).toISOString();
}

function userDetail(overrides: Partial<WorkspaceExport["initiated_by_detail"]> = {}) {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    first_name: "Ana",
    last_name: "Siva",
    avatar: "",
    avatar_url: null,
    display_name: "Ana Siva",
    is_bot: false,
    ...overrides,
  };
}

function record(overrides: Partial<WorkspaceExport> = {}): WorkspaceExport {
  return {
    id: "22222222-2222-4222-8222-222222222222",
    created_at: daysAgo(1),
    updated_at: daysAgo(1),
    project: ["33333333-3333-4333-8333-333333333333"],
    provider: "csv",
    status: "completed",
    url: "https://files.test/export.csv",
    initiated_by: "11111111-1111-4111-8111-111111111111",
    initiated_by_detail: userDetail(),
    token: "token",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

describe("isExpired", () => {
  it("keeps a fresh record downloadable", () => {
    expect(isExpired(daysAgo(1), NOW)).toBe(false);
  });

  it("lapses a nine-day-old record", () => {
    expect(isExpired(daysAgo(9), NOW)).toBe(true);
  });

  it("reads a record exactly seven days old as expired", () => {
    expect(isExpired(daysAgo(7), NOW)).toBe(true);
  });

  it("keeps a record one millisecond short of seven days", () => {
    const almost = new Date(NOW.getTime() - EXPORT_HISTORY_EXPIRY_MS + 1).toISOString();
    expect(isExpired(almost, NOW)).toBe(false);
  });
});

describe("shouldPoll", () => {
  it("stays idle with no rows", () => {
    expect(shouldPoll([])).toBe(false);
  });

  it("stays idle on a queued row alone", () => {
    expect(shouldPoll([{ status: "queued" }])).toBe(false);
  });

  it("stays idle once every row has settled", () => {
    expect(shouldPoll([{ status: "completed" }, { status: "failed" }])).toBe(false);
  });

  it("polls while any row is processing", () => {
    expect(shouldPoll([{ status: "completed" }, { status: "processing" }])).toBe(true);
  });
});

describe("historyQuery", () => {
  it("polls on the history cadence while work runs", () => {
    expect(historyQuery([{ status: "processing" }])).toEqual({ refetchInterval: EXPORT_HISTORY_POLL_MS });
  });

  it("goes idle once nothing runs", () => {
    expect(historyQuery([{ status: "completed" }])).toEqual({ refetchInterval: false });
  });
});

describe("historyFormatLabel", () => {
  it.each([
    ["csv", "CSV"],
    ["xlsx", "Excel"],
    ["json", "JSON"],
  ])("labels the %s provider", (provider, label) => {
    expect(historyFormatLabel(provider)).toBe(label);
  });

  it("marks an unknown provider instead of crashing", () => {
    expect(historyFormatLabel("parquet")).toBe("Unknown");
  });
});

describe("historyStatus", () => {
  it.each([
    ["queued", "queued"],
    ["processing", "processing"],
    ["completed", "completed"],
    ["failed", "failed"],
  ])("keeps the %s state", (stored, marker) => {
    expect(historyStatus(stored)).toBe(marker);
  });

  it("marks an unknown status instead of crashing", () => {
    expect(historyStatus("archived")).toBe("unknown");
  });
});

describe("historyInitiator", () => {
  it("prefers the display name and the avatar url", () => {
    expect(historyInitiator(userDetail({ display_name: "Ana Siva", avatar_url: "https://files.test/a.png" }))).toEqual({
      name: "Ana Siva",
      initial: "A",
      avatarUrl: "https://files.test/a.png",
    });
  });

  it("falls back to the joined names when the display name is blank", () => {
    expect(historyInitiator(userDetail({ display_name: "  " }))).toEqual({
      name: "Ana Siva",
      initial: "A",
      avatarUrl: null,
    });
  });

  it("takes the initial from the first name when the display name is blank", () => {
    const initiator = historyInitiator(userDetail({ display_name: "", first_name: "ana", last_name: "" }));
    expect(initiator).toEqual({ name: "ana", initial: "a", avatarUrl: null });
  });

  it("falls back to a question mark with no names at all", () => {
    expect(historyInitiator(userDetail({ display_name: "", first_name: " ", last_name: "" }))).toEqual({
      name: "",
      initial: "?",
      avatarUrl: null,
    });
  });

  it("treats a blank avatar url as no avatar", () => {
    expect(historyInitiator(userDetail({ avatar_url: "  " })).avatarUrl).toBeNull();
  });
});

describe("historyStartedLabel", () => {
  it("renders the start date like Apr 12, 2024", () => {
    const noon = new Date(2024, 3, 12, 12, 0, 0);
    expect(historyStartedLabel(noon.toISOString())).toBe("Apr 12, 2024");
  });
});

describe("historyDownload", () => {
  it("links a fresh finished row to its file", () => {
    expect(historyDownload(record(), NOW)).toEqual({ kind: "link", href: "https://files.test/export.csv" });
  });

  it("lapses a finished row after seven days whatever its state", () => {
    expect(historyDownload(record({ created_at: daysAgo(9) }), NOW)).toEqual({ kind: "expired" });
    expect(historyDownload(record({ created_at: daysAgo(9), status: "failed" }), NOW)).toEqual({
      kind: "expired",
    });
  });

  it("shows the dash for fresh rows with no file", () => {
    expect(historyDownload(record({ url: null }), NOW)).toEqual({ kind: "dash" });
    expect(historyDownload(record({ url: "" }), NOW)).toEqual({ kind: "dash" });
    expect(historyDownload(record({ status: "processing", url: null }), NOW)).toEqual({ kind: "dash" });
    expect(historyDownload(record({ status: "failed", url: null }), NOW)).toEqual({ kind: "dash" });
  });
});

describe("historyRowFacts", () => {
  it("reads every cell fact left to right", () => {
    const facts = historyRowFacts(
      record({
        project: ["a", "b"],
        provider: "xlsx",
        status: "processing",
        url: null,
        initiated_by_detail: userDetail({ display_name: "Ana Siva", avatar_url: null }),
      }),
      { now: NOW }
    );
    expect(facts).toEqual({
      initiatorName: "Ana Siva",
      initiatorInitial: "A",
      avatarUrl: null,
      startedLabel: historyStartedLabel(daysAgo(1)),
      projectCount: 2,
      projectLabel: "2 project(s)",
      formatLabel: "Excel",
      status: "processing",
      statusTone: "warning",
      download: { kind: "dash" },
    });
  });

  it("counts a null project list as zero projects", () => {
    const facts = historyRowFacts(record({ project: null }), { now: NOW });
    expect(facts.projectCount).toBe(0);
    expect(facts.projectLabel).toBe("0 project(s)");
  });

  it("marks unknown provider and status rows instead of crashing", () => {
    const facts = historyRowFacts(record({ provider: "parquet", status: "archived", url: null }), {
      now: NOW,
    });
    const status: ExportHistoryStatus = facts.status;
    expect(facts.formatLabel).toBe("Unknown");
    expect(status).toBe("unknown");
    expect(facts.statusTone).toBe("neutral");
    expect(facts.download).toEqual({ kind: "dash" });
  });
});

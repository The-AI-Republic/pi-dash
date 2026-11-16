// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Triage helpers (INT-020–023, 025): the overflow entries, the chip
// model and snooze math, and the post-action focus landing.

import { waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  TRIAGE_DELETE_LABEL,
  TRIAGE_DUPLICATE_LABEL,
  TRIAGE_REFUSED_TITLE,
  TRIAGE_SNOOZE_LABEL,
  TRIAGE_UNSNOOZE_LABEL,
  buildTriageMenuEntries,
  focusTriageLanding,
  scheduleTriageFocus,
  selectIntakeStatusChip,
  snoozeDayFromTimestamp,
  snoozeDaysLeft,
  snoozeTimestampForDay,
  triageRefusedMessage,
} from "./triage.js";

describe("triage refused notice", () => {
  it("names the admin requirement for every guarded action", () => {
    expect(TRIAGE_REFUSED_TITLE).toBe("Permission denied");
    expect(triageRefusedMessage("accept")).toContain("admin");
    expect(triageRefusedMessage("decline")).toContain("admin");
    expect(triageRefusedMessage("snooze")).toContain("admin");
    expect(triageRefusedMessage("duplicate")).toContain("admin");
  });
});

describe("buildTriageMenuEntries", () => {
  function handlers(overrides: Partial<Parameters<typeof buildTriageMenuEntries>[0]> = {}) {
    return {
      snoozed: false,
      showDelete: true,
      onSnooze: vi.fn(),
      onUnsnooze: vi.fn(),
      onDuplicate: vi.fn(),
      onDelete: vi.fn(),
      ...overrides,
    };
  }

  it("leads with snooze and the duplicate picker, trailing delete", () => {
    const h = handlers();
    const entries = buildTriageMenuEntries(h);
    expect(entries.lead.map((entry) => ("label" in entry ? entry.label : null))).toEqual([
      TRIAGE_SNOOZE_LABEL,
      TRIAGE_DUPLICATE_LABEL,
    ]);
    expect(entries.trail.map((entry) => ("label" in entry ? entry.label : null))).toEqual([TRIAGE_DELETE_LABEL]);
    for (const entry of [...entries.lead, ...entries.trail]) {
      if ("onSelect" in entry) entry.onSelect();
    }
    expect(h.onSnooze).toHaveBeenCalledTimes(1);
    expect(h.onDuplicate).toHaveBeenCalledTimes(1);
    expect(h.onDelete).toHaveBeenCalledTimes(1);
    expect(h.onUnsnooze).not.toHaveBeenCalled();
  });

  it("swaps snooze for un-snooze on a snoozed request", () => {
    const h = handlers({ snoozed: true });
    const entries = buildTriageMenuEntries(h);
    expect(entries.lead.map((entry) => ("label" in entry ? entry.label : null))).toEqual([
      TRIAGE_UNSNOOZE_LABEL,
      TRIAGE_DUPLICATE_LABEL,
    ]);
    const first = entries.lead[0]!;
    if ("onSelect" in first) first.onSelect();
    expect(h.onUnsnooze).toHaveBeenCalledTimes(1);
    expect(h.onSnooze).not.toHaveBeenCalled();
  });

  it("omits delete without remove standing", () => {
    const entries = buildTriageMenuEntries(handlers({ showDelete: false }));
    expect(entries.trail).toEqual([]);
  });
});

describe("snooze day math", () => {
  it("reads the local day of a timestamp", () => {
    const at = new Date(2026, 9, 30, 12, 0, 0);
    expect(snoozeDayFromTimestamp(at.toISOString())).toBe("2026-10-30");
  });

  it("rejects an absent or unparseable timestamp", () => {
    expect(snoozeDayFromTimestamp(null)).toBeNull();
    expect(snoozeDayFromTimestamp("not-a-date")).toBeNull();
  });

  it("counts whole calendar days to the snooze day", () => {
    expect(snoozeDaysLeft("2026-11-06", "2026-10-30")).toBe(7);
    expect(snoozeDaysLeft("2026-10-30", "2026-10-30")).toBe(0);
    expect(snoozeDaysLeft("2026-10-29", "2026-10-30")).toBe(-1);
  });

  it("rejects malformed days on either side", () => {
    expect(snoozeDaysLeft(null, "2026-10-30")).toBeNull();
    expect(snoozeDaysLeft("2026-10-31", "today")).toBeNull();
    expect(snoozeDaysLeft("2026-02-30", "2026-10-30")).toBeNull();
    expect(snoozeDaysLeft("2026-10-31", "2026-13-01")).toBeNull();
    expect(snoozeDaysLeft("2026-10", "2026-10-30")).toBeNull();
    expect(snoozeDaysLeft("", "2026-10-30")).toBeNull();
    expect(snoozeDaysLeft("2026--15", "2026-10-30")).toBeNull();
    expect(snoozeDaysLeft("2026-10-31", "2026-10-xx")).toBeNull();
  });

  it("stores a day as its local midnight", () => {
    const stored = snoozeTimestampForDay("2026-10-30");
    expect(stored).not.toBeNull();
    const at = new Date(stored!);
    expect(at.getFullYear()).toBe(2026);
    expect(at.getMonth()).toBe(9);
    expect(at.getDate()).toBe(30);
    expect(at.getHours()).toBe(0);
    expect(at.getMinutes()).toBe(0);
  });

  it("rejects a malformed day for storage", () => {
    expect(snoozeTimestampForDay("2026-02-30")).toBeNull();
    expect(snoozeTimestampForDay("tomorrow")).toBeNull();
    expect(snoozeTimestampForDay("2026-10")).toBeNull();
    expect(snoozeTimestampForDay("2026--15")).toBeNull();
  });
});

describe("selectIntakeStatusChip", () => {
  it("names resolved and pending requests", () => {
    expect(selectIntakeStatusChip(1, null, "2026-10-30")).toEqual({ text: "Accepted", tone: "default" });
    expect(selectIntakeStatusChip(-1, null, "2026-10-30")).toEqual({ text: "Declined", tone: "default" });
    expect(selectIntakeStatusChip(2, null, "2026-10-30")).toEqual({ text: "Duplicate", tone: "default" });
    expect(selectIntakeStatusChip(-2, null, "2026-10-30")).toEqual({ text: "Pending", tone: "default" });
    expect(selectIntakeStatusChip(99, null, "2026-10-30")).toEqual({ text: "Pending", tone: "default" });
  });

  it("counts whole days to the snooze day", () => {
    const stamp = (day: number) => new Date(2026, 9, day, 0, 0, 0).toISOString();
    expect(selectIntakeStatusChip(0, stamp(31), "2026-10-30")).toEqual({ text: "1 day to go", tone: "default" });
    expect(selectIntakeStatusChip(0, stamp(31), "2026-10-24")).toEqual({
      text: "7 days to go",
      tone: "default",
    });
    expect(selectIntakeStatusChip(0, stamp(30), "2026-10-30")).toEqual({ text: "Due today", tone: "default" });
  });

  it("keeps a lapsed snooze visible with the passed tone", () => {
    const past = new Date(2026, 9, 29, 0, 0, 0).toISOString();
    expect(selectIntakeStatusChip(0, past, "2026-10-30")).toEqual({ text: "Overdue", tone: "danger" });
  });

  it("names a dateless snooze without a countdown", () => {
    expect(selectIntakeStatusChip(0, null, "2026-10-30")).toEqual({ text: "Snoozed", tone: "default" });
    expect(selectIntakeStatusChip(0, "broken", "2026-10-30")).toEqual({ text: "Snoozed", tone: "default" });
  });
});

describe("focusTriageLanding", () => {
  it("lands on the editable title inside the neighbour detail", () => {
    document.body.innerHTML =
      '<div data-testid="intake-detail"><textarea aria-label="Title"></textarea></div><div data-testid="intake-list-pane" tabindex="-1"></div>';
    const field = document.querySelector('textarea[aria-label="Title"]') as HTMLElement;
    expect(focusTriageLanding(false)).toBe(true);
    expect(document.activeElement).toBe(field);
    document.body.innerHTML = "";
  });

  it("lands on the detail container when the title is static", () => {
    document.body.innerHTML = '<div data-testid="intake-detail" tabindex="-1"><h1>APO-7</h1></div>';
    const detail = document.querySelector('[data-testid="intake-detail"]') as HTMLElement;
    expect(focusTriageLanding(false)).toBe(true);
    expect(document.activeElement).toBe(detail);
    document.body.innerHTML = "";
  });

  it("lands nowhere when the detail is gone", () => {
    document.body.innerHTML = "";
    expect(focusTriageLanding(false)).toBe(false);
  });

  it("lands on the first row when the tab emptied", () => {
    document.body.innerHTML =
      '<div data-testid="intake-list-pane" tabindex="-1"><button type="button" data-testid="intake-row">A</button></div>';
    const row = document.querySelector('[data-testid="intake-row"]') as HTMLElement;
    expect(focusTriageLanding(true)).toBe(true);
    expect(document.activeElement).toBe(row);
    document.body.innerHTML = "";
  });

  it("falls back to the list pane when no row remains", () => {
    document.body.innerHTML = '<div data-testid="intake-list-pane" tabindex="-1"></div>';
    const pane = document.querySelector('[data-testid="intake-list-pane"]') as HTMLElement;
    expect(focusTriageLanding(true)).toBe(true);
    expect(document.activeElement).toBe(pane);
    document.body.innerHTML = "";
  });

  it("lands nowhere when the list is gone too", () => {
    document.body.innerHTML = "";
    expect(focusTriageLanding(true)).toBe(false);
  });
});

describe("scheduleTriageFocus", () => {
  it("lands on the first frame when the detail is already painted", async () => {
    document.body.innerHTML = '<div data-testid="intake-detail" tabindex="-1"></div>';
    const detail = document.querySelector('[data-testid="intake-detail"]') as HTMLElement;
    scheduleTriageFocus(false);
    await waitFor(() => expect(document.activeElement).toBe(detail));
    document.body.innerHTML = "";
  });

  it("retries across frames until the landing paints", async () => {
    document.body.innerHTML = "";
    scheduleTriageFocus(false);
    globalThis.setTimeout(() => {
      document.body.innerHTML = '<div data-testid="intake-detail" tabindex="-1"></div>';
    }, 30);
    await waitFor(() => expect(document.activeElement).toBe(document.querySelector('[data-testid="intake-detail"]')));
    document.body.innerHTML = "";
  });

  it("gives up quietly when the landing never arrives", async () => {
    document.body.innerHTML = "";
    const before = document.activeElement;
    scheduleTriageFocus(false, 2);
    await new Promise((resolve) => globalThis.setTimeout(resolve, 80));
    expect(document.activeElement).toBe(before);
  });
});

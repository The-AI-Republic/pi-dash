// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Incremental history reads (CMT-017): the per-source refresh cursor
// and the delta merge that joins new rows onto the cached list.
import { describe, expect, it } from "vitest";

import { mergeDeltaRows, newestCreatedAt, type TimestampedRow } from "./incremental.js";

function row(id: string, created_at: string): TimestampedRow {
  return { id, created_at };
}

describe("newestCreatedAt", () => {
  it("returns the newest stored timestamp untouched", () => {
    const rows = [row("a", "2026-10-16T10:00:00Z"), row("b", "2026-10-16T12:00:00+00:00")];
    expect(newestCreatedAt(rows)).toBe("2026-10-16T12:00:00+00:00");
    expect(newestCreatedAt([row("only", "2026-10-16T10:00:00Z")])).toBe("2026-10-16T10:00:00Z");
  });

  it("keeps the first row when timestamps tie", () => {
    const rows = [row("a", "2026-10-16T10:00:00Z"), row("b", "2026-10-16T10:00:00Z")];
    expect(newestCreatedAt(rows)).toBe("2026-10-16T10:00:00Z");
  });

  it("yields no cursor for an empty list or unreadable timestamps", () => {
    expect(newestCreatedAt([])).toBeUndefined();
    expect(newestCreatedAt([row("a", "not a date"), row("b", "")])).toBeUndefined();
  });

  it("skips unreadable timestamps but keeps the readable ones", () => {
    const rows = [row("a", "not a date"), row("b", "2026-10-16T10:00:00Z")];
    expect(newestCreatedAt(rows)).toBe("2026-10-16T10:00:00Z");
  });
});

describe("mergeDeltaRows", () => {
  it("appends new rows after the cached ones", () => {
    const previous = [row("a", "2026-10-16T10:00:00Z")];
    const delta = [row("b", "2026-10-16T11:00:00Z"), row("c", "2026-10-16T12:00:00Z")];
    expect(mergeDeltaRows(previous, delta).map((entry) => entry.id)).toEqual(["a", "b", "c"]);
  });

  it("skips rows already present and keeps the cached copy", () => {
    const cached = row("a", "2026-10-16T10:00:00Z");
    const repeated = row("a", "2026-10-16T10:00:00Z");
    const merged = mergeDeltaRows([cached], [repeated, row("b", "2026-10-16T11:00:00Z")]);
    expect(merged.map((entry) => entry.id)).toEqual(["a", "b"]);
    expect(merged[0]).toBe(cached);
  });

  it("leaves the cached list alone when the delta is empty", () => {
    const previous = [row("a", "2026-10-16T10:00:00Z")];
    expect(mergeDeltaRows(previous, []).map((entry) => entry.id)).toEqual(["a"]);
  });
});

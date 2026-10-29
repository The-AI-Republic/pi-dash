// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import {
  FRESH_BYPASS_LIMIT,
  FRESH_WINDOW_MS,
  createTempId,
  isFresh,
  isRowHighlighted,
  isTempRowId,
  partitionFresh,
  rowDomId,
} from "./fresh.js";

const NOW = Date.parse("2026-10-17T03:00:00Z");
const FRESH_AT = new Date(NOW - 5_000).toISOString();
const STALE_AT = new Date(NOW - FRESH_WINDOW_MS - 1_000).toISOString();

describe("freshness", () => {
  it("treats issues inside the half-minute window as fresh", () => {
    expect(FRESH_WINDOW_MS).toBe(30_000);
    expect(isFresh(FRESH_AT, NOW)).toBe(true);
    expect(isFresh(new Date(NOW - FRESH_WINDOW_MS + 1).toISOString(), NOW)).toBe(true);
    expect(isFresh(STALE_AT, NOW)).toBe(false);
    expect(isFresh(new Date(NOW - FRESH_WINDOW_MS).toISOString(), NOW)).toBe(false);
  });

  it("treats future and unreadable timestamps as fresh and stale", () => {
    expect(isFresh(new Date(NOW + 60_000).toISOString(), NOW)).toBe(true);
    expect(isFresh("not-a-date", NOW)).toBe(false);
  });

  it("derives row dom ids from issue ids", () => {
    expect(rowDomId("abc")).toBe("issue-abc");
  });

  it("highlights fresh rows and the explicit id only", () => {
    expect(isRowHighlighted({ id: "a", created_at: FRESH_AT }, null, NOW)).toBe(true);
    expect(isRowHighlighted({ id: "a", created_at: STALE_AT }, "a", NOW)).toBe(true);
    expect(isRowHighlighted({ id: "a", created_at: STALE_AT }, "b", NOW)).toBe(false);
    expect(isRowHighlighted({ id: "a", created_at: STALE_AT }, null, NOW)).toBe(false);
  });

  it("partitions fresh rows first, preserving order in both halves", () => {
    const issues = [
      { id: "stale-1", created_at: STALE_AT },
      { id: "fresh-1", created_at: FRESH_AT },
      { id: "stale-2", created_at: STALE_AT },
      { id: "fresh-2", created_at: FRESH_AT },
    ];
    expect(partitionFresh(issues, NOW)).toEqual({
      fresh: [issues[1], issues[3]],
      rest: [issues[0], issues[2]],
    });
    expect(partitionFresh([], NOW)).toEqual({ fresh: [], rest: [] });
  });

  it("caps the bypass so overflow fresh rows join the virtualized rest (NEWFRONT-1014)", () => {
    const issues = [
      ...Array.from({ length: FRESH_BYPASS_LIMIT + 2 }, (_, index) => ({
        id: `fresh-${index}`,
        created_at: FRESH_AT,
      })),
      { id: "stale-1", created_at: STALE_AT },
    ];
    const { fresh, rest } = partitionFresh(issues, NOW);
    expect(fresh.map((row) => row.id)).toEqual(
      Array.from({ length: FRESH_BYPASS_LIMIT }, (_, index) => `fresh-${index}`)
    );
    expect(rest.map((row) => row.id)).toEqual([
      `fresh-${FRESH_BYPASS_LIMIT}`,
      `fresh-${FRESH_BYPASS_LIMIT + 1}`,
      "stale-1",
    ]);
  });

  it("honors an explicit bypass limit, including zero", () => {
    const issues = [
      { id: "fresh-1", created_at: FRESH_AT },
      { id: "stale-1", created_at: STALE_AT },
      { id: "fresh-2", created_at: FRESH_AT },
    ];
    expect(partitionFresh(issues, NOW, 1)).toEqual({ fresh: [issues[0]], rest: [issues[1], issues[2]] });
    expect(partitionFresh(issues, NOW, 0)).toEqual({ fresh: [], rest: issues });
  });

  it("mints unique temp ids under the temp prefix", () => {
    const first = createTempId();
    const second = createTempId();
    expect(first).toMatch(/^temp-/);
    expect(second).not.toBe(first);
    expect(isTempRowId(first)).toBe(true);
    expect(isTempRowId("123e4567-e89b-12d3-a456-426614174000")).toBe(false);
  });
});

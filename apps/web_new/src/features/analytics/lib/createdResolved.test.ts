// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Created-vs-resolved shaping tests (parity/specs/analytics.md §9, ANL-009).
import type { AnalyticsWorkItemsChartPoint } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  CREATED_SERIES_KEY,
  CREATED_SERIES_NAME,
  formatSeriesMonth,
  RESOLVED_SERIES_KEY,
  RESOLVED_SERIES_NAME,
  selectCreatedResolvedColors,
  selectCreatedResolvedPoints,
} from "./createdResolved.js";

function point(name: string, created: number, completed: number): AnalyticsWorkItemsChartPoint {
  return { key: name, name, count: created, created_issues: created, completed_issues: completed };
}

describe("created-vs-resolved shaping", () => {
  it("labels buckets as calendar dates with the day kept", () => {
    expect(formatSeriesMonth("2026-10-01")).toBe("Oct 01, 2026");
    expect(formatSeriesMonth("2024-03-15")).toBe("Mar 15, 2024");
  });

  it("leaves non-buckets verbatim", () => {
    expect(formatSeriesMonth("October")).toBe("October");
    expect(formatSeriesMonth("2026-13-01")).toBe("2026-13-01");
    expect(formatSeriesMonth("2026-10-32")).toBe("2026-10-32");
    expect(formatSeriesMonth("2026-10-00")).toBe("2026-10-00");
  });

  it("names the series resolved-first with green and blue fills", () => {
    expect(RESOLVED_SERIES_NAME).toBe("Resolved");
    expect(CREATED_SERIES_NAME).toBe("Created");
    expect(selectCreatedResolvedColors("light")).toEqual({ resolved: "#2f9e5b", created: "#2563eb" });
    expect(selectCreatedResolvedColors("dark")).toEqual({ resolved: "#3db86c", created: "#60a5fa" });
  });

  it("shapes buckets into labeled points in server order", () => {
    const points = selectCreatedResolvedPoints([point("2026-10-01", 3, 1), point("2026-11-01", 0, 2)]);
    expect(points).toEqual([
      { label: "Oct 01, 2026", values: { [RESOLVED_SERIES_KEY]: 1, [CREATED_SERIES_KEY]: 3 } },
      { label: "Nov 01, 2026", values: { [RESOLVED_SERIES_KEY]: 2, [CREATED_SERIES_KEY]: 0 } },
    ]);
  });
});

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared chart series mapping tests (parity/specs/analytics.md
// §§6, 9–11, NEWFRONT-732).
import { describe, expect, it } from "vitest";

import {
  barSegmentKey,
  createLegendOrderSorter,
  mapAreaData,
  mapBarData,
  mapBarPayloadValues,
  mapLegendEntries,
  mapRadarData,
  mapTooltipEntries,
  mapTooltipLabel,
} from "./series.js";

describe("mapRadarData", () => {
  it("renames points onto the plotted keys in order", () => {
    expect(
      mapRadarData([
        { label: "Work Items", value: 2 },
        { label: "Cycles", value: 0 },
      ])
    ).toEqual([
      { dimension: "Work Items", count: 2 },
      { dimension: "Cycles", count: 0 },
    ]);
  });

  it("maps no points to no rows", () => {
    expect(mapRadarData([])).toEqual([]);
  });
});

describe("mapAreaData", () => {
  it("flattens points onto the series keys", () => {
    expect(
      mapAreaData(
        [
          { label: "Oct 01, 2026", values: { resolved: 3, created: 5 } },
          { label: "Nov 01, 2026", values: { resolved: 1, created: 0 } },
        ],
        ["resolved", "created"]
      )
    ).toEqual([
      { label: "Oct 01, 2026", resolved: 3, created: 5 },
      { label: "Nov 01, 2026", resolved: 1, created: 0 },
    ]);
  });

  it("reads a missing series as zero", () => {
    expect(mapAreaData([{ label: "Oct 01, 2026", values: { created: 5 } }], ["resolved", "created"])).toEqual([
      { label: "Oct 01, 2026", resolved: 0, created: 5 },
    ]);
  });

  it("maps no points to no rows", () => {
    expect(mapAreaData([], ["resolved"])).toEqual([]);
  });
});

describe("mapBarData", () => {
  it("flattens bars onto indexed segment keys", () => {
    expect(barSegmentKey(0)).toBe("segment-0");
    expect(barSegmentKey(2)).toBe("segment-2");
    expect(
      mapBarData(
        [
          { label: "Urgent", values: [2, 1] },
          { label: "High", values: [0, 4] },
        ],
        2
      )
    ).toEqual([
      { label: "Urgent", "segment-0": 2, "segment-1": 1 },
      { label: "High", "segment-0": 0, "segment-1": 4 },
    ]);
  });

  it("reads segments past a short bar as zero", () => {
    expect(mapBarData([{ label: "Urgent", values: [2] }], 2)).toEqual([
      { label: "Urgent", "segment-0": 2, "segment-1": 0 },
    ]);
  });

  it("maps no bars to no rows", () => {
    expect(mapBarData([], 2)).toEqual([]);
  });
});

describe("mapBarPayloadValues", () => {
  it("reads segment values off a payload row", () => {
    expect(mapBarPayloadValues({ label: "Urgent", "segment-0": 2, "segment-1": 1 }, 2)).toEqual([2, 1]);
  });

  it("reads missing and non-number segments as zero", () => {
    expect(mapBarPayloadValues({ "segment-0": "two" }, 2)).toEqual([0, 0]);
  });

  it("reads non-record payloads as all zero", () => {
    expect(mapBarPayloadValues(undefined, 2)).toEqual([0, 0]);
    expect(mapBarPayloadValues(null, 1)).toEqual([0]);
    expect(mapBarPayloadValues("row", 1)).toEqual([0]);
  });

  it("reads no values for no segments", () => {
    expect(mapBarPayloadValues({ "segment-0": 2 }, 0)).toEqual([]);
  });
});

describe("mapTooltipLabel", () => {
  it("passes string labels through", () => {
    expect(mapTooltipLabel("Work Items")).toBe("Work Items");
  });

  it("reads missing labels as empty", () => {
    expect(mapTooltipLabel(null)).toBe("");
    expect(mapTooltipLabel(undefined)).toBe("");
  });

  it("stringifies non-string labels", () => {
    expect(mapTooltipLabel(2026)).toBe("2026");
  });
});

describe("mapTooltipEntries", () => {
  it("shapes hovered values for display", () => {
    expect(
      mapTooltipEntries([
        { name: "Count", value: 2, color: "#5e6ad2" },
        { name: "Resolved", value: "3", color: "#2f9e5b" },
      ])
    ).toEqual([
      { name: "Count", value: "2", color: "#5e6ad2" },
      { name: "Resolved", value: "3", color: "#2f9e5b" },
    ]);
  });

  it("reads a missing payload as no entries", () => {
    expect(mapTooltipEntries(undefined)).toEqual([]);
    expect(mapTooltipEntries(null)).toEqual([]);
    expect(mapTooltipEntries({})).toEqual([]);
    expect(mapTooltipEntries([])).toEqual([]);
  });

  it("reads missing names and values as empty and drops non-string colors", () => {
    expect(mapTooltipEntries([{ color: 7 }, {}, "skip", null, 3])).toEqual([
      { name: "", value: "", color: undefined },
      { name: "", value: "", color: undefined },
    ]);
  });

  it("stringifies non-string names and values", () => {
    expect(mapTooltipEntries([{ name: 7, value: 12 }])).toEqual([{ name: "7", value: "12", color: undefined }]);
  });
});

describe("mapLegendEntries", () => {
  it("shapes legend rows for display", () => {
    expect(
      mapLegendEntries([
        { value: "Resolved", color: "#2f9e5b" },
        { value: "Created", color: "#3aa7c1" },
      ])
    ).toEqual([
      { label: "Resolved", color: "#2f9e5b" },
      { label: "Created", color: "#3aa7c1" },
    ]);
  });

  it("reads a missing payload as no rows", () => {
    expect(mapLegendEntries(undefined)).toEqual([]);
    expect(mapLegendEntries("nope")).toEqual([]);
    expect(mapLegendEntries([])).toEqual([]);
  });

  it("reads missing labels as empty and drops non-string colors", () => {
    expect(mapLegendEntries([{ color: 9 }, {}, null])).toEqual([
      { label: "", color: undefined },
      { label: "", color: undefined },
    ]);
  });

  it("stringifies non-string labels", () => {
    expect(mapLegendEntries([{ value: 4 }])).toEqual([{ label: "4", color: undefined }]);
  });
});

describe("createLegendOrderSorter", () => {
  const SERIES = [
    { key: "resolved", name: "Resolved" },
    { key: "created", name: "Created" },
  ];

  it("ranks entries by series data key", () => {
    const rank = createLegendOrderSorter(SERIES);
    expect(rank({ value: "Resolved", dataKey: "resolved" })).toBe(0);
    expect(rank({ value: "Created", dataKey: "created" })).toBe(1);
  });

  it("falls back to the display name when the key is missing", () => {
    const rank = createLegendOrderSorter(SERIES);
    expect(rank({ value: "Created" })).toBe(1);
    expect(rank({ value: "Resolved" })).toBe(0);
  });

  it("sorts unknown entries after every series", () => {
    const rank = createLegendOrderSorter(SERIES);
    expect(rank({ value: "Other", dataKey: "other" })).toBe(2);
    expect(rank({ value: undefined })).toBe(2);
    expect(rank({ value: "Created", dataKey: () => 1 })).toBe(1);
  });
});

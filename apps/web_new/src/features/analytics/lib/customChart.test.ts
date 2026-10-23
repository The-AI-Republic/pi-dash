// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom bar-chart shaping tests (parity/specs/analytics.md §10, ANL-011).
import type { AnalyticsCustomChartRow, State } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  buildCustomChartModel,
  capitalizeCustomLabel,
  CUSTOM_SINGLE_SERIES_NAME,
  formatCustomDateLabel,
  indexStateColors,
  selectCustomBarLabel,
  selectCustomSegmentLabel,
  selectPriorityBarColor,
  selectStateBarColor,
  type CustomChartInput,
} from "./customChart.js";

const PALETTE = [
  "#6172e8",
  "#8b6edb",
  "#e05f99",
  "#29a383",
  "#cb8a37",
  "#3aa7c1",
  "#f1b24a",
  "#e84855",
  "#50c799",
  "#b35f9e",
];

function stateRow(overrides: Partial<State> & { id: string; name: string; color: string }): State {
  return {
    project_id: "00000000-0000-4000-8000-000000000001",
    workspace_id: "00000000-0000-4000-8000-000000000002",
    group: "backlog",
    default: false,
    description: "",
    sequence: 0,
    ...overrides,
  };
}

function row(key: string | number, name: string, count: number, extra: Record<string, unknown> = {}) {
  return { key, name, count, ...extra } as AnalyticsCustomChartRow;
}

function input(overrides: Partial<CustomChartInput>): CustomChartInput {
  return {
    rows: [],
    schema: {},
    xAxis: "PRIORITY",
    split: null,
    states: [],
    scheme: "light",
    currentYear: 2026,
    ...overrides,
  };
}

describe("custom labels", () => {
  it("capitalizes the first letter only", () => {
    expect(capitalizeCustomLabel("urgent")).toBe("Urgent");
    expect(capitalizeCustomLabel("Urgent")).toBe("Urgent");
    expect(capitalizeCustomLabel("")).toBe("");
  });

  it("reads missing dates as None", () => {
    expect(formatCustomDateLabel("", 2026)).toBe("None");
    expect(formatCustomDateLabel("   ", 2026)).toBe("None");
    expect(formatCustomDateLabel("none", 2026)).toBe("None");
    expect(formatCustomDateLabel("None", 2026)).toBe("None");
  });

  it("labels current-year dates without the year and older dates with it", () => {
    expect(formatCustomDateLabel("2026-10-23", 2026)).toBe("Oct 23");
    expect(formatCustomDateLabel("2024-03-15", 2026)).toBe("Mar 15, 2024");
    expect(formatCustomDateLabel("2027-01-05", 2026)).toBe("Jan 5, 2027");
  });

  it("leaves invalid values verbatim", () => {
    expect(formatCustomDateLabel("not a date", 2026)).toBe("not a date");
    expect(formatCustomDateLabel("2026-13-01", 2026)).toBe("2026-13-01");
    expect(formatCustomDateLabel("2026-00-10", 2026)).toBe("2026-00-10");
    expect(formatCustomDateLabel("2026-10-32", 2026)).toBe("2026-10-32");
    expect(formatCustomDateLabel("2026-10-00", 2026)).toBe("2026-10-00");
  });

  it("formats bar labels per axis", () => {
    expect(selectCustomBarLabel("2024-03-15", "TARGET_DATE", 2026)).toBe("Mar 15, 2024");
    expect(selectCustomBarLabel("urgent", "PRIORITY", 2026)).toBe("Urgent");
    expect(selectCustomBarLabel("started", "STATE_GROUPS", 2026)).toBe("Started");
    expect(selectCustomBarLabel("Anl Label 0", "LABELS", 2026)).toBe("Anl Label 0");
  });

  it("labels stacked segments under the split rule", () => {
    expect(selectCustomSegmentLabel("Backlog", "backlog", "STATES")).toBe("Backlog");
    expect(selectCustomSegmentLabel("urgent", "urgent", "PRIORITY")).toBe("Urgent");
    expect(selectCustomSegmentLabel(undefined, "mystery", "LABELS")).toBe("mystery");
    expect(selectCustomSegmentLabel(undefined, "mystery", "STATE_GROUPS")).toBe("Mystery");
  });
});

describe("custom bar colors", () => {
  it("paints priority bars with the fixed hues, matched case-insensitively", () => {
    expect(selectPriorityBarColor("urgent", 0, "light")).toBe("#ef4444");
    expect(selectPriorityBarColor("High", 1, "light")).toBe("#f97316");
    expect(selectPriorityBarColor("MEDIUM", 2, "light")).toBe("#eab308");
    expect(selectPriorityBarColor("low", 3, "light")).toBe("#22c55e");
    expect(selectPriorityBarColor("None", 4, "light")).toBe("#ced4da");
  });

  it("falls back to the palette slot for unknown priority keys", () => {
    expect(selectPriorityBarColor("triage", 2, "light")).toBe(PALETTE[2]);
    expect(selectPriorityBarColor(0, 1, "light")).toBe(PALETTE[1]);
  });

  it("indexes state colors by id and by name, first row wins", () => {
    const colors = indexStateColors([
      stateRow({ id: "s1", name: "Backlog", color: "#111111" }),
      stateRow({ id: "s1", name: "Backlog", color: "#222222" }),
      stateRow({ id: "s2", name: "Backlog", color: "#333333" }),
    ]);
    expect(colors.byId.get("s1")).toBe("#111111");
    expect(colors.byName.get("Backlog")).toBe("#111111");
    expect(colors.byId.get("s2")).toBe("#333333");
  });

  it("paints state bars from the workspace colors with a palette fallback", () => {
    const colors = indexStateColors([stateRow({ id: "s1", name: "Backlog", color: "#123456" })]);
    expect(selectStateBarColor("s1", "Backlog", 0, "light", colors)).toBe("#123456");
    expect(selectStateBarColor("gone", "Backlog", 1, "light", colors)).toBe("#123456");
    expect(selectStateBarColor("gone", "Missing", 2, "light", colors)).toBe(PALETTE[2]);
  });
});

describe("buildCustomChartModel", () => {
  it("shapes single-series priority bars with fixed hues", () => {
    const model = buildCustomChartModel(
      input({
        rows: [row("urgent", "urgent", 2), row("none", "none", 1)],
      })
    );
    expect(CUSTOM_SINGLE_SERIES_NAME).toBe("Count");
    expect(model.segments).toEqual([{ name: "Count", color: PALETTE[0] }]);
    expect(model.bars).toEqual([
      { label: "Urgent", values: [2] },
      { label: "None", values: [1] },
    ]);
    expect(model.fills).toEqual(["#ef4444", "#ced4da"]);
  });

  it("shapes single-series state bars from the workspace colors", () => {
    const model = buildCustomChartModel(
      input({
        xAxis: "STATES",
        rows: [row("s1", "Backlog", 5), row("gone", "Missing", 1)],
        states: [stateRow({ id: "s1", name: "Backlog", color: "#123456" })],
      })
    );
    expect(model.bars.map((bar) => bar.label)).toEqual(["Backlog", "Missing"]);
    expect(model.fills).toEqual(["#123456", PALETTE[1]]);
  });

  it("paints other single-series axes from the palette in plotted order", () => {
    const model = buildCustomChartModel(
      input({
        xAxis: "LABELS",
        rows: [row("l1", "Anl Label 0", 1), row("l2", "Anl Label 1", 1)],
      })
    );
    expect(model.bars.map((bar) => bar.label)).toEqual(["Anl Label 0", "Anl Label 1"]);
    expect(model.fills).toEqual([PALETTE[0], PALETTE[1]]);
  });

  it("stacks one segment per schema key in schema order", () => {
    const model = buildCustomChartModel(
      input({
        split: "STATES",
        rows: [row("urgent", "urgent", 3, { s1: 1, s2: 1, s3: 1 })],
        schema: { s1: "Backlog", s2: "In Progress", s3: "Done" },
      })
    );
    expect(model.fills).toBeUndefined();
    expect(model.segments).toEqual([
      { name: "Backlog", color: PALETTE[0] },
      { name: "In Progress", color: PALETTE[1] },
      { name: "Done", color: PALETTE[2] },
    ]);
    expect(model.bars).toEqual([{ label: "Urgent", values: [1, 1, 1] }]);
  });

  it("reads missing and non-numeric segment values as zero", () => {
    const model = buildCustomChartModel(
      input({
        split: "LABELS",
        rows: [row("urgent", "urgent", 1, { l1: 1, l2: "two" })],
        schema: { l1: "Anl Label 0", l2: "Anl Label 1", l3: "Anl Label 2" },
      })
    );
    expect(model.bars).toEqual([{ label: "Urgent", values: [1, 0, 0] }]);
  });

  it("extends past ten segments with generated hues", () => {
    const schema: Record<string, string> = {};
    const extra: Record<string, number> = {};
    for (let index = 0; index < 11; index += 1) {
      schema[`l${index}`] = `Anl Label ${index}`;
      extra[`l${index}`] = 1;
    }
    const model = buildCustomChartModel(input({ split: "LABELS", rows: [row("none", "none", 11, extra)], schema }));
    expect(model.segments.slice(0, 10).map((segment) => segment.color)).toEqual(PALETTE);
    const extended = model.segments[10]?.color ?? "";
    expect(extended).toMatch(/^#[0-9a-f]{6}$/);
    expect(PALETTE).not.toContain(extended);
  });
});

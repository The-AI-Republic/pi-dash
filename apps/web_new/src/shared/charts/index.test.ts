// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  AreaChart,
  BAR_END_RADIUS,
  BarChart,
  barSegmentKey,
  CHART_PALETTE_SLOTS,
  ChartLegendContent,
  ChartTooltipContent,
  chartPalette,
  chartSlotColor,
  mapAreaData,
  mapBarData,
  mapBarPayloadValues,
  mapLegendEntries,
  mapRadarData,
  mapTooltipEntries,
  mapTooltipLabel,
  RadarChart,
  resolveChartScheme,
  roundedEnds,
  singleBarRadius,
  stackedBarRadii,
  stackedSegmentRadius,
} from "./index.js";

describe("charts barrel", () => {
  it("exposes the shared chart layer's public API", () => {
    expect(typeof RadarChart).toBe("function");
    expect(typeof AreaChart).toBe("function");
    expect(typeof BarChart).toBe("function");
    expect(typeof ChartTooltipContent).toBe("function");
    expect(typeof ChartLegendContent).toBe("function");
    expect(typeof chartSlotColor).toBe("function");
    expect(typeof chartPalette).toBe("function");
    expect(typeof resolveChartScheme).toBe("function");
    expect(typeof singleBarRadius).toBe("function");
    expect(typeof roundedEnds).toBe("function");
    expect(typeof stackedSegmentRadius).toBe("function");
    expect(typeof stackedBarRadii).toBe("function");
    expect(typeof mapRadarData).toBe("function");
    expect(typeof mapAreaData).toBe("function");
    expect(typeof mapBarData).toBe("function");
    expect(typeof mapBarPayloadValues).toBe("function");
    expect(typeof barSegmentKey).toBe("function");
    expect(typeof mapTooltipLabel).toBe("function");
    expect(typeof mapTooltipEntries).toBe("function");
    expect(typeof mapLegendEntries).toBe("function");
    expect(CHART_PALETTE_SLOTS).toBe(10);
    expect(BAR_END_RADIUS).toBe(4);
  });
});

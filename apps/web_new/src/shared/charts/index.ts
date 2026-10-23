// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the shared chart layer (Stack decision D15,
// NEWFRONT-732). Features import chart wrappers and helpers only
// from here.
export { AreaChart } from "./AreaChart.js";
export type { AreaChartProps, AreaSeriesDef } from "./AreaChart.js";
export { BarChart } from "./BarChart.js";
export type { BarChartProps, BarSegmentDef } from "./BarChart.js";
export { ChartLegendContent, ChartTooltipContent } from "./ChartTooltip.js";
export { CHART_PALETTE_SLOTS, chartPalette, chartSlotColor, resolveChartScheme } from "./colors.js";
export { RadarChart } from "./RadarChart.js";
export type { RadarChartProps } from "./RadarChart.js";
export type { BarShapeRenderProps, RadarTickProps, RechartsModule } from "./recharts.js";
export { BAR_END_RADIUS, singleBarRadius, stackedBarRadii, stackedSegmentRadius } from "./rounding.js";
export type { BarSegmentRadius } from "./rounding.js";
export {
  barSegmentKey,
  mapAreaData,
  mapBarData,
  mapBarPayloadValues,
  mapLegendEntries,
  mapRadarData,
  mapTooltipEntries,
  mapTooltipLabel,
} from "./series.js";
export type {
  AreaDatum,
  AreaPoint,
  BarDatum,
  BarInput,
  ChartLegendEntry,
  ChartTooltipEntry,
  RadarDatum,
  RadarPoint,
} from "./series.js";

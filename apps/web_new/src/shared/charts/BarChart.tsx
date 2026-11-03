// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared bar chart (Stack decision D15, NEWFRONT-732). Props-based
// wrapper over the lazily loaded plotting library: one bar per
// labeled category, single-series or stacked, with a hover tooltip
// naming every segment. Single-series bars round both ends;
// stacked bars round only the true extremes per bar (see
// rounding.ts), drawn through a custom segment shape. Colors
// resolve per segment from the caller's concrete colors with grid
// and tick chrome from kit tokens; callers pass display-ready
// labels. The library is only ever reached through the loader's
// dynamic import.
import { chartSchemeColors } from "@pidash/kit";
import * as React from "react";

import { useResolvedScheme } from "../../core/theme/index.js";
import { ChartTooltipContent } from "./ChartTooltip.js";
import { resolveChartScheme } from "./colors.js";
import type { BarShapeRenderProps, RechartsModule } from "./recharts.js";
import { roundedEnds, singleBarRadius, stackedSegmentRadius } from "./rounding.js";
import { barSegmentKey, mapBarData, mapBarPayloadValues, type BarInput } from "./series.js";

/** One stacked segment: its tooltip name and concrete color. */
export interface BarSegmentDef {
  name: string;
  color: string;
}

export interface BarChartProps {
  /** Bars in plotted order, with display-ready labels. */
  bars: readonly BarInput[];
  /** Segments in bottom-to-top order. */
  segments: readonly BarSegmentDef[];
  /** Stack the segments; single-series bars otherwise. */
  stacked?: boolean;
  /**
   * Per-bar fills in plotted order (NEWFRONT-733). Only read when
   * `stacked` is off: each series renders one cell per bar, so
   * single-series bars can each carry their own color. Entries past
   * the bars are ignored; bars past the entries keep the series
   * color.
   */
  fills?: readonly string[] | undefined;
  /** Plot height in pixels. */
  height?: number;
}

/** Plot height when the caller names none. */
const DEFAULT_BAR_HEIGHT = 320;

/** Shared stack id: bars with one id stack together. */
const BAR_STACK_ID = "segments";

interface BarPlotProps {
  bars: readonly BarInput[];
  segments: readonly BarSegmentDef[];
  stacked: boolean;
  fills: readonly string[] | undefined;
  height: number;
  grid: string;
  tick: string;
}

function createBarPlot(charts: RechartsModule): React.ComponentType<BarPlotProps> {
  const { Bar, BarChart: Plot, CartesianGrid, Cell, Rectangle, ResponsiveContainer, Tooltip, XAxis, YAxis } = charts;

  /** Custom segment shape: per-bar extreme rounding the stock bar cannot vary per datum. */
  function SegmentShape(
    shapeProps: BarShapeRenderProps,
    segmentIndex: number,
    segmentCount: number
  ): React.ReactElement {
    const radius = stackedSegmentRadius(mapBarPayloadValues(shapeProps.payload, segmentCount), segmentIndex);
    const ends = roundedEnds(radius);
    return (
      <Rectangle
        x={shapeProps.x}
        y={shapeProps.y}
        width={shapeProps.width}
        height={shapeProps.height}
        fill={shapeProps.fill}
        radius={radius}
        data-top-rounded={ends.top}
        data-bottom-rounded={ends.bottom}
      />
    );
  }

  function BarPlot({ bars, segments, stacked, fills, height, grid, tick }: BarPlotProps): React.ReactElement {
    return (
      <ResponsiveContainer width="100%" height={height}>
        <Plot data={mapBarData(bars, segments.length)}>
          <CartesianGrid stroke={grid} vertical={false} />
          <XAxis dataKey="label" tick={{ fill: tick, fontSize: 12 }} tickLine={false} axisLine={{ stroke: grid }} />
          <YAxis tick={{ fill: tick, fontSize: 12 }} tickLine={false} axisLine={false} allowDecimals={false} />
          <Tooltip content={<ChartTooltipContent />} cursor={{ stroke: grid }} />
          {segments.map((segment, index) => {
            const rounding = stacked
              ? {
                  stackId: BAR_STACK_ID,
                  shape: (shapeProps: BarShapeRenderProps) => SegmentShape(shapeProps, index, segments.length),
                }
              : { radius: singleBarRadius() };
            return (
              <Bar
                key={segment.name}
                dataKey={barSegmentKey(index)}
                name={segment.name}
                fill={segment.color}
                {...rounding}
              >
                {!stacked && fills !== undefined
                  ? bars.map((bar, barIndex) => (
                      <Cell key={`${bar.label}-${barIndex}`} fill={fills[barIndex] ?? segment.color} />
                    ))
                  : null}
              </Bar>
            );
          })}
        </Plot>
      </ResponsiveContainer>
    );
  }

  return BarPlot;
}

async function loadBarPlot(): Promise<{ default: React.ComponentType<BarPlotProps> }> {
  const charts = await import("recharts");
  return { default: createBarPlot(charts) };
}

const LazyBarPlot = React.lazy(loadBarPlot);

export function BarChart({
  bars,
  segments,
  stacked = false,
  fills,
  height = DEFAULT_BAR_HEIGHT,
}: BarChartProps): React.ReactElement {
  const { scheme, customDark } = useResolvedScheme();
  const colors = chartSchemeColors(resolveChartScheme(scheme, customDark));
  return (
    <div data-testid="chart-bar">
      <React.Suspense fallback={<div style={{ height }} aria-hidden="true" />}>
        <LazyBarPlot
          bars={bars}
          segments={segments}
          stacked={stacked}
          fills={fills}
          height={height}
          grid={colors.grid}
          tick={colors.tick}
        />
      </React.Suspense>
    </div>
  );
}

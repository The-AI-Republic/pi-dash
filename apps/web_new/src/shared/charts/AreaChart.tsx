// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared area chart (Stack decision D15, NEWFRONT-732). Props-based
// wrapper over the lazily loaded plotting library: smoothed
// multi-series areas over labeled points with a hover tooltip and
// an optional legend. Colors resolve per series from the caller's
// concrete colors with grid and tick chrome from kit tokens;
// callers pass display-ready labels. The library is only ever
// reached through the loader's dynamic import.
import { chartSchemeColors } from "@pidash/kit";
import * as React from "react";

import { useResolvedScheme } from "../../core/theme/index.js";
import { ChartLegendContent, ChartTooltipContent } from "./ChartTooltip.js";
import { resolveChartScheme } from "./colors.js";
import type { RechartsModule } from "./recharts.js";
import { mapAreaData, type AreaPoint } from "./series.js";

/** One plotted series: its data key, legend name, and concrete color. */
export interface AreaSeriesDef {
  key: string;
  name: string;
  color: string;
}

export interface AreaChartProps {
  /** Points in plotted order, with display-ready labels. */
  points: readonly AreaPoint[];
  /** Series in plotted and legend order. */
  series: readonly AreaSeriesDef[];
  /** Legend under the plot; on by default. */
  legend?: boolean;
  /** Plot height in pixels. */
  height?: number;
}

/** Plot height when the caller names none. */
const DEFAULT_AREA_HEIGHT = 320;

/** Partial fill under each area; the strokes stay solid. */
const AREA_FILL_OPACITY = 0.25;

interface AreaPlotProps {
  points: readonly AreaPoint[];
  series: readonly AreaSeriesDef[];
  legend: boolean;
  height: number;
  grid: string;
  tick: string;
}

function createAreaPlot(charts: RechartsModule): React.ComponentType<AreaPlotProps> {
  const { Area, AreaChart: Plot, CartesianGrid, Legend, ResponsiveContainer, Tooltip, XAxis, YAxis } = charts;

  function AreaPlot({ points, series, legend, height, grid, tick }: AreaPlotProps): React.ReactElement {
    return (
      <ResponsiveContainer width="100%" height={height}>
        <Plot
          data={mapAreaData(
            points,
            series.map((entry) => entry.key)
          )}
        >
          <CartesianGrid stroke={grid} vertical={false} />
          <XAxis dataKey="label" tick={{ fill: tick, fontSize: 12 }} tickLine={false} axisLine={{ stroke: grid }} />
          <YAxis tick={{ fill: tick, fontSize: 12 }} tickLine={false} axisLine={false} allowDecimals={false} />
          <Tooltip content={<ChartTooltipContent />} cursor={{ stroke: grid }} />
          {legend ? <Legend content={<ChartLegendContent />} verticalAlign="bottom" align="left" /> : null}
          {series.map((entry) => (
            <Area
              key={entry.key}
              type="monotone"
              dataKey={entry.key}
              name={entry.name}
              stroke={entry.color}
              fill={entry.color}
              fillOpacity={AREA_FILL_OPACITY}
              dot={false}
            />
          ))}
        </Plot>
      </ResponsiveContainer>
    );
  }

  return AreaPlot;
}

async function loadAreaPlot(): Promise<{ default: React.ComponentType<AreaPlotProps> }> {
  const charts = await import("recharts");
  return { default: createAreaPlot(charts) };
}

const LazyAreaPlot = React.lazy(loadAreaPlot);

export function AreaChart({
  points,
  series,
  legend = true,
  height = DEFAULT_AREA_HEIGHT,
}: AreaChartProps): React.ReactElement {
  const { scheme, customDark } = useResolvedScheme();
  const colors = chartSchemeColors(resolveChartScheme(scheme, customDark));
  return (
    <div data-testid="chart-area">
      <React.Suspense fallback={<div style={{ height }} aria-hidden="true" />}>
        <LazyAreaPlot
          points={points}
          series={series}
          legend={legend}
          height={height}
          grid={colors.grid}
          tick={colors.tick}
        />
      </React.Suspense>
    </div>
  );
}

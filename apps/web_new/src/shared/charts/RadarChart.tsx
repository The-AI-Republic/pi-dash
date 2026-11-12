// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared radar chart (Stack decision D15, NEWFRONT-732). Props-based
// wrapper over the lazily loaded plotting library: one named series
// over labeled points with an accent stroke and fill plus a hover
// tooltip. Colors resolve from kit tokens for the active scheme;
// callers pass display-ready labels. The library is only ever
// reached through the loader's dynamic import.
import { chartSchemeColors } from "@pidash/kit";
import * as React from "react";

import { useResolvedScheme } from "../../core/theme/index.js";
import { ChartTooltipContent } from "./ChartTooltip.js";
import { resolveChartScheme } from "./colors.js";
import type { RadarTickProps, RechartsModule } from "./recharts.js";
import { mapRadarData, mapTooltipLabel, type RadarPoint } from "./series.js";

export interface RadarChartProps {
  /** Points in plotted order, with display-ready labels. */
  points: readonly RadarPoint[];
  /** Series name (tooltip row and accessible label). */
  seriesName: string;
  /** Plot height in pixels. */
  height?: number;
}

/** Plot height when the caller names none. */
const DEFAULT_RADAR_HEIGHT = 320;

/** Partial fill for the radar area; the stroke stays solid. */
const RADAR_FILL_OPACITY = 0.35;

interface RadarPlotProps {
  points: readonly RadarPoint[];
  seriesName: string;
  height: number;
  accent: string;
  grid: string;
  tick: string;
}

function createRadarPlot(charts: RechartsModule): React.ComponentType<RadarPlotProps> {
  const { PolarAngleAxis, PolarGrid, Radar, RadarChart: Plot, ResponsiveContainer, Tooltip } = charts;

  function RadarPlot({ points, seriesName, height, accent, grid, tick }: RadarPlotProps): React.ReactElement {
    return (
      <ResponsiveContainer width="100%" height={height}>
        <Plot data={mapRadarData(points)} margin={{ top: 8, right: 24, bottom: 8, left: 24 }} outerRadius="72%">
          <PolarGrid stroke={grid} />
          <PolarAngleAxis
            dataKey="dimension"
            tick={(tickProps: RadarTickProps) => (
              <text
                x={tickProps.x}
                y={tickProps.y}
                textAnchor={tickProps.textAnchor}
                fill={tick}
                fontSize={12}
                data-testid="chart-radar-tick"
              >
                {mapTooltipLabel(tickProps.payload.value)}
              </text>
            )}
          />
          <Radar
            dataKey="count"
            name={seriesName}
            stroke={accent}
            fill={accent}
            fillOpacity={RADAR_FILL_OPACITY}
            dot={{ r: 3, fill: accent, fillOpacity: 1 }}
          />
          <Tooltip content={<ChartTooltipContent />} />
        </Plot>
      </ResponsiveContainer>
    );
  }

  return RadarPlot;
}

async function loadRadarPlot(): Promise<{ default: React.ComponentType<RadarPlotProps> }> {
  const charts = await import("recharts");
  return { default: createRadarPlot(charts) };
}

const LazyRadarPlot = React.lazy(loadRadarPlot);

export function RadarChart({ points, seriesName, height = DEFAULT_RADAR_HEIGHT }: RadarChartProps): React.ReactElement {
  const { scheme, customDark } = useResolvedScheme();
  const colors = chartSchemeColors(resolveChartScheme(scheme, customDark));
  return (
    <div data-testid="chart-radar">
      <React.Suspense fallback={<div style={{ height }} aria-hidden="true" />}>
        <LazyRadarPlot
          points={points}
          seriesName={seriesName}
          height={height}
          accent={colors.accent}
          grid={colors.grid}
          tick={colors.tick}
        />
      </React.Suspense>
    </div>
  );
}

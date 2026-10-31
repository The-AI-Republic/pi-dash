// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared radar chart tests (NEWFRONT-732). The plotting library is
// stubbed: the suite owns the wrapper's mapping (points onto rows,
// kit colors for the scheme, series naming), never the library.
import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("recharts", () => ({
  ResponsiveContainer: ({ children, height }: { children?: ReactNode; height?: number }) => (
    <div data-testid="mock-responsive" data-height={height}>
      {children}
    </div>
  ),
  RadarChart: ({ children, data }: { children?: ReactNode; data?: Array<Record<string, unknown>> }) => (
    <div data-testid="mock-radar-chart" data-rows={JSON.stringify(data)}>
      {children}
    </div>
  ),
  PolarGrid: ({ stroke }: { stroke?: string }) => <div data-testid="mock-polar-grid" data-stroke={stroke} />,
  PolarAngleAxis: ({ tick }: { tick?: (props: Record<string, unknown>) => ReactNode }) => (
    <div data-testid="mock-angle-axis">
      <svg>{tick?.({ x: 1, y: 2, textAnchor: "middle", payload: { value: "Work Items" } })}</svg>
    </div>
  ),
  Radar: (props: Record<string, unknown>) => (
    <div
      data-testid="mock-radar"
      data-datakey={props["dataKey"] as string}
      data-name={props["name"] as string}
      data-stroke={props["stroke"] as string}
      data-fill={props["fill"] as string}
      data-fillopacity={props["fillOpacity"] as number}
      data-dot={JSON.stringify(props["dot"])}
    />
  ),
  Tooltip: ({ content }: { content?: ReactNode }) => <div data-testid="mock-tooltip">{content}</div>,
}));

import { RadarChart } from "./RadarChart.js";

afterEach(() => {
  document.documentElement.removeAttribute("data-theme");
});

const POINTS = [
  { label: "Work Items", value: 2 },
  { label: "Cycles", value: 0 },
];

describe("RadarChart", () => {
  it("plots the mapped points with kit light colors", async () => {
    render(<RadarChart points={POINTS} seriesName="Count" />);
    const chart = await screen.findByTestId("mock-radar-chart");
    expect(chart.dataset["rows"]).toBe(
      JSON.stringify([
        { dimension: "Work Items", count: 2 },
        { dimension: "Cycles", count: 0 },
      ])
    );
    const radar = screen.getByTestId("mock-radar");
    expect(radar.dataset["datakey"]).toBe("count");
    expect(radar.dataset["name"]).toBe("Count");
    expect(radar.dataset["stroke"]).toBe("#5e6ad2");
    expect(radar.dataset["fill"]).toBe("#5e6ad2");
    expect(radar.dataset["fillopacity"]).toBe("0.35");
    expect(radar.dataset["dot"]).toBe(JSON.stringify({ r: 3, fill: "#5e6ad2", fillOpacity: 1 }));
    expect(screen.getByTestId("mock-polar-grid").dataset["stroke"]).toBe("#e4e4e7");
    const ticks = screen.getAllByTestId("chart-radar-tick");
    expect(ticks).toHaveLength(1);
    expect(ticks[0]).toHaveTextContent("Work Items");
    expect(ticks[0]?.getAttribute("fill")).toBe("#71717a");
    expect(screen.getByTestId("mock-tooltip")).toBeInTheDocument();
    expect(screen.getByTestId("mock-responsive").dataset["height"]).toBe("320");
  });

  it("resolves kit dark colors under the dark document theme", async () => {
    document.documentElement.setAttribute("data-theme", "dark");
    render(<RadarChart points={POINTS} seriesName="Count" height={240} />);
    const radar = await screen.findByTestId("mock-radar");
    expect(radar.dataset["stroke"]).toBe("#7c85e0");
    expect(radar.dataset["fill"]).toBe("#7c85e0");
    expect(screen.getByTestId("mock-polar-grid").dataset["stroke"]).toBe("#26262a");
    expect(screen.getByTestId("chart-radar-tick").getAttribute("fill")).toBe("#8b8b93");
    expect(screen.getByTestId("mock-responsive").dataset["height"]).toBe("240");
  });

  it("plots no rows for no points", async () => {
    render(<RadarChart points={[]} seriesName="Count" />);
    const chart = await screen.findByTestId("mock-radar-chart");
    expect(chart.dataset["rows"]).toBe(JSON.stringify([]));
  });
});

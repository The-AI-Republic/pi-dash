// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared area chart tests (NEWFRONT-732). The plotting library is
// stubbed: the suite owns the wrapper's mapping (points onto rows,
// per-series colors, legend placement), never the library.
import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("recharts", () => ({
  ResponsiveContainer: ({ children, height }: { children?: ReactNode; height?: number }) => (
    <div data-testid="mock-responsive" data-height={height}>
      {children}
    </div>
  ),
  AreaChart: ({ children, data }: { children?: ReactNode; data?: Array<Record<string, unknown>> }) => (
    <div data-testid="mock-area-chart" data-rows={JSON.stringify(data)}>
      {children}
    </div>
  ),
  CartesianGrid: ({ stroke }: { stroke?: string }) => <div data-testid="mock-grid" data-stroke={stroke} />,
  XAxis: ({ tick }: { tick?: Record<string, unknown> }) => (
    <div data-testid="mock-x-axis" data-tick={JSON.stringify(tick)} />
  ),
  YAxis: () => <div data-testid="mock-y-axis" />,
  Area: (props: Record<string, unknown>) => (
    <div
      data-testid="mock-area"
      data-datakey={props["dataKey"] as string}
      data-name={props["name"] as string}
      data-stroke={props["stroke"] as string}
      data-fill={props["fill"] as string}
      data-type={props["type"] as string}
      data-dot={String(props["dot"])}
    />
  ),
  Legend: ({ content }: { content?: ReactNode }) => <div data-testid="mock-legend">{content}</div>,
  Tooltip: ({ content }: { content?: ReactNode }) => <div data-testid="mock-tooltip">{content}</div>,
}));

import { AreaChart } from "./AreaChart.js";

afterEach(() => {
  document.documentElement.removeAttribute("data-theme");
});

const POINTS = [
  { label: "Oct 01, 2026", values: { resolved: 3, created: 5 } },
  { label: "Nov 01, 2026", values: { resolved: 1 } },
];
const SERIES = [
  { key: "resolved", name: "Resolved", color: "#2f9e5b" },
  { key: "created", name: "Created", color: "#3aa7c1" },
];

describe("AreaChart", () => {
  it("plots smoothed series without dots behind a legend", async () => {
    render(<AreaChart points={POINTS} series={SERIES} />);
    const chart = await screen.findByTestId("mock-area-chart");
    expect(chart.dataset["rows"]).toBe(
      JSON.stringify([
        { label: "Oct 01, 2026", resolved: 3, created: 5 },
        { label: "Nov 01, 2026", resolved: 1, created: 0 },
      ])
    );
    const areas = screen.getAllByTestId("mock-area");
    expect(areas.map((node) => node.dataset["datakey"])).toEqual(["resolved", "created"]);
    expect(areas.map((node) => node.dataset["name"])).toEqual(["Resolved", "Created"]);
    expect(areas.map((node) => node.dataset["stroke"])).toEqual(["#2f9e5b", "#3aa7c1"]);
    expect(areas.map((node) => node.dataset["fill"])).toEqual(["#2f9e5b", "#3aa7c1"]);
    expect(areas.map((node) => node.dataset["type"])).toEqual(["monotone", "monotone"]);
    expect(areas.map((node) => node.dataset["dot"])).toEqual(["false", "false"]);
    expect(screen.getByTestId("mock-legend")).toBeInTheDocument();
    expect(screen.getByTestId("mock-grid").dataset["stroke"]).toBe("#e4e4e7");
    expect(screen.getByTestId("mock-x-axis").dataset["tick"]).toBe(JSON.stringify({ fill: "#71717a", fontSize: 12 }));
  });

  it("omits the legend when asked", async () => {
    render(<AreaChart points={POINTS} series={SERIES} legend={false} />);
    await screen.findByTestId("mock-area-chart");
    expect(screen.queryByTestId("mock-legend")).not.toBeInTheDocument();
  });

  it("resolves kit dark chrome under the dark document theme", async () => {
    document.documentElement.setAttribute("data-theme", "dark");
    render(<AreaChart points={POINTS} series={SERIES} height={240} />);
    await screen.findByTestId("mock-area-chart");
    expect(screen.getByTestId("mock-grid").dataset["stroke"]).toBe("#26262a");
    expect(screen.getByTestId("mock-x-axis").dataset["tick"]).toBe(JSON.stringify({ fill: "#8b8b93", fontSize: 12 }));
    expect(screen.getByTestId("mock-responsive").dataset["height"]).toBe("240");
  });
});

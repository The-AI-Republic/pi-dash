// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared bar chart tests (NEWFRONT-732). The plotting library is
// stubbed: the suite owns the wrapper's mapping (bars onto rows,
// per-segment colors, stacking, extreme rounding), never the
// library.
import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

interface FakeShapeProps {
  x: number;
  y: number;
  width: number;
  height: number;
  fill?: string | undefined;
  payload?: unknown;
}

vi.mock("recharts", () => ({
  ResponsiveContainer: ({ children, height }: { children?: ReactNode; height?: number }) => (
    <div data-testid="mock-responsive" data-height={height}>
      {children}
    </div>
  ),
  BarChart: ({ children, data }: { children?: ReactNode; data?: Array<Record<string, unknown>> }) => (
    <div data-testid="mock-bar-chart" data-rows={JSON.stringify(data)}>
      {children}
    </div>
  ),
  CartesianGrid: ({ stroke }: { stroke?: string }) => <div data-testid="mock-grid" data-stroke={stroke} />,
  XAxis: ({ tick }: { tick?: Record<string, unknown> }) => (
    <div data-testid="mock-x-axis" data-tick={JSON.stringify(tick)} />
  ),
  YAxis: () => <div data-testid="mock-y-axis" />,
  Bar: (props: {
    dataKey?: string;
    name?: string;
    fill?: string;
    stackId?: string;
    radius?: unknown;
    shape?: (shapeProps: FakeShapeProps) => ReactNode;
  }) => (
    <div
      data-testid="mock-bar"
      data-datakey={props.dataKey}
      data-name={props.name}
      data-fill={props.fill}
      data-stackid={props.stackId}
      data-radius={props.radius === undefined ? undefined : JSON.stringify(props.radius)}
      data-shape={props.shape === undefined ? "none" : "custom"}
    >
      {props.shape?.({
        x: 0,
        y: 0,
        width: 10,
        height: 20,
        fill: props.fill,
        payload: { label: "Urgent", "segment-0": 2, "segment-1": 0 },
      })}
    </div>
  ),
  Rectangle: (props: { radius?: unknown }) => (
    <div data-testid="mock-rectangle" data-radius={JSON.stringify(props.radius)} />
  ),
  Tooltip: ({ content }: { content?: ReactNode }) => <div data-testid="mock-tooltip">{content}</div>,
}));

import { BarChart } from "./BarChart.js";

afterEach(() => {
  document.documentElement.removeAttribute("data-theme");
});

const BARS = [
  { label: "Urgent", values: [2, 1] },
  { label: "High", values: [0, 4] },
];
const SEGMENTS = [
  { name: "Alpha", color: "#6172e8" },
  { name: "Beta", color: "#8b6edb" },
];

describe("BarChart", () => {
  it("plots single-series bars with both ends rounded", async () => {
    render(<BarChart bars={BARS} segments={SEGMENTS} />);
    const chart = await screen.findByTestId("mock-bar-chart");
    expect(chart.dataset["rows"]).toBe(
      JSON.stringify([
        { label: "Urgent", "segment-0": 2, "segment-1": 1 },
        { label: "High", "segment-0": 0, "segment-1": 4 },
      ])
    );
    const bars = screen.getAllByTestId("mock-bar");
    expect(bars.map((node) => node.dataset["datakey"])).toEqual(["segment-0", "segment-1"]);
    expect(bars.map((node) => node.dataset["name"])).toEqual(["Alpha", "Beta"]);
    expect(bars.map((node) => node.dataset["fill"])).toEqual(["#6172e8", "#8b6edb"]);
    expect(bars.map((node) => node.getAttribute("data-stackid"))).toEqual([null, null]);
    expect(bars.map((node) => node.dataset["radius"])).toEqual([
      JSON.stringify([4, 4, 4, 4]),
      JSON.stringify([4, 4, 4, 4]),
    ]);
    expect(bars.map((node) => node.dataset["shape"])).toEqual(["none", "none"]);
    expect(screen.queryByTestId("mock-rectangle")).not.toBeInTheDocument();
    expect(screen.getByTestId("mock-grid").dataset["stroke"]).toBe("#e4e4e7");
  });

  it("stacks segments with per-bar extreme rounding", async () => {
    render(<BarChart bars={BARS} segments={SEGMENTS} stacked />);
    await screen.findByTestId("mock-bar-chart");
    const bars = screen.getAllByTestId("mock-bar");
    expect(bars.map((node) => node.dataset["stackid"])).toEqual(["segments", "segments"]);
    expect(bars.map((node) => node.getAttribute("data-radius"))).toEqual([null, null]);
    expect(bars.map((node) => node.dataset["shape"])).toEqual(["custom", "custom"]);
    // The stubbed datum reads [2, 0]: segment 0 is the lone nonzero
    // segment (both ends), segment 1 is zero (square).
    const shapes = screen.getAllByTestId("mock-rectangle");
    expect(shapes.map((node) => node.dataset["radius"])).toEqual([
      JSON.stringify([4, 4, 4, 4]),
      JSON.stringify([0, 0, 0, 0]),
    ]);
  });

  it("resolves kit dark chrome under the dark document theme", async () => {
    document.documentElement.setAttribute("data-theme", "dark");
    render(<BarChart bars={BARS} segments={SEGMENTS} height={240} />);
    await screen.findByTestId("mock-bar-chart");
    expect(screen.getByTestId("mock-grid").dataset["stroke"]).toBe("#26262a");
    expect(screen.getByTestId("mock-x-axis").dataset["tick"]).toBe(JSON.stringify({ fill: "#8b8b93", fontSize: 12 }));
    expect(screen.getByTestId("mock-responsive").dataset["height"]).toBe("240");
  });
});

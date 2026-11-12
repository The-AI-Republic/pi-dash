// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom-insights picker tests (parity/specs/analytics.md §10, ANL-010).
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AnalyticsXAxis, AnalyticsYAxis } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

import { CustomInsightPickers } from "./CustomInsightPickers.js";

const X_LABELS = [
  "State name",
  "State group",
  "Priority",
  "Label",
  "Assignee",
  "Estimate point",
  "Cycle",
  "Module",
  "Completed date",
  "Due date",
  "Start date",
  "Created date",
];

function renderPickers(
  props: {
    metric?: AnalyticsYAxis;
    xAxis?: AnalyticsXAxis;
    split?: AnalyticsXAxis | null;
    onMetricChange?: (metric: AnalyticsYAxis) => void;
    onXAxisChange?: (axis: AnalyticsXAxis) => void;
    onSplitChange?: (axis: AnalyticsXAxis | null) => void;
  } = {}
): void {
  render(
    <CustomInsightPickers
      metric={props.metric ?? "WORK_ITEM_COUNT"}
      xAxis={props.xAxis ?? "PRIORITY"}
      split={props.split ?? null}
      onMetricChange={props.onMetricChange ?? (() => {})}
      onXAxisChange={props.onXAxisChange ?? (() => {})}
      onSplitChange={props.onSplitChange ?? (() => {})}
    />
  );
}

function optionLabels(): string[] {
  return within(screen.getByRole("listbox"))
    .getAllByRole("option")
    .map((row) => row.querySelector(":scope > span.truncate")?.textContent ?? "");
}

describe("CustomInsightPickers", () => {
  it("opens on the metric, priority, and an empty split", () => {
    renderPickers();
    expect(screen.getByTestId("analytics-custom-trigger-0")).toHaveTextContent("Work item");
    expect(screen.getByTestId("analytics-custom-trigger-1")).toHaveTextContent("Priority");
    expect(screen.getByTestId("analytics-custom-trigger-2")).toHaveTextContent("Add Property");
  });

  it("offers only the in-scope count on the metric picker", async () => {
    const user = userEvent.setup();
    const onMetricChange = vi.fn();
    renderPickers({ onMetricChange });
    await user.click(screen.getByTestId("analytics-custom-trigger-0"));
    await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
    expect(optionLabels()).toEqual(["Work item"]);
    await user.click(within(screen.getByRole("listbox")).getByRole("option", { name: "Work item" }));
    expect(onMetricChange).toHaveBeenCalledWith("WORK_ITEM_COUNT");
  });

  it("offers every dimension on x while the split excludes the chosen one", async () => {
    const user = userEvent.setup();
    renderPickers();
    await user.click(screen.getByTestId("analytics-custom-trigger-1"));
    await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
    expect(optionLabels()).toEqual(X_LABELS);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("listbox")).not.toBeInTheDocument());
    await user.click(screen.getByTestId("analytics-custom-trigger-2"));
    await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
    expect(optionLabels()).toEqual(["No value", ...X_LABELS.filter((label) => label !== "Priority")]);
  });

  it("chooses a split through onSplitChange", async () => {
    const user = userEvent.setup();
    const onSplitChange = vi.fn();
    renderPickers({ onSplitChange });
    await user.click(screen.getByTestId("analytics-custom-trigger-2"));
    await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
    await user.click(within(screen.getByRole("listbox")).getByRole("option", { name: "State name" }));
    expect(onSplitChange).toHaveBeenCalledWith("STATES");
  });

  it("clears the split back through No value", async () => {
    const user = userEvent.setup();
    const onSplitChange = vi.fn();
    renderPickers({ split: "STATES", onSplitChange });
    expect(screen.getByTestId("analytics-custom-trigger-2")).toHaveTextContent("State name");
    await user.click(screen.getByTestId("analytics-custom-trigger-2"));
    await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
    await user.click(within(screen.getByRole("listbox")).getByRole("option", { name: "No value" }));
    expect(onSplitChange).toHaveBeenCalledWith(null);
  });

  it("narrows the x picker while a split is chosen", async () => {
    const user = userEvent.setup();
    renderPickers({ split: "STATES" });
    await user.click(screen.getByTestId("analytics-custom-trigger-1"));
    await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
    expect(optionLabels()).toEqual(X_LABELS.filter((label) => label !== "State name"));
  });

  it("moves x through onXAxisChange", async () => {
    const user = userEvent.setup();
    const onXAxisChange = vi.fn();
    renderPickers({ onXAxisChange });
    await user.click(screen.getByTestId("analytics-custom-trigger-1"));
    await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
    await user.click(within(screen.getByRole("listbox")).getByRole("option", { name: "Due date" }));
    expect(onXAxisChange).toHaveBeenCalledWith("TARGET_DATE");
  });
});

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom-insights pickers (parity/specs/analytics.md §10,
// ANL-010). The three axis controls — metric, x property, and the
// optional second split — over the fixed picker model. The two
// property pickers exclude each other's choice; the metric picker
// holds the settled single count. Presentational: the section
// owns the selection state and the re-query.
import { Button, Combobox, type ComboboxOption } from "@pidash/kit";
import type { AnalyticsXAxis, AnalyticsYAxis } from "@pidash/api-client";
import * as React from "react";

import {
  CUSTOM_METRICS,
  customAxisLabel,
  customMetricLabel,
  selectCustomSplitOptions,
  selectCustomXOptions,
} from "../lib/customAxes.js";

export interface CustomInsightPickersProps {
  metric: AnalyticsYAxis;
  xAxis: AnalyticsXAxis;
  split: AnalyticsXAxis | null;
  onMetricChange: (metric: AnalyticsYAxis) => void;
  onXAxisChange: (axis: AnalyticsXAxis) => void;
  onSplitChange: (axis: AnalyticsXAxis | null) => void;
}

/** Listbox id for the cleared split; never collides with an axis value. */
const SPLIT_CLEAR_ID = "";

function metricOptions(): ComboboxOption[] {
  return CUSTOM_METRICS.map((entry) => ({ id: entry.value, label: entry.label }));
}

function xAxisOptions(split: AnalyticsXAxis | null): ComboboxOption[] {
  return selectCustomXOptions(split).map((entry) => ({ id: entry.value, label: entry.label }));
}

function splitOptions(xAxis: AnalyticsXAxis): ComboboxOption[] {
  return selectCustomSplitOptions(xAxis).map((entry) => ({
    id: entry.value ?? SPLIT_CLEAR_ID,
    label: entry.label,
  }));
}

function PickerCaption({ children }: { children: React.ReactNode }): React.ReactElement {
  return <span className="text-meta text-(--text-muted)">{children}</span>;
}

export function CustomInsightPickers({
  metric,
  xAxis,
  split,
  onMetricChange,
  onXAxisChange,
  onSplitChange,
}: CustomInsightPickersProps): React.ReactElement {
  return (
    <div data-testid="analytics-custom-pickers" className="flex flex-wrap items-end gap-(--space-3)">
      <div className="flex flex-col gap-(--space-1)">
        <PickerCaption>Metric</PickerCaption>
        <Combobox
          trigger={
            <Button variant="secondary" data-testid="analytics-custom-trigger-0">
              <span className="truncate">{customMetricLabel(metric)}</span>
            </Button>
          }
          options={metricOptions()}
          value={metric}
          onSelect={(id) => onMetricChange(id as AnalyticsYAxis)}
          listLabel="Metric"
          searchEnabled={false}
          emptyText="No metrics"
        />
      </div>
      <div className="flex flex-col gap-(--space-1)">
        <PickerCaption>X axis</PickerCaption>
        <Combobox
          trigger={
            <Button variant="secondary" data-testid="analytics-custom-trigger-1">
              <span className="truncate">{customAxisLabel(xAxis)}</span>
            </Button>
          }
          options={xAxisOptions(split)}
          value={xAxis}
          onSelect={(id) => onXAxisChange(id as AnalyticsXAxis)}
          listLabel="X axis"
          searchEnabled={false}
          emptyText="No dimensions"
        />
      </div>
      <div className="flex flex-col gap-(--space-1)">
        <PickerCaption>Split</PickerCaption>
        <Combobox
          trigger={
            <Button variant="secondary" data-testid="analytics-custom-trigger-2">
              <span className="truncate">{customAxisLabel(split)}</span>
            </Button>
          }
          options={splitOptions(xAxis)}
          value={split ?? SPLIT_CLEAR_ID}
          onSelect={(id) => onSplitChange(id === SPLIT_CLEAR_ID ? null : (id as AnalyticsXAxis))}
          listLabel="Split"
          searchEnabled={false}
          emptyText="No dimensions"
        />
      </div>
    </div>
  );
}

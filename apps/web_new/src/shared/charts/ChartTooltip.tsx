// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared chart hover tooltip and legend content (NEWFRONT-732).
// Passed to the plotting library as content elements; the library
// injects the hovered payload or legend rows as props. Reads run
// defensively off the injected record so no library type leaks into
// the wrappers' APIs. Styled from kit tokens.
import * as React from "react";

import { mapLegendEntries, mapTooltipEntries, mapTooltipLabel } from "./series.js";

/**
 * Hover tooltip body. Renders nothing without hovered entries; the
 * library only mounts content while hovering, so that branch is
 * defensive.
 */
export function ChartTooltipContent(props: Record<string, unknown>): React.ReactElement | null {
  const entries = mapTooltipEntries(props["payload"]);
  if (entries.length === 0) return null;
  return (
    <div className="shadow-popover flex min-w-32 flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-3) py-(--space-2)">
      <div data-testid="chart-tooltip-label" className="text-meta font-medium text-(--text)">
        {mapTooltipLabel(props["label"])}
      </div>
      {entries.map((entry, index) => (
        <div
          key={`${entry.name}-${index}`}
          data-testid="chart-tooltip-row"
          className="flex items-center gap-(--space-2)"
        >
          {entry.color === undefined ? null : (
            <span
              aria-hidden="true"
              style={{ backgroundColor: entry.color }}
              className="size-2 shrink-0 rounded-full"
            />
          )}
          <span data-testid="chart-tooltip-name" className="text-meta flex-1 text-(--text-muted)">
            {entry.name}
          </span>
          <span data-testid="chart-tooltip-value" className="text-meta font-medium text-(--text)">
            {entry.value}
          </span>
        </div>
      ))}
    </div>
  );
}

/**
 * Legend body for charts that carry one. Renders nothing without
 * legend rows; one swatch plus label per series, in series order.
 */
export function ChartLegendContent(props: Record<string, unknown>): React.ReactElement | null {
  const entries = mapLegendEntries(props["payload"]);
  if (entries.length === 0) return null;
  return (
    <div data-testid="chart-legend" className="flex flex-wrap items-center gap-(--space-4)">
      {entries.map((entry, index) => (
        <span key={`${entry.label}-${index}`} className="inline-flex items-center gap-(--space-2)">
          <span
            aria-hidden="true"
            style={{ backgroundColor: entry.color ?? "transparent" }}
            className="size-2 shrink-0 rounded-full"
          />
          <span className="text-meta text-(--text-muted)">{entry.label}</span>
        </span>
      ))}
    </div>
  );
}

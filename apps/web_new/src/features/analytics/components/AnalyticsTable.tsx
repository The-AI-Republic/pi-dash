// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reusable analytics table chrome (parity/specs/analytics.md §§11, 12,
// 17; ANL-012, ANL-013, ANL-017). Renders the column model, a fixed-noun
// row-count label, an expanding first-column search that clears on close,
// an optional export action, and loading/empty branches. The table never
// sorts or paginates; callers supply display order.
import { Button, IconButton, Input, Skeleton } from "@pidash/kit";
import { Search, X } from "lucide-react";
import * as React from "react";

import { filterByFirstColumn, rowCountLabel, type AnalyticsColumn } from "../lib/table.js";
import { AnalyticsSectionGuidance } from "./AnalyticsSectionGuidance.js";

export interface AnalyticsTableProps<Row> {
  /** Stable section id used by the parity driver. */
  testId: string;
  columns: readonly AnalyticsColumn<Row>[];
  rows: readonly Row[];
  /** Fixed noun in the row-count label, e.g. `Projects` or the x-axis label. */
  countNoun: string;
  loading?: boolean;
  /** Guidance copy for an empty read or a filter that matches no rows. */
  emptyText?: string | null;
  /** Number of skeleton body rows while loading. */
  skeletonRows?: number;
  /** Handles the filtered rows; omitted means no export action. */
  onExport?: ((rows: readonly Row[]) => void) | undefined;
}

/** Empty search input label; the placeholder is part of the driver contract. */
const SEARCH_LABEL = "Search rows";
const SEARCH_PLACEHOLDER = "Search";
const EXPORT_LABEL = "Export as csv";

function CellContent<Row>({ column, row }: { column: AnalyticsColumn<Row>; row: Row }): React.ReactElement {
  if (column.render !== undefined) {
    return (
      <div className={column.align === "right" ? "flex justify-end" : undefined}>
        {column.render(row) as React.ReactNode}
      </div>
    );
  }
  return <>{column.text(row)}</>;
}

export function AnalyticsTable<Row>({
  testId,
  columns,
  rows,
  countNoun,
  loading = false,
  emptyText = null,
  skeletonRows = 5,
  onExport,
}: AnalyticsTableProps<Row>): React.ReactElement {
  const [searchOpen, setSearchOpen] = React.useState(false);
  const [query, setQuery] = React.useState("");
  const filtered = React.useMemo(() => filterByFirstColumn(rows, query, columns), [rows, query, columns]);

  const closeSearch = (): void => {
    setQuery("");
    setSearchOpen(false);
  };

  const handleExport = (): void => {
    if (onExport === undefined || filtered.length === 0) return;
    onExport(filtered);
  };

  return (
    <div data-testid={testId} className="flex min-w-0 flex-col gap-(--space-3)">
      <div className="flex flex-wrap items-center gap-(--space-2)">
        <div data-testid="analytics-table-count" className="text-meta whitespace-nowrap text-(--text-muted)">
          {rowCountLabel(rows.length, countNoun)}
        </div>
        <div className="ml-auto flex items-center gap-(--space-2)">
          {searchOpen ? (
            <>
              <IconButton label="Close search" size="small" onClick={closeSearch}>
                <X aria-hidden="true" className="size-4" />
              </IconButton>
              <Input
                label={SEARCH_LABEL}
                placeholder={SEARCH_PLACEHOLDER}
                hideLabel
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                autoFocus
                className="h-8 w-48"
              />
            </>
          ) : (
            <IconButton label="Search" size="small" onClick={() => setSearchOpen(true)}>
              <Search aria-hidden="true" className="size-4" />
            </IconButton>
          )}
          {onExport === undefined || loading ? null : (
            <Button variant="secondary" size="small" onClick={handleExport}>
              {EXPORT_LABEL}
            </Button>
          )}
        </div>
      </div>
      <div className="min-w-0 overflow-x-auto">
        <table data-testid="analytics-table" className="w-full min-w-max border-collapse text-left">
          <thead>
            <tr className="border-b border-(--border)">
              {columns.map((column) => (
                <th
                  key={column.key}
                  scope="col"
                  className={`text-meta px-(--space-3) py-(--space-2) font-medium text-(--text-muted) ${column.align === "right" ? "text-right" : "text-left"}`}
                >
                  {column.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {loading ? (
              Array.from({ length: skeletonRows }, (_, rowIndex) => (
                <tr key={`skeleton-${rowIndex}`} className="border-b border-(--border)">
                  {columns.map((column) => (
                    <td key={column.key} className="px-(--space-3) py-(--space-2)">
                      <Skeleton height="20px" />
                    </td>
                  ))}
                </tr>
              ))
            ) : filtered.length === 0 && emptyText !== null ? (
              <tr>
                <td colSpan={Math.max(columns.length, 1)} className="px-(--space-3) py-(--space-4)">
                  <AnalyticsSectionGuidance title={emptyText} description="" />
                </td>
              </tr>
            ) : (
              filtered.map((row, index) => (
                <tr key={index} className="border-b border-(--border)">
                  {columns.map((column) => (
                    <td
                      key={column.key}
                      className={`text-body px-(--space-3) py-(--space-2) text-(--text) ${column.align === "right" ? "text-right" : "text-left"}`}
                    >
                      <CellContent column={column} row={row} />
                    </td>
                  ))}
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

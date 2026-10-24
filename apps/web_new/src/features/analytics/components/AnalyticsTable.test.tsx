// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// @vitest-environment jsdom
// Reusable analytics table chrome tests (parity/specs/analytics.md §17,
// ANL-017).
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { AnalyticsColumn } from "../lib/table.js";
import { AnalyticsTable } from "./AnalyticsTable.js";

interface Row {
  name: string;
  count: number;
}

const COLUMNS: AnalyticsColumn<Row>[] = [
  { key: "name", label: "Name", align: "left", text: (row) => row.name, exportValue: (row) => row.name },
  { key: "count", label: "Count", align: "right", text: (row) => String(row.count), exportValue: (row) => row.count },
];

const ROWS: Row[] = [
  { name: "Urgent", count: 1 },
  { name: "High", count: 2 },
];

function setup(overrides: Partial<React.ComponentProps<typeof AnalyticsTable<Row>>> = {}) {
  const onExport = vi.fn();
  render(
    <AnalyticsTable
      testId="analytics-test-table"
      columns={COLUMNS}
      rows={ROWS}
      countNoun="Items"
      onExport={onExport}
      {...overrides}
    />
  );
  return { onExport };
}

describe("AnalyticsTable", () => {
  it("renders headers, body rows and the loaded row count", () => {
    setup();
    expect(screen.getByRole("columnheader", { name: "Name" })).toBeInTheDocument();
    expect(screen.getAllByRole("row")).toHaveLength(3);
    expect(screen.getByTestId("analytics-table-count")).toHaveTextContent("2 Items");
  });

  it("uses the fixed count label even while search narrows rows", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.type(screen.getByPlaceholderText("Search"), "urgent");
    expect(screen.getByTestId("analytics-table-count")).toHaveTextContent("2 Items");
    expect(screen.getAllByRole("row")).toHaveLength(2);
  });

  it("expands the search, filters the first column and clears on close", async () => {
    const user = userEvent.setup();
    setup();
    expect(screen.queryByPlaceholderText("Search")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Search" }));
    const box = screen.getByPlaceholderText("Search");
    expect(box).toBeVisible();
    expect(screen.getByText("Search rows")).toHaveClass("sr-only");
    await user.type(box, "hig");
    expect(screen.getByText("High")).toBeInTheDocument();
    expect(screen.queryByText("Urgent")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Close search" }));
    expect(screen.queryByPlaceholderText("Search")).not.toBeInTheDocument();
    expect(screen.getByText("Urgent")).toBeInTheDocument();
    expect(screen.getByText("High")).toBeInTheDocument();
  });

  it("exports only the visible rows", async () => {
    const user = userEvent.setup();
    const { onExport } = setup();
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.type(screen.getByPlaceholderText("Search"), "urgent");
    await user.click(screen.getByRole("button", { name: "Export as csv" }));
    expect(onExport).toHaveBeenCalledWith([{ name: "Urgent", count: 1 }]);
  });

  it("does not export when the filter leaves no rows", async () => {
    const user = userEvent.setup();
    const { onExport } = setup({ emptyText: "Create projects with work items." });
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.type(screen.getByPlaceholderText("Search"), "no match");
    expect(screen.getAllByRole("row")).toHaveLength(2);
    expect(screen.getAllByRole("cell")).toHaveLength(1);
    expect(screen.getByText("Create projects with work items.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Export as csv" }));
    expect(onExport).not.toHaveBeenCalled();
  });

  it("renders skeleton rows instead of data while loading", () => {
    setup({ loading: true, skeletonRows: 3 });
    expect(screen.queryByText("Urgent")).not.toBeInTheDocument();
    expect(document.querySelectorAll("tbody td > span[style*='height'][style*='20px']")).toHaveLength(6);
    expect(screen.queryByRole("button", { name: "Export as csv" })).not.toBeInTheDocument();
  });

  it("renders the guidance branch in one spanning cell when empty", () => {
    setup({ rows: [], emptyText: "Create projects with work items." });
    expect(screen.getAllByRole("row")).toHaveLength(2);
    expect(screen.getByText("Create projects with work items.")).toBeInTheDocument();
    expect(screen.getAllByRole("cell")).toHaveLength(1);
  });

  it("omits the export action when the caller supplies no handler", () => {
    setup({ onExport: undefined });
    expect(screen.queryByRole("button", { name: "Export as csv" })).not.toBeInTheDocument();
  });

  it("renders a custom first cell without adding it to the text content", () => {
    setup({
      columns: [
        {
          ...COLUMNS[0]!,
          render: () => <span data-testid="custom-cell" />,
        },
        COLUMNS[1]!,
      ],
    });
    expect(screen.getAllByTestId("custom-cell")).toHaveLength(2);
    expect(screen.getByRole("cell", { name: "1" })).toBeInTheDocument();
  });

  it("right-aligns a custom-rendered cell", () => {
    setup({
      columns: [
        COLUMNS[0]!,
        {
          ...COLUMNS[1]!,
          render: () => <span data-testid="custom-count" />,
        },
      ],
    });
    expect(screen.getAllByTestId("custom-count")[0]?.parentElement).toHaveClass("justify-end");
  });
});

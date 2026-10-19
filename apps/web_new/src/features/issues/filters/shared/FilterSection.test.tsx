// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { CircleDot } from "lucide-react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { FILTER_SECTION_CAP, FilterSection, type FilterSectionOption } from "./FilterSection.js";

const ROWS: FilterSectionOption[] = [
  { id: "a", label: "Alpha" },
  { id: "b", label: "Bravo" },
  { id: "c", label: "Charlie" },
  { id: "d", label: "Delta" },
  { id: "e", label: "Echo" },
  { id: "f", label: "Foxtrot" },
  { id: "g", label: "Golf" },
];

function renderSection(overrides: Partial<React.ComponentProps<typeof FilterSection>> = {}) {
  const onToggle = vi.fn();
  const props: React.ComponentProps<typeof FilterSection> = {
    title: "Cycle",
    kind: "multi",
    applied: [],
    options: ROWS,
    onToggle,
    testId: "section",
    ...overrides,
  };
  const view = render(<FilterSection {...props} />);
  return { ...view, onToggle, props };
}

describe("FilterSection", () => {
  it("renders the title and shows the applied count", () => {
    renderSection({ applied: ["a", "c"] });
    expect(screen.getByRole("heading", { name: "Cycle (2)" })).toBeInTheDocument();
  });

  it("collapses and reopens through the chevron", async () => {
    const user = userEvent.setup();
    renderSection();
    expect(screen.getByRole("button", { name: "Collapse Cycle section" })).toHaveAttribute("aria-expanded", "true");
    await user.click(screen.getByRole("button", { name: "Collapse Cycle section" }));
    expect(screen.getByRole("button", { name: "Expand Cycle section" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("checkbox", { name: "Alpha" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Expand Cycle section" }));
    expect(screen.getByRole("checkbox", { name: "Alpha" })).toBeInTheDocument();
  });

  it("honours controlled collapse state", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    const { rerender } = renderSection({ open: true, onOpenChange });
    await user.click(screen.getByRole("button", { name: "Collapse Cycle section" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    // Controlled: the host's value, not internal state, decides.
    expect(screen.getByRole("checkbox", { name: "Alpha" })).toBeInTheDocument();
    rerender(<FilterSection title="Cycle" kind="multi" applied={[]} options={ROWS} onToggle={() => {}} open={false} />);
    expect(screen.queryByRole("checkbox", { name: "Alpha" })).not.toBeInTheDocument();
  });

  it("marks rows as checkboxes in a multi section", async () => {
    const user = userEvent.setup();
    const { onToggle } = renderSection({ applied: ["b"] });
    expect(screen.getByRole("checkbox", { name: "Bravo" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("checkbox", { name: "Alpha" })).toHaveAttribute("aria-checked", "false");
    await user.click(screen.getByRole("checkbox", { name: "Alpha" }));
    expect(onToggle).toHaveBeenCalledWith("a");
  });

  it("marks rows as radios in a single section", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(
      <FilterSection
        title="Order by"
        kind="single"
        applied={["b"]}
        options={ROWS}
        onToggle={onToggle}
        groupLabel="Order by options"
      />
    );
    expect(screen.getByRole("radiogroup", { name: "Order by options" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Bravo" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: "Alpha" })).toHaveAttribute("aria-checked", "false");
    await user.click(screen.getByRole("radio", { name: "Alpha" }));
    expect(onToggle).toHaveBeenCalledWith("a");
  });

  it("offers a pressed-button read when the host keeps it", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(
      <FilterSection
        title="Group by"
        kind="single"
        selectionRole="pressed"
        applied={["b"]}
        options={ROWS}
        onToggle={onToggle}
      />
    );
    expect(screen.queryByRole("radio")).not.toBeInTheDocument();
    expect(screen.queryByRole("radiogroup")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Bravo" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Alpha" })).toHaveAttribute("aria-pressed", "false");
    await user.click(screen.getByRole("button", { name: "Alpha" }));
    expect(onToggle).toHaveBeenCalledWith("a");
  });

  it("filters by the shared search box, ignoring case and matching extra text", async () => {
    const user = userEvent.setup();
    renderSection({ options: [...ROWS, { id: "x", label: "Sprint", searchText: "APO sprint code" }] });
    const search = screen.getByRole("searchbox", { name: "Search Cycle" });
    await user.type(search, "apo");
    expect(screen.getByRole("checkbox", { name: "Sprint" })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "Alpha" })).not.toBeInTheDocument();
    await user.clear(search);
    await user.type(search, "a");
    expect(screen.queryByRole("checkbox", { name: "Golf" })).not.toBeInTheDocument();
  });

  it("shows the empty placeholder when the search matches nothing", async () => {
    const user = userEvent.setup();
    renderSection();
    await user.type(screen.getByRole("searchbox", { name: "Search Cycle" }), "zzz");
    expect(screen.getByText("No matches found")).toBeInTheDocument();
    expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
  });

  it("caps the list at five rows and expands and collapses it again", async () => {
    const user = userEvent.setup();
    renderSection();
    expect(FILTER_SECTION_CAP).toBe(5);
    expect(screen.getAllByRole("checkbox")).toHaveLength(5);
    expect(screen.getByRole("checkbox", { name: "Echo" })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "Foxtrot" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "View all" }));
    expect(screen.getAllByRole("checkbox")).toHaveLength(7);
    expect(screen.getByRole("checkbox", { name: "Golf" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "View less" }));
    expect(screen.getAllByRole("checkbox")).toHaveLength(5);
  });

  it("offers no view-all control when every row fits", () => {
    renderSection({ options: ROWS.slice(0, 5) });
    expect(screen.queryByRole("button", { name: "View all" })).not.toBeInTheDocument();
  });

  it("expands the cap to the filtered set and resets it when the search changes", async () => {
    const user = userEvent.setup();
    renderSection({ options: ROWS.map((row) => ({ ...row, searchText: `${row.label} team` })) });
    await user.click(screen.getByRole("button", { name: "View all" }));
    expect(screen.getAllByRole("checkbox")).toHaveLength(7);
    // A new search collapses back to the cap.
    await user.type(screen.getByRole("searchbox", { name: "Search Cycle" }), "team");
    expect(screen.getAllByRole("checkbox")).toHaveLength(5);
  });

  it("honours a host cap", () => {
    renderSection({ cap: 2 });
    expect(screen.getAllByRole("checkbox")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "View all" })).toBeInTheDocument();
  });

  it("renders an option icon and marks the pulsing one", () => {
    renderSection({
      options: [
        { id: "run", label: "Sprint 4", icon: <CircleDot />, pulse: true },
        { id: "calm", label: "Sprint 5", icon: <CircleDot /> },
        { id: "none-icon", label: "Sprint 6" },
      ],
    });
    const icons = screen.getAllByTestId("filter-option-icon");
    expect(icons).toHaveLength(2);
    expect(icons[0]).toHaveAttribute("data-pulse", "true");
    expect(icons[0]?.className).toContain("animate-pulse");
    expect(icons[1]).toHaveAttribute("data-pulse", "false");
  });

  it("renders a disabled row and ignores its click", async () => {
    const user = userEvent.setup();
    const { onToggle } = renderSection({ options: [{ id: "gone", label: "Departed", disabled: true }] });
    const row = screen.getByRole("checkbox", { name: "Departed" });
    expect(row).toBeDisabled();
    await user.click(row);
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("shows the loading note in place of the rows", () => {
    renderSection({ loading: true, loadingText: "Loading cycles" });
    expect(screen.getByText("Loading cycles")).toBeInTheDocument();
    expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
    expect(screen.queryByText("No matches found")).not.toBeInTheDocument();
  });

  it("lets a host body keep the title and collapse without a search box", async () => {
    const user = userEvent.setup();
    render(
      <FilterSection title="Options" kind="multi" applied={[]} options={[]} onToggle={() => {}} testId="body">
        <button type="button">Custom control</button>
      </FilterSection>
    );
    expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Collapse Options section" }));
    expect(screen.queryByRole("button", { name: "Custom control" })).not.toBeInTheDocument();
  });

  it("reads a count override and a checked override separately from applied rows", () => {
    renderSection({ applied: [], checked: ["c"], count: 0 });
    expect(screen.getByRole("heading", { name: "Cycle" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Charlie" })).toHaveAttribute("aria-checked", "true");
  });

  it("exposes the section title for host styling hooks", () => {
    renderSection({ className: "py-4" });
    const section = screen.getByTestId("section");
    expect(section).toHaveAttribute("data-section-title", "Cycle");
    expect(section.className).toContain("py-4");
  });
});

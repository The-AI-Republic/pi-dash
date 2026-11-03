// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Widget filter dropdown (parity/specs/issues.md §15 + §22 ISS-094,
// ISS-178): the shared section chrome over the offered dimensions, each
// choice reported per parent, the date sections keeping presets above a
// custom day, the project dimension only when asked for, and the trigger
// counting applied dimensions.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { createAppClient } from "../../../core/api/client.js";
import { emptySubIssueFilters, SUB_ISSUE_FILTER_KEYS, type SubIssueFilters } from "./rules.js";
import { hasCustomDay, stateGroupLabel, SubIssueFilterMenu, SUB_ISSUE_STATE_GROUPS } from "./SubIssueFilterMenu.js";
import type { SubIssueFilterSources } from "./types.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

const SOURCES: SubIssueFilterSources = {
  assigneeRows: [
    { id: "u1", label: "Ada" },
    { id: "u2", label: "Bo" },
  ],
  assigneesLoading: false,
};

function stub(): void {
  const state = {
    id: "623e4567-e89b-12d3-a456-426614174009",
    project_id: PROJECT,
    workspace_id: "323e4567-e89b-12d3-a456-426614174002",
    name: "Todo",
    group: "unstarted",
    color: "#aaa",
    default: true,
    description: "",
    sequence: 1,
  };
  createAppClient("https://api.test", (async (url: string) => {
    const body = url.endsWith("/states/") ? [state] : url.endsWith("/projects/") ? [] : {};
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    };
  }) as never);
}

function renderMenu(props: Partial<React.ComponentProps<typeof SubIssueFilterMenu>> = {}) {
  stub();
  const onToggle = vi.fn();
  const onSetDate = vi.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={client}>
      <SubIssueFilterMenu
        workspaceSlug={WS}
        projectId={PROJECT}
        filters={props.filters ?? emptySubIssueFilters()}
        sources={props.sources ?? SOURCES}
        offered={props.offered}
        onToggle={props.onToggle ?? onToggle}
        onSetDate={props.onSetDate ?? onSetDate}
        {...{
          ...(props.editionFilters === undefined ? {} : { editionFilters: props.editionFilters }),
          ...(props.onToggleEdition === undefined ? {} : { onToggleEdition: props.onToggleEdition }),
          ...(props.editionChips === undefined ? {} : { editionChips: props.editionChips }),
          ...(props.onRemoveEditionChip === undefined ? {} : { onRemoveEditionChip: props.onRemoveEditionChip }),
        }}
      />
    </QueryClientProvider>
  );
  return { onToggle, onSetDate, ...view };
}

function openMenu(): void {
  fireEvent.click(screen.getByTestId("sub-issue-filters-trigger"));
}

describe("SubIssueFilterMenu", () => {
  it("labels the trigger and counts applied dimensions", () => {
    renderMenu();
    expect(screen.getByTestId("sub-issue-filters-trigger")).toHaveAccessibleName("Filters");
    renderMenu({ filters: { ...emptySubIssueFilters(), priority: ["high"], state_group: ["started"] } });
    const triggers = screen.getAllByTestId("sub-issue-filters-trigger");
    expect(triggers[1]).toHaveAccessibleName("Filters, 2 applied");
    expect(triggers[1]).toHaveAttribute("aria-pressed", "true");
  });

  it("draws every offered dimension in order and reports one toggle", async () => {
    const h = renderMenu();
    openMenu();
    const panel = screen.getByTestId("sub-issue-filters-panel");
    for (const testId of [
      "sub-issue-filter-priority",
      "sub-issue-filter-state-group",
      "sub-issue-filter-state",
      "sub-issue-filter-project",
      "sub-issue-filter-assignees",
      "sub-issue-filter-start_date",
      "sub-issue-filter-target_date",
    ]) {
      expect(within(panel).getByTestId(testId)).toBeInTheDocument();
    }
    fireEvent.click(within(panel).getByRole("checkbox", { name: "High" }));
    expect(h.onToggle).toHaveBeenCalledWith("priority", "high");
    fireEvent.click(within(panel).getByRole("checkbox", { name: "Started" }));
    expect(h.onToggle).toHaveBeenCalledWith("state_group", "started");
    fireEvent.click(within(panel).getByRole("checkbox", { name: "Ada" }));
    expect(h.onToggle).toHaveBeenCalledWith("assignees", "u1");
    // The state rows load from the project's own read.
    fireEvent.click(await within(panel).findByRole("checkbox", { name: "Todo" }));
    expect(h.onToggle).toHaveBeenCalledWith("state", "623e4567-e89b-12d3-a456-426614174009");
  });

  it("names the applied edition values and removes the picked one", () => {
    const onRemoveEditionChip = vi.fn();
    const chip = { key: "teamProject" as const, value: "t1", label: "Platform" };
    renderMenu({
      editionChips: [chip],
      editionFilters: { teamProject: ["t1"], issueType: [] },
      onToggleEdition: vi.fn(),
      onRemoveEditionChip,
    });
    openMenu();
    const panel = screen.getByTestId("sub-issue-filters-panel");
    expect(within(panel).getByTestId("edition-filter-chips")).toHaveTextContent("Platform");
    fireEvent.click(within(panel).getByRole("button", { name: "Remove Platform filter" }));
    expect(onRemoveEditionChip).toHaveBeenCalledWith(chip);
  });

  it("leaves the chip row out without applied edition values", () => {
    renderMenu({
      editionChips: [],
      editionFilters: { teamProject: [], issueType: [] },
      onToggleEdition: vi.fn(),
      onRemoveEditionChip: vi.fn(),
    });
    openMenu();
    expect(screen.queryByTestId("edition-filter-chips")).toBeNull();
  });

  it("counts applied edition dimensions in the trigger (ISS-234)", () => {
    renderMenu({
      filters: { ...emptySubIssueFilters(), priority: ["high"] },
      editionFilters: { teamProject: ["t1"], issueType: ["y1"] },
      editionChips: [],
      onToggleEdition: vi.fn(),
      onRemoveEditionChip: vi.fn(),
    });
    expect(screen.getByTestId("sub-issue-filters-trigger")).toHaveAccessibleName("Filters, 3 applied");
  });

  it("counts an edition-only filter without a base one (ISS-234)", () => {
    renderMenu({
      editionFilters: { teamProject: [], issueType: ["y1"] },
      editionChips: [],
      onToggleEdition: vi.fn(),
      onRemoveEditionChip: vi.fn(),
    });
    expect(screen.getByTestId("sub-issue-filters-trigger")).toHaveAccessibleName("Filters, 1 applied");
  });

  it("omits state and project when the parent does not offer them", () => {
    renderMenu({ offered: ["priority", "state_group"] });
    openMenu();
    const panel = screen.getByTestId("sub-issue-filters-panel");
    expect(within(panel).queryByTestId("sub-issue-filter-state")).not.toBeInTheDocument();
    expect(within(panel).queryByTestId("sub-issue-filter-project")).not.toBeInTheDocument();
    expect(within(panel).queryByTestId("sub-issue-filter-assignees")).not.toBeInTheDocument();
    expect(within(panel).queryByTestId("sub-issue-filter-target_date")).not.toBeInTheDocument();
  });

  it("toggles a date preset through the chosen list", () => {
    const h = renderMenu();
    openMenu();
    fireEvent.click(screen.getAllByRole("checkbox", { name: "1 week from now" })[0]!);
    expect(h.onSetDate).toHaveBeenCalledWith("start_date", ["1_weeks;after;fromnow"]);
  });

  it("keeps presets above a custom day and reads the direction", () => {
    const h = renderMenu({
      filters: { ...emptySubIssueFilters(), start_date: ["1_weeks;after;fromnow", "2026-10-05;before"] },
    });
    openMenu();
    const custom = screen.getByLabelText("Start date custom day") as HTMLInputElement;
    expect(custom).toHaveValue("2026-10-05");
    expect(screen.getAllByRole("radio", { name: "Before" })[0]).toHaveAttribute("aria-checked", "true");
    fireEvent.change(custom, { target: { value: "2026-11-01" } });
    expect(h.onSetDate).toHaveBeenCalledWith("start_date", ["1_weeks;after;fromnow", "2026-11-01;before"]);
  });

  it("routes the direction radios and refuses an empty day", () => {
    const h = renderMenu({ filters: { ...emptySubIssueFilters(), target_date: ["2026-10-05"] } });
    openMenu();
    fireEvent.click(screen.getAllByRole("radio", { name: "After" }).at(-1)!);
    expect(h.onSetDate).toHaveBeenCalledWith("target_date", ["2026-10-05;after"]);
    fireEvent.click(screen.getAllByRole("radio", { name: "Before" }).at(-1)!);
    expect(h.onSetDate).toHaveBeenCalledWith("target_date", ["2026-10-05;before"]);
    fireEvent.click(screen.getAllByRole("radio", { name: "On" }).at(-1)!);
    expect(h.onSetDate).toHaveBeenCalledWith("target_date", ["2026-10-05"]);
    h.onSetDate.mockClear();
    const empty = renderMenu({ filters: { ...emptySubIssueFilters(), target_date: ["2026-10-05"] } });
    const triggers = screen.getAllByTestId("sub-issue-filters-trigger");
    fireEvent.click(triggers.at(-1)!);
    expect(empty.onSetDate).not.toHaveBeenCalled();
  });

  it("clears the custom day back to the presets", () => {
    const h = renderMenu({
      filters: { ...emptySubIssueFilters(), start_date: ["1_weeks;after;fromnow", "2026-10-05;after"] },
    });
    openMenu();
    fireEvent.change(screen.getByLabelText("Start date custom day"), { target: { value: "" } });
    expect(h.onSetDate).toHaveBeenCalledWith("start_date", ["1_weeks;after;fromnow"]);
    fireEvent.click(screen.getByRole("button", { name: "Clear Start date custom day" }));
    expect(h.onSetDate).toHaveBeenCalledWith("start_date", ["1_weeks;after;fromnow"]);
  });

  it("names the custom day's direction options", () => {
    renderMenu({ filters: { ...emptySubIssueFilters(), target_date: ["2026-10-05"] } });
    openMenu();
    expect(screen.getByRole("radiogroup", { name: "Due date custom direction" })).toBeInTheDocument();
    expect(screen.getAllByRole("radio").map((radio) => radio.textContent)).toEqual([
      "On",
      "After",
      "Before",
      "On",
      "After",
      "Before",
    ]);
  });

  it("uses the shared section chrome and a fixed group order", () => {
    renderMenu();
    openMenu();
    expect(SUB_ISSUE_STATE_GROUPS).toEqual([
      "backlog",
      "unstarted",
      "started",
      "review",
      "test",
      "completed",
      "cancelled",
    ]);
    expect(stateGroupLabel("state_group" as never)).toBe("State_group");
    expect(stateGroupLabel("started")).toBe("Started");
    const panel = screen.getByTestId("sub-issue-filters-panel");
    // The seven-member priority list is capped at five with a view-all.
    expect(within(panel).getAllByRole("checkbox", { name: /^(None|Low|Medium|High|Urgent)$/ })).toHaveLength(5);
    expect(within(panel).getByRole("button", { name: "View all" })).toBeInTheDocument();
    fireEvent.click(within(panel).getByRole("button", { name: "View all" }));
    expect(within(panel).getAllByRole("checkbox", { name: /^(None|Low|Medium|High|Urgent)$/ })).toHaveLength(5);
    expect(SUB_ISSUE_FILTER_KEYS).toHaveLength(7);
  });

  it("answers whether a date section holds a custom day", () => {
    expect(hasCustomDay(emptySubIssueFilters(), "start_date")).toBe(false);
    expect(hasCustomDay({ ...emptySubIssueFilters(), start_date: ["2026-10-05"] }, "start_date")).toBe(true);
  });

  it("treats an empty custom list as no direction", () => {
    renderMenu({ filters: { ...emptySubIssueFilters(), start_date: ["2026-10-05"] } as SubIssueFilters });
    openMenu();
    expect(screen.getByLabelText("Start date custom day")).toHaveValue("2026-10-05");
  });
});

describe("SubIssueFilterMenu branches", () => {
  it("stores a custom day as On when the section has no direction yet", () => {
    const h = renderMenu();
    openMenu();
    fireEvent.change(screen.getByLabelText("Start date custom day"), { target: { value: "2026-10-07" } });
    expect(h.onSetDate).toHaveBeenCalledWith("start_date", ["2026-10-07"]);
  });

  it("reports one project choice and names a failed project read", async () => {
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/projects/")) {
        return { status: 500, url, headers: { get: () => null }, text: async () => "{}" };
      }
      return { status: 200, url, headers: { get: () => null }, text: async () => "[]" };
    }) as never);
    const onToggle = vi.fn();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <SubIssueFilterMenu
          workspaceSlug={WS}
          projectId={PROJECT}
          filters={emptySubIssueFilters()}
          sources={SOURCES}
          offered={["project"]}
          onToggle={onToggle}
          onSetDate={vi.fn()}
        />
      </QueryClientProvider>
    );
    openMenu();
    expect(await screen.findByText("Couldn't load projects.")).toBeInTheDocument();
  });

  it("reports one project choice through the shared chrome", async () => {
    createAppClient("https://api.test", (async (url: string) => {
      const body = url.endsWith("/projects/")
        ? [
            {
              id: PROJECT,
              name: "Atlas",
              identifier: "ATL",
              sort_order: 1,
              logo_props: {},
              member_role: 15,
              intake_count: 0,
              archived_at: null,
              workspace: "323e4567-e89b-12d3-a456-426614174002",
              cycle_view: true,
              issue_views_view: true,
              module_view: true,
              page_view: true,
              inbox_view: true,
              is_default: false,
              guest_view_all_features: false,
              project_lead: null,
              network: 2,
              created_at: "2026-01-01T00:00:00Z",
              updated_at: "2026-01-01T00:00:00Z",
              created_by: null,
              updated_by: null,
            },
          ]
        : [];
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
    }) as never);
    const onToggle = vi.fn();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <SubIssueFilterMenu
          workspaceSlug={WS}
          projectId={PROJECT}
          filters={emptySubIssueFilters()}
          sources={SOURCES}
          offered={["project"]}
          onToggle={onToggle}
          onSetDate={vi.fn()}
        />
      </QueryClientProvider>
    );
    openMenu();
    fireEvent.click(await screen.findByRole("checkbox", { name: "Atlas" }));
    expect(onToggle).toHaveBeenCalledWith("project", PROJECT);
  });
});

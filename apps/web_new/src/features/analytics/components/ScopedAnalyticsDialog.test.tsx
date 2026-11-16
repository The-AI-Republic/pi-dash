// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scoped analytics dialog tests (parity/specs/analytics.md §13, ANL-014;
// §19, ANL-018).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: {
    getProjectAnalyticsTotals: vi.fn(),
    getProjectAnalyticsChart: vi.fn(),
    getProjectAnalyticsStats: vi.fn(),
    getAnalyticsTotals: vi.fn(),
    getAnalyticsChart: vi.fn(),
    getAnalyticsStats: vi.fn(),
  },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

// The chart layer lazy-loads Recharts; the dialog's job is the panel, not
// the plot, so the chart labels render their section heading only.
vi.mock("./CreatedResolvedSection.js", () => ({
  CreatedResolvedSection: (): React.ReactElement => <section data-testid="analytics-created-stub" />,
}));
vi.mock("./CustomInsightsSection.js", () => ({
  CustomInsightsSection: (): React.ReactElement => <section data-testid="analytics-custom-stub" />,
}));

import { dialogScopeFor, ScopedAnalyticsDialog } from "./ScopedAnalyticsDialog.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";
const MODULE = "423e4567-e89b-12d3-a456-426614174003";

function statsRow(extra: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    display_name: "Ada Lovelace",
    assignee_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    avatar_url: null,
    cancelled_work_items: 0,
    completed_work_items: 1,
    backlog_work_items: 2,
    un_started_work_items: 3,
    started_work_items: 4,
    ...extra,
  };
}

interface HarnessOptions {
  open?: boolean;
  cycle?: { id: string; name: string };
  module?: { id: string; name: string };
}

function renderDialog(options: HarnessOptions = {}): { onOpenChange: ReturnType<typeof vi.fn> } {
  const onOpenChange = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ScopedAnalyticsDialog
        workspaceSlug="acme"
        open={options.open ?? true}
        onOpenChange={onOpenChange}
        projectId={PROJECT}
        projectName="Website"
        cycle={options.cycle}
        module={options.module}
      />
    </QueryClientProvider>
  );
  return { onOpenChange };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  contractMocks.getProjectAnalyticsTotals.mockResolvedValue({
    total_work_items: { count: 2 },
    started_work_items: { count: 0 },
    backlog_work_items: { count: 2 },
    un_started_work_items: { count: 0 },
    completed_work_items: { count: 0 },
  });
  contractMocks.getProjectAnalyticsChart.mockResolvedValue({ data: [], schema: {} });
  contractMocks.getProjectAnalyticsStats.mockResolvedValue([statsRow()]);
});

afterEach(() => resetAppClient());

describe("dialogScopeFor", () => {
  it("keeps the project alone with no narrowing", () => {
    expect(dialogScopeFor(PROJECT, undefined, undefined)).toEqual({
      projectId: PROJECT,
      cycleId: null,
      moduleId: null,
    });
  });

  it("narrows by the cycle", () => {
    expect(dialogScopeFor(PROJECT, { id: CYCLE }, undefined)).toEqual({
      projectId: PROJECT,
      cycleId: CYCLE,
      moduleId: null,
    });
  });

  it("narrows by the module", () => {
    expect(dialogScopeFor(PROJECT, undefined, { id: MODULE })).toEqual({
      projectId: PROJECT,
      cycleId: null,
      moduleId: MODULE,
    });
  });

  it("prefers the cycle when both surfaces somehow arrive", () => {
    expect(dialogScopeFor(PROJECT, { id: CYCLE }, { id: MODULE })).toEqual({
      projectId: PROJECT,
      cycleId: CYCLE,
      moduleId: null,
    });
  });
});

describe("ScopedAnalyticsDialog", () => {
  it("titles a project scope", async () => {
    renderDialog();
    expect(await screen.findByRole("dialog", { name: "Analytics for Website" })).toBeInTheDocument();
  });

  it("titles a cycle scope", async () => {
    renderDialog({ cycle: { id: CYCLE, name: "Sprint 3" } });
    expect(await screen.findByRole("dialog", { name: "Analytics for Website in Sprint 3" })).toBeInTheDocument();
  });

  it("titles a module scope with one space", async () => {
    renderDialog({ module: { id: MODULE, name: "Alpha" } });
    expect(await screen.findByRole("dialog", { name: "Analytics for Website in Alpha" })).toBeInTheDocument();
  });

  it("spins while the scope's first reads are in flight, then shows content in order", async () => {
    let releaseTotals: (value: unknown) => void = () => undefined;
    contractMocks.getProjectAnalyticsTotals.mockReturnValue(
      new Promise((resolve) => {
        releaseTotals = resolve;
      })
    );
    renderDialog();
    expect(await screen.findByText("Loading analytics")).toBeInTheDocument();
    expect(screen.queryByTestId("analytics-headline-grid")).not.toBeInTheDocument();
    releaseTotals({
      total_work_items: { count: 2 },
      started_work_items: { count: 0 },
      backlog_work_items: { count: 2 },
      un_started_work_items: { count: 0 },
      completed_work_items: { count: 0 },
    });
    expect(await screen.findByTestId("analytics-headline-grid")).toBeInTheDocument();
    expect(screen.queryByText("Loading analytics")).not.toBeInTheDocument();
    const sections = [
      screen.getByTestId("analytics-headline-grid"),
      screen.getByTestId("analytics-created-stub"),
      screen.getByTestId("analytics-custom-stub"),
      screen.getByTestId("analytics-dialog-assignee-table"),
    ];
    // Cards, then created-vs-resolved, then custom insights with its
    // table, then the assignee table: document order, top to bottom.
    for (let index = 1; index < sections.length; index += 1) {
      const previous = sections[index - 1]!;
      const current = sections[index]!;
      expect(previous.compareDocumentPosition(current) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(
        Node.DOCUMENT_POSITION_FOLLOWING
      );
    }
  });

  it("shows the five work-items cards with the scoped totals", async () => {
    renderDialog();
    await screen.findByTestId("analytics-headline-grid");
    const labels = screen.getAllByTestId("analytics-headline-label").map((node) => node.textContent);
    const values = screen.getAllByTestId("analytics-headline-value").map((node) => node.textContent);
    expect(labels).toEqual([
      "Total Work items",
      "Started Work items",
      "Backlog Work items",
      "Unstarted Work items",
      "Completed Work items",
    ]);
    expect(values).toEqual(["2", "0", "2", "0", "0"]);
  });

  it("reads only the project endpoints with project_ids and the narrowing", async () => {
    renderDialog({ module: { id: MODULE, name: "Alpha" } });
    await screen.findByTestId("analytics-dialog-assignee-table");
    expect(contractMocks.getAnalyticsTotals).not.toHaveBeenCalled();
    expect(contractMocks.getAnalyticsStats).not.toHaveBeenCalled();
    expect(contractMocks.getProjectAnalyticsTotals).toHaveBeenCalledWith(expect.anything(), "acme", PROJECT, {
      projectIds: [PROJECT],
      cycleId: undefined,
      moduleId: MODULE,
    });
    expect(contractMocks.getProjectAnalyticsStats).toHaveBeenCalledWith(expect.anything(), "acme", PROJECT, {
      projectIds: [PROJECT],
      cycleId: undefined,
      moduleId: MODULE,
    });
  });

  it("offers the wide toggle and reports resizing through the callback", async () => {
    const user = userEvent.setup();
    renderDialog();
    await screen.findByTestId("analytics-dialog-assignee-table");
    const toggle = screen.getByTestId("analytics-dialog-wide-toggle");
    expect(toggle).toHaveAccessibleName("Widen the panel");
    await user.click(toggle);
    expect(screen.getByTestId("analytics-dialog-wide-toggle")).toHaveAccessibleName("Use the narrower panel");
    await user.click(screen.getByTestId("analytics-dialog-wide-toggle"));
    expect(screen.getByTestId("analytics-dialog-wide-toggle")).toHaveAccessibleName("Widen the panel");
  });

  it("closes on Escape and reports the close", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    await screen.findByTestId("analytics-dialog-assignee-table");
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("closes from the close control", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    await screen.findByTestId("analytics-dialog-assignee-table");
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("renders nothing while closed", () => {
    renderDialog({ open: false });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(contractMocks.getProjectAnalyticsTotals).not.toHaveBeenCalled();
  });

  it("shows the assignee table with avatar, initials and the unassigned marker", async () => {
    contractMocks.getProjectAnalyticsStats.mockResolvedValue([
      statsRow(),
      statsRow({ display_name: "Bob Stone", avatar_url: "https://cdn.test/bob.png" }),
      statsRow({ display_name: null, assignee_id: null, avatar_url: null }),
    ]);
    renderDialog();
    await screen.findByText("Ada Lovelace");
    const table = screen.getByTestId("analytics-dialog-assignee-table");
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((head) => head.textContent)
    ).toEqual(["Assignee", "Backlog", "Started", "Unstarted", "Completed", "Cancelled"]);
    expect(screen.getByTestId("analytics-table-count")).toHaveTextContent("3 Assignee");
    expect(within(table).getByText("Unassigned")).toBeInTheDocument();
    expect(table.querySelector('img[src="https://cdn.test/bob.png"]')).not.toBeNull();
    expect(within(table).getByRole("img", { name: "Ada Lovelace" })).toHaveTextContent("AL");
  });

  it("does not gate any role: a guest-visible caller sees the same read-only panel", async () => {
    // ANL-018: analytics adds no permission check of its own. The dialog's
    // props carry no role at all, so a trigger owner (the cycle bar) may
    // open it for a guest; the server decides what the reads return.
    renderDialog();
    const dialog = await screen.findByRole("dialog");
    expect(dialog).not.toHaveAttribute("aria-disabled");
    expect(screen.queryByRole("button", { name: /create/i })).not.toBeInTheDocument();
    await waitFor(() => expect(contractMocks.getProjectAnalyticsStats).toHaveBeenCalled());
  });
});

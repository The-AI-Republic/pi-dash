// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// MOD-034 surface: the progress section opens only for usable dates
// with its pinned notices otherwise; the burndown chart switches
// between work items and points (points only under a points system)
// and refetches the details on switch; each breakdown tab shows its
// own dimension and remembers itself per module; entries toggle the
// module's rich filters on the detail page and stay inert in the
// peek, for guests and on archived modules.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ArchivedModuleDetail, ModuleDetail, ModuleUserProperties } from "@pidash/api-client";
import * as React from "react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../../shared/shell/toaster.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("recharts", () => ({
  ResponsiveContainer: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  AreaChart: ({ children, data }: { children?: ReactNode; data?: Array<Record<string, unknown>> }) => (
    <div data-testid="mock-area-chart" data-rows={JSON.stringify(data)}>
      {children}
    </div>
  ),
  CartesianGrid: () => <div />,
  XAxis: () => <div />,
  YAxis: () => <div />,
  Area: () => <div />,
  Legend: () => <div />,
  Tooltip: () => <div />,
}));

import { createAppClient, getAppClient, resetAppClient } from "../../../../core/api/client.js";
import { jsonResponse } from "../../testing.js";
import { ModuleProgressSection, ModuleProgressSectionContent } from "./ModuleProgressSection.js";
import { MODULE_PROGRESS_TABS_STORAGE_KEY } from "./moduleProgressModel.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";
const ADA = "aaaaaaaa-0000-4000-8000-000000000001";
const BUG = "bbbbbbbb-0000-4000-8000-000000000002";
const DETAIL_PATH = `/api/workspaces/${SLUG}/projects/${PROJECT}/modules/${MODULE}/`;
const ARCHIVED_PATH = `/api/workspaces/${SLUG}/projects/${PROJECT}/archived-modules/${MODULE}/`;
const ESTIMATES_PATH = `/api/workspaces/${SLUG}/projects/${PROJECT}/estimates/`;
const ROLES_PATH = `/api/users/me/workspaces/${SLUG}/project-roles/`;

function detailFixture(overrides: Partial<ModuleDetail> = {}): ModuleDetail {
  return {
    id: MODULE,
    workspace_id: "123e4567-e89b-12d3-a456-426614174000",
    project_id: PROJECT,
    name: "Delivery",
    description: "",
    start_date: "2026-09-01",
    target_date: "2026-12-31",
    status: "in-progress",
    lead_id: null,
    member_ids: [],
    sort_order: 0,
    is_favorite: false,
    total_issues: 3,
    completed_issues: 1,
    cancelled_issues: 0,
    started_issues: 0,
    unstarted_issues: 1,
    backlog_issues: 1,
    total_estimate_points: 4,
    completed_estimate_points: 1,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    link_module: [],
    sub_issues: null,
    distribution: {
      assignees: [
        {
          assignee_id: ADA,
          avatar_url: null,
          first_name: "Ada",
          last_name: "Lovelace",
          display_name: "Ada Lovelace",
          total_issues: 1,
          completed_issues: 0,
          pending_issues: 1,
        },
        {
          assignee_id: null,
          avatar_url: null,
          first_name: null,
          last_name: null,
          display_name: null,
          total_issues: 2,
          completed_issues: 1,
          pending_issues: 1,
        },
      ],
      labels: [
        {
          label_id: BUG,
          label_name: "Bug",
          color: "#ff0000",
          total_issues: 1,
          completed_issues: 0,
          pending_issues: 1,
        },
      ],
      completion_chart: { "2026-10-01": 3, "2026-10-02": 2, "2026-10-03": 1, "2026-10-25": null },
    },
    estimate_distribution: {
      assignees: [
        {
          assignee_id: ADA,
          avatar_url: null,
          first_name: "Ada",
          last_name: "Lovelace",
          display_name: "Ada Lovelace",
          total_estimates: 2,
          completed_estimates: 0,
          pending_estimates: 2,
        },
      ],
      labels: [
        {
          label_id: BUG,
          label_name: "Bug",
          color: "#ff0000",
          total_estimates: 1,
          completed_estimates: 0,
          pending_estimates: 1,
        },
      ],
      completion_chart: { "2026-10-01": 4, "2026-10-02": 3 },
    },
    ...overrides,
  };
}

function archivedFixture(overrides: Partial<ArchivedModuleDetail> = {}): ArchivedModuleDetail {
  return {
    id: MODULE,
    workspace_id: "123e4567-e89b-12d3-a456-426614174000",
    project_id: PROJECT,
    name: "Archived delivery",
    description: "",
    description_text: null,
    description_html: null,
    start_date: "2026-09-01",
    target_date: "2026-12-31",
    status: "completed",
    lead_id: null,
    member_ids: [],
    sort_order: 0,
    total_issues: 2,
    completed_issues: 2,
    cancelled_issues: 0,
    started_issues: 0,
    unstarted_issues: 0,
    backlog_issues: 0,
    total_estimate_points: 0,
    completed_estimate_points: 0,
    backlog_estimate_points: 0,
    unstarted_estimate_points: 0,
    started_estimate_points: 0,
    cancelled_estimate_points: 0,
    is_favorite: false,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    archived_at: "2026-03-01T00:00:00Z",
    link_module: [],
    sub_issues: null,
    distribution: {
      assignees: [{ assignee_id: ADA, display_name: "Ada Lovelace", total_issues: 2, completed_issues: 2 }],
      labels: [],
      completion_chart: { "2026-09-30": 0 },
    },
    estimate_distribution: {},
    ...overrides,
  };
}

function prefsFixture(overrides: Partial<ModuleUserProperties> = {}): ModuleUserProperties {
  return {
    id: "523e4567-e89b-12d3-a456-426614174005",
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    project: PROJECT,
    module: MODULE,
    user: "623e4567-e89b-12d3-a456-426614174006",
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    created_by: null,
    updated_by: null,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    ...overrides,
  };
}

function estimate(type: string) {
  return {
    id: "423e4567-e89b-12d3-a456-426614174003",
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    project: PROJECT,
    name: "Estimate",
    description: "",
    type,
    last_used: true,
    points: [],
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };
}

interface Harness {
  detail: ModuleDetail | ArchivedModuleDetail;
  detailStatus: number;
  estimates: ReturnType<typeof estimate>[];
  roles: Record<string, number>;
  detailCalls: number;
  archivedCalls: number;
  richFilters: Record<string, unknown>;
  patchBodies: Record<string, unknown>[];
  prefsDelay: number;
}

let harness: Harness;
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

beforeEach(() => {
  localStorage.clear();
  harness = {
    detail: detailFixture(),
    detailStatus: 200,
    estimates: [],
    roles: {},
    detailCalls: 0,
    archivedCalls: 0,
    richFilters: {},
    patchBodies: [],
    prefsDelay: 0,
  };
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
    const method = init?.method ?? "GET";
    if (url.endsWith(DETAIL_PATH) && method === "GET") {
      harness.detailCalls += 1;
      return jsonResponse(
        url,
        harness.detailStatus,
        harness.detailStatus === 200 ? harness.detail : { detail: "boom" }
      );
    }
    if (url.endsWith(ARCHIVED_PATH) && method === "GET") {
      harness.archivedCalls += 1;
      return jsonResponse(url, 200, harness.detail);
    }
    if (url.endsWith(ESTIMATES_PATH)) {
      return jsonResponse(url, 200, harness.estimates);
    }
    if (url.endsWith(ROLES_PATH)) {
      return jsonResponse(url, 200, harness.roles);
    }
    if (url.endsWith("/user-properties/") && method === "PATCH") {
      const body = JSON.parse(init?.body ?? "{}") as Record<string, unknown>;
      harness.patchBodies.push(body);
      harness.richFilters = (body["rich_filters"] ?? {}) as Record<string, unknown>;
      return jsonResponse(url, 201, prefsFixture({ rich_filters: harness.richFilters }));
    }
    if (url.endsWith("/user-properties/")) {
      if (harness.prefsDelay > 0) await sleep(harness.prefsDelay);
      return jsonResponse(url, 200, prefsFixture({ rich_filters: harness.richFilters }));
    }
    throw new Error(`unexpected request: ${method} ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

function renderSection(overrides: Partial<React.ComponentProps<typeof ModuleProgressSection>> = {}) {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ModuleProgressSection workspaceSlug={SLUG} projectId={PROJECT} moduleId={MODULE} {...overrides} />
    </QueryClientProvider>
  );
}

function renderContent(overrides: Partial<React.ComponentProps<typeof ModuleProgressSectionContent>> = {}) {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ModuleProgressSectionContent
        workspaceSlug={SLUG}
        projectId={PROJECT}
        moduleId={MODULE}
        record={detailFixture()}
        readOnly={false}
        archived={false}
        editor
        showPoints={false}
        onRefetchDetails={() => undefined}
        {...overrides}
      />
    </QueryClientProvider>
  );
}

function chartRows(): unknown {
  const chart = screen.getByTestId("module-progress-chart");
  return JSON.parse((within(chart).getByTestId("mock-area-chart").dataset["rows"] ?? "null") as string);
}

const OTHER_MODULE = "423e4567-e89b-12d3-a456-426614174003";

function renderSwitchable(initial: Partial<React.ComponentProps<typeof ModuleProgressSectionContent>> = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const element = (overrides: Partial<React.ComponentProps<typeof ModuleProgressSectionContent>> = {}) => (
    <QueryClientProvider client={client}>
      <ModuleProgressSectionContent
        workspaceSlug={SLUG}
        projectId={PROJECT}
        moduleId={MODULE}
        record={detailFixture()}
        readOnly={false}
        archived={false}
        editor
        showPoints={false}
        onRefetchDetails={() => undefined}
        {...initial}
        {...overrides}
      />
    </QueryClientProvider>
  );
  const view = render(element());
  return {
    ...view,
    show: (overrides: Partial<React.ComponentProps<typeof ModuleProgressSectionContent>>) =>
      view.rerender(element(overrides)),
  };
}

describe("ModuleProgressSection", () => {
  it("stays empty until the detail lands", () => {
    harness.detailStatus = 500;
    renderSection({ canEdit: true });
    expect(screen.queryByTestId("module-progress-section")).toBeNull();
  });

  it("renders the section once the detail loads", async () => {
    renderSection({ canEdit: true });
    expect(await screen.findByTestId("module-progress-section")).toBeInTheDocument();
    expect(screen.getByTestId("module-progress-percent").textContent).toBe("33%");
  });

  it("passes an explicit client through to the reads", async () => {
    renderSection({ canEdit: true, client: getAppClient() });
    expect(await screen.findByTestId("module-progress-section")).toBeInTheDocument();
    expect(harness.detailCalls).toBe(1);
  });

  it("derives editability from the role map when no override is passed", async () => {
    harness.roles = { [PROJECT]: 20 };
    renderSection();
    await screen.findByTestId("module-progress-section");
    await userEvent.setup().click(screen.getByTestId("module-progress-tab-States"));
    await waitFor(() => expect(screen.getByTestId("module-progress-entry-backlog").tagName).toBe("BUTTON"));
  });

  it("reads guests as inert through the role map", async () => {
    renderSection();
    await screen.findByTestId("module-progress-section");
    await userEvent.setup().click(screen.getByTestId("module-progress-tab-States"));
    expect(screen.getByTestId("module-progress-entry-backlog").tagName).toBe("DIV");
  });

  it("reads the archived endpoint for archived mounts", async () => {
    harness.detail = archivedFixture();
    renderSection({ archived: true, canEdit: true });
    expect(await screen.findByTestId("module-progress-section")).toBeInTheDocument();
    expect(harness.archivedCalls).toBe(1);
    expect(harness.detailCalls).toBe(0);
  });

  it("refetches the detail when the measure switches", async () => {
    const user = userEvent.setup();
    harness.estimates = [estimate("points")];
    renderSection({ canEdit: true });
    await screen.findByTestId("module-progress-measure");
    expect(harness.detailCalls).toBe(1);
    await user.click(screen.getByTestId("module-progress-measure"));
    await user.click(await screen.findByRole("option", { name: "Points" }));
    await waitFor(() => expect(screen.getByTestId("module-progress-measure").textContent).toBe("Points"));
    await waitFor(() => expect(harness.detailCalls).toBe(2));
    await user.click(screen.getByTestId("module-progress-measure"));
    await user.click(await screen.findByRole("option", { name: "Work items" }));
    await waitFor(() => expect(screen.getByTestId("module-progress-measure").textContent).toBe("Work items"));
    await waitFor(() => expect(harness.detailCalls).toBe(3));
  });
});

describe("ModuleProgressSectionContent dates", () => {
  it("opens an active module with its chart and breakdown tabs", async () => {
    renderContent();
    const section = await screen.findByTestId("module-progress-section");
    expect(section.getAttribute("aria-label")).toBe("Progress");
    expect(screen.getByTestId("module-progress-toggle").getAttribute("aria-expanded")).toBe("true");
    expect(screen.queryByTestId("module-progress-notice")).toBeNull();
    expect(chartRows()).toEqual([
      { label: "Oct 1, 2026", remaining: 3 },
      { label: "Oct 2, 2026", remaining: 2 },
      { label: "Oct 3, 2026", remaining: 1 },
    ]);
    expect(screen.getByTestId("module-progress-tabs")).toBeInTheDocument();
  });

  it("explains a future start without a toggle, percent, chart or tabs", () => {
    renderContent({ record: detailFixture({ start_date: "2999-01-01", target_date: "2999-01-31" }) });
    expect(screen.getByTestId("module-progress-notice").textContent).toBe("This module isn't active yet.");
    expect(screen.queryByTestId("module-progress-toggle")).toBeNull();
    expect(screen.queryByTestId("module-progress-percent")).toBeNull();
    expect(screen.queryByTestId("module-progress-chart")).toBeNull();
    expect(screen.queryByTestId("module-progress-tabs")).toBeNull();
  });

  it("explains missing dates as invalid", () => {
    renderContent({ record: detailFixture({ start_date: null, target_date: null }) });
    expect(screen.getByTestId("module-progress-notice").textContent).toBe("Invalid date. Please enter valid date.");
    expect(screen.queryByTestId("module-progress-toggle")).toBeNull();
    expect(screen.queryByTestId("module-progress-tabs")).toBeNull();
  });

  it("explains a start after the target as not active yet", () => {
    renderContent({ record: detailFixture({ start_date: "2026-12-31", target_date: "2026-09-01" }) });
    expect(screen.getByTestId("module-progress-notice").textContent).toBe("This module isn't active yet.");
  });

  it("collapses and reopens through the toggle", async () => {
    const user = userEvent.setup();
    renderContent();
    const toggle = await screen.findByTestId("module-progress-toggle");
    await user.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByTestId("module-progress-tabs")).toBeNull();
    expect(screen.queryByTestId("module-progress-chart")).toBeNull();
    await user.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByTestId("module-progress-tabs")).toBeInTheDocument();
  });

  it("hides the badge when nothing is complete", async () => {
    renderContent({ record: detailFixture({ completed_issues: 0 }) });
    await screen.findByTestId("module-progress-section");
    expect(screen.queryByTestId("module-progress-percent")).toBeNull();
  });

  it("hides the chart when the side carries no chart object", async () => {
    renderContent({
      record: archivedFixture({ distribution: { assignees: [], labels: [], completion_chart: "nope" } }),
    });
    await screen.findByTestId("module-progress-section");
    expect(screen.queryByTestId("module-progress-chart")).toBeNull();
    expect(screen.getByTestId("module-progress-tabs")).toBeInTheDocument();
  });
});

describe("ModuleProgressSectionContent measure", () => {
  it("offers no measure without a points system", async () => {
    renderContent({ showPoints: false });
    await screen.findByTestId("module-progress-section");
    expect(screen.queryByTestId("module-progress-measure")).toBeNull();
  });

  it("switches the chart, header and breakdowns to points and refetches", async () => {
    const user = userEvent.setup();
    const onRefetchDetails = vi.fn();
    renderContent({ showPoints: true, onRefetchDetails });
    await screen.findByTestId("module-progress-measure");
    expect(screen.getByTestId("module-progress-measure").textContent).toBe("Work items");
    expect(screen.getByTestId("module-progress-percent").textContent).toBe("33%");
    await user.click(screen.getByTestId("module-progress-measure"));
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual(["Work items", "Points"]);
    await user.click(screen.getByRole("option", { name: "Points" }));
    await waitFor(() => expect(screen.getByTestId("module-progress-measure").textContent).toBe("Points"));
    expect(onRefetchDetails).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("module-progress-percent").textContent).toBe("25%");
    expect(chartRows()).toEqual([
      { label: "Oct 1, 2026", remaining: 4 },
      { label: "Oct 2, 2026", remaining: 3 },
    ]);
    expect(screen.getByTestId("module-progress-chart")).toBeInTheDocument();
  });

  it("zeroes the breakdowns when the estimate side is empty", async () => {
    const user = userEvent.setup();
    renderContent({ showPoints: true, record: detailFixture({ estimate_distribution: {} }) });
    await screen.findByTestId("module-progress-measure");
    await user.click(screen.getByTestId("module-progress-measure"));
    await user.click(await screen.findByRole("option", { name: "Points" }));
    await waitFor(() => expect(screen.getByTestId("module-progress-measure").textContent).toBe("Points"));
    expect(screen.queryByTestId("module-progress-chart")).toBeNull();
    expect(screen.getByTestId("module-progress-entries-empty").textContent).toBe("No assignee");
    await user.click(screen.getByTestId("module-progress-tab-States"));
    expect(screen.getByTestId("module-progress-entry-backlog").textContent).toContain("0%");
    expect(screen.getByTestId("module-progress-entry-backlog").textContent).toContain("of 4");
  });
});

describe("ModuleProgressSectionContent tabs", () => {
  it("defaults to Assignees with its own breakdown", async () => {
    renderContent();
    await screen.findByTestId("module-progress-section");
    expect(screen.getByTestId("module-progress-tab-Assignees").getAttribute("aria-selected")).toBe("true");
    await waitFor(() => expect(screen.getByTestId(`module-progress-entry-${ADA}`).tagName).toBe("BUTTON"));
    const ada = screen.getByTestId(`module-progress-entry-${ADA}`);
    expect(ada.tagName).toBe("BUTTON");
    expect(ada.textContent).toContain("Ada Lovelace");
    expect(ada.textContent).toContain("of 1");
    const unassigned = screen.getByTestId("module-progress-entry-unassigned-1");
    expect(unassigned.tagName).toBe("DIV");
    expect(unassigned.textContent).toContain("No assignee");
    expect(unassigned.textContent).toContain("50%");
    expect(unassigned.textContent).toContain("of 2");
  });

  it("shows state groups under States and labels with colors under Labels", async () => {
    const user = userEvent.setup();
    renderContent();
    await screen.findByTestId("module-progress-section");
    await user.click(screen.getByTestId("module-progress-tab-States"));
    expect(screen.getByTestId("module-progress-tab-States").getAttribute("aria-selected")).toBe("true");
    const backlog = screen.getByTestId("module-progress-entry-backlog");
    expect(backlog.textContent).toContain("Backlog");
    expect(backlog.textContent).toContain("33%");
    expect(backlog.textContent).toContain("of 3");
    expect(screen.getByTestId("module-progress-entry-cancelled").textContent).toContain("0%");
    await user.click(screen.getByTestId("module-progress-tab-Labels"));
    const bug = screen.getByTestId(`module-progress-entry-${BUG}`);
    expect(bug.textContent).toContain("Bug");
    expect(bug.textContent).toContain("0%");
    expect(bug.textContent).toContain("of 1");
    const dot = bug.querySelector("span > span") as HTMLElement | null;
    expect(dot?.style.backgroundColor).toBe("rgb(255, 0, 0)");
  });

  it("shows the empty states when a side carries nothing", async () => {
    const user = userEvent.setup();
    renderContent({
      record: detailFixture({
        distribution: { assignees: [], labels: [], completion_chart: { "2026-10-01": 0 } },
      }),
    });
    await screen.findByTestId("module-progress-section");
    expect(screen.getByTestId("module-progress-entries-empty").textContent).toBe("No assignee");
    await user.click(screen.getByTestId("module-progress-tab-Labels"));
    expect(screen.getByTestId("module-progress-entries-empty").textContent).toBe("No labels yet");
  });

  it("remembers the tab per module across mounts", async () => {
    const user = userEvent.setup();
    const first = renderContent();
    await screen.findByTestId("module-progress-section");
    await user.click(screen.getByTestId("module-progress-tab-Labels"));
    expect(JSON.parse(localStorage.getItem(MODULE_PROGRESS_TABS_STORAGE_KEY) ?? "{}")).toEqual({ [MODULE]: "Labels" });
    first.unmount();
    renderContent({ moduleId: "423e4567-e89b-12d3-a456-426614174003" });
    await screen.findByTestId("module-progress-section");
    expect(screen.getByTestId("module-progress-tab-Assignees").getAttribute("aria-selected")).toBe("true");
  });

  it("falls back to Assignees for a stored unknown tab", async () => {
    localStorage.setItem(MODULE_PROGRESS_TABS_STORAGE_KEY, JSON.stringify({ [MODULE]: "stat-labels" }));
    renderContent();
    await screen.findByTestId("module-progress-section");
    expect(screen.getByTestId("module-progress-tab-Assignees").getAttribute("aria-selected")).toBe("true");
  });
});

describe("ModuleProgressSectionContent filtering", () => {
  it("toggles a state group filter and persists it", async () => {
    const user = userEvent.setup();
    renderContent();
    await screen.findByTestId("module-progress-section");
    await user.click(screen.getByTestId("module-progress-tab-States"));
    const backlog = screen.getByTestId("module-progress-entry-backlog");
    expect(backlog.getAttribute("aria-pressed")).toBe("false");
    await user.click(backlog);
    await waitFor(() => expect(harness.patchBodies).toHaveLength(1));
    expect(JSON.stringify(harness.patchBodies[0])).toContain("backlog");
    await waitFor(() =>
      expect(screen.getByTestId("module-progress-entry-backlog").getAttribute("aria-pressed")).toBe("true")
    );
    await user.click(screen.getByTestId("module-progress-entry-backlog"));
    await waitFor(() => expect(harness.patchBodies).toHaveLength(2));
    expect(JSON.stringify(harness.patchBodies[1])).not.toContain("state_group");
    await waitFor(() =>
      expect(screen.getByTestId("module-progress-entry-backlog").getAttribute("aria-pressed")).toBe("false")
    );
  });

  it("toggles assignee and label filters by id", async () => {
    const user = userEvent.setup();
    renderContent();
    await screen.findByTestId("module-progress-section");
    await waitFor(() => expect(screen.getByTestId(`module-progress-entry-${ADA}`).tagName).toBe("BUTTON"));
    await user.click(screen.getByTestId(`module-progress-entry-${ADA}`));
    await waitFor(() => expect(harness.patchBodies).toHaveLength(1));
    expect(JSON.stringify(harness.patchBodies[0])).toContain(ADA);
    await waitFor(() =>
      expect(screen.getByTestId(`module-progress-entry-${ADA}`).getAttribute("aria-pressed")).toBe("true")
    );
    await user.click(screen.getByTestId("module-progress-tab-Labels"));
    await user.click(screen.getByTestId(`module-progress-entry-${BUG}`));
    await waitFor(() => expect(harness.patchBodies).toHaveLength(2));
    expect(JSON.stringify(harness.patchBodies[1])).toContain(BUG);
  });

  it("marks the stored selection without a click", async () => {
    harness.richFilters = { state_group__in: ["backlog"] };
    const user = userEvent.setup();
    renderContent();
    await screen.findByTestId("module-progress-section");
    await user.click(screen.getByTestId("module-progress-tab-States"));
    await waitFor(() =>
      expect(screen.getByTestId("module-progress-entry-backlog").getAttribute("aria-pressed")).toBe("true")
    );
  });

  it("keeps entries inert until the filter prefs load", async () => {
    harness.prefsDelay = 200;
    const user = userEvent.setup();
    renderContent();
    await screen.findByTestId("module-progress-section");
    await user.click(screen.getByTestId("module-progress-tab-States"));
    expect(screen.getByTestId("module-progress-entry-backlog").tagName).toBe("DIV");
    await waitFor(() => expect(screen.getByTestId("module-progress-entry-backlog").tagName).toBe("BUTTON"));
  });

  it("keeps the peek read-only: tabs switch but entries never write", async () => {
    const user = userEvent.setup();
    renderContent({ readOnly: true });
    await screen.findByTestId("module-progress-section");
    await user.click(screen.getByTestId("module-progress-tab-States"));
    expect(screen.getByTestId("module-progress-tab-States").getAttribute("aria-selected")).toBe("true");
    const backlog = screen.getByTestId("module-progress-entry-backlog");
    expect(backlog.tagName).toBe("DIV");
    await user.click(backlog);
    await sleep(20);
    expect(harness.patchBodies).toHaveLength(0);
    expect(backlog.getAttribute("aria-pressed")).toBeNull();
  });

  it("keeps guests read-only", async () => {
    const user = userEvent.setup();
    renderContent({ editor: false });
    await screen.findByTestId("module-progress-section");
    await user.click(screen.getByTestId("module-progress-tab-States"));
    expect(screen.getByTestId("module-progress-entry-backlog").tagName).toBe("DIV");
    expect(screen.queryByTestId("module-progress-entries-empty")).toBeNull();
  });

  it("keeps archived records read-only while showing their data", async () => {
    const user = userEvent.setup();
    renderContent({ record: archivedFixture(), archived: true });
    await screen.findByTestId("module-progress-section");
    expect(screen.getByTestId("module-progress-percent").textContent).toBe("100%");
    expect(chartRows()).toEqual([{ label: "Sep 30, 2026", remaining: 0 }]);
    const ada = screen.getByTestId(`module-progress-entry-${ADA}`);
    expect(ada.tagName).toBe("DIV");
    expect(ada.textContent).toContain("Ada Lovelace");
    await user.click(ada);
    await sleep(20);
    expect(harness.patchBodies).toHaveLength(0);
  });

  it("treats an archived record as read-only even without the flag", async () => {
    const user = userEvent.setup();
    renderContent({ record: archivedFixture() });
    await screen.findByTestId("module-progress-section");
    await user.click(screen.getByTestId("module-progress-tab-States"));
    expect(screen.getByTestId("module-progress-entry-backlog").tagName).toBe("DIV");
  });
});

describe("ModuleProgressSectionContent module switches and date edits", () => {
  it("re-syncs the tab and measure when the module changes in place", async () => {
    const user = userEvent.setup();
    const view = renderSwitchable({ showPoints: true });
    await screen.findByTestId("module-progress-measure");
    await user.click(screen.getByTestId("module-progress-measure"));
    await user.click(await screen.findByRole("option", { name: "Points" }));
    await waitFor(() => expect(screen.getByTestId("module-progress-measure").textContent).toBe("Points"));
    await user.click(screen.getByTestId("module-progress-tab-Labels"));
    expect(screen.getByTestId("module-progress-tab-Labels").getAttribute("aria-selected")).toBe("true");
    view.show({ moduleId: OTHER_MODULE });
    expect(screen.getByTestId("module-progress-tab-Assignees").getAttribute("aria-selected")).toBe("true");
    expect(screen.getByTestId("module-progress-measure").textContent).toBe("Work items");
    expect(screen.getByTestId("module-progress-toggle").getAttribute("aria-expanded")).toBe("true");
    expect(JSON.parse(localStorage.getItem(MODULE_PROGRESS_TABS_STORAGE_KEY) ?? "{}")).toEqual({ [MODULE]: "Labels" });
  });

  it("restores the next module's own stored tab on an in-place switch", async () => {
    localStorage.setItem(MODULE_PROGRESS_TABS_STORAGE_KEY, JSON.stringify({ [OTHER_MODULE]: "States" }));
    const user = userEvent.setup();
    const view = renderSwitchable();
    await screen.findByTestId("module-progress-section");
    await user.click(screen.getByTestId("module-progress-tab-Labels"));
    view.show({ moduleId: OTHER_MODULE });
    expect(screen.getByTestId("module-progress-tab-States").getAttribute("aria-selected")).toBe("true");
  });

  it("opens when an edit moves the module into its window", () => {
    const view = renderSwitchable({
      record: detailFixture({ start_date: "2999-01-01", target_date: "2999-01-31" }),
    });
    expect(screen.getByTestId("module-progress-notice").textContent).toBe("This module isn't active yet.");
    view.show({ record: detailFixture() });
    expect(screen.getByTestId("module-progress-toggle").getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByTestId("module-progress-chart")).toBeInTheDocument();
    expect(screen.getByTestId("module-progress-tabs")).toBeInTheDocument();
  });

  it("explains when an edit moves the module out of its window", async () => {
    const view = renderSwitchable();
    await screen.findByTestId("module-progress-toggle");
    view.show({ record: detailFixture({ start_date: null, target_date: null }) });
    expect(screen.getByTestId("module-progress-notice").textContent).toBe("Invalid date. Please enter valid date.");
    expect(screen.queryByTestId("module-progress-toggle")).toBeNull();
  });

  it("keeps an explicit collapse across same-module date edits", async () => {
    const user = userEvent.setup();
    const view = renderSwitchable();
    const toggle = await screen.findByTestId("module-progress-toggle");
    await user.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    view.show({ record: detailFixture({ target_date: "2026-11-30" }) });
    expect(screen.getByTestId("module-progress-toggle").getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByTestId("module-progress-tabs")).toBeNull();
  });
});

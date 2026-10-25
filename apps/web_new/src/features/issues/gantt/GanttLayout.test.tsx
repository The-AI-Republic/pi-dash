// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline layout entry (parity/specs/issues.md §6, ISS-044–049). The
// layout orders the fetched rows by the display preference, tints bars
// from the project's states, and docks the shared peek for a clicked row
// while the chart keeps its own scroll state.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import { sessionKeys } from "../../../core/session/queries.js";
import { authKeys } from "../../auth/queries.js";
import { projectKeys } from "../../projects/api/keys.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { issueKeys } from "../api/keys.js";
import { BULK_OPERATIONS_FLAG } from "../bulk/edition.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import { todayIsoDay } from "../dropdowns/calendar.js";
import { GanttLayout, barColorsFor, ganttRowPeek, orderedRows } from "./GanttLayout.js";
import { buildRows } from "./rows.js";
import { addDays } from "./scale.js";
import { ganttStatesKey } from "./useGanttData.js";

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => vi.fn(),
}));

const bulkMocks = vi.hoisted(() => ({
  update: vi.fn(),
  remove: vi.fn(),
  archive: vi.fn(),
  subscribe: vi.fn(),
}));

const bulkPending = vi.hoisted(() => ({
  update: false,
  remove: false,
  archive: false,
  subscribe: false,
}));

vi.mock("../peek/ResolvingIssuePeek.js", () => ({
  ResolvingIssuePeek: (props: {
    peeked: string;
    peekProjectId?: string | null;
    onPeekChange?: (selection: unknown) => void;
    onClose: () => void;
  }) => (
    <div
      aria-label="Work item preview"
      data-testid="issue-preview"
      data-peeked={props.peeked}
      data-peek-project={props.peekProjectId ?? ""}
    >
      <button type="button" onClick={props.onClose}>
        Close preview
      </button>
      {props.onPeekChange === undefined ? null : (
        <button
          type="button"
          onClick={() => props.onPeekChange?.({ issueId: "child", projectId: "other", nestingLevel: 1 })}
        >
          Nest child
        </button>
      )}
    </div>
  ),
}));

vi.mock("../bulk/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../bulk/index.js")>()),
  useBulkMutations: () => ({
    update: { mutateAsync: bulkMocks.update, isPending: bulkPending.update },
    remove: { mutateAsync: bulkMocks.remove, isPending: bulkPending.remove },
    archive: { mutateAsync: bulkMocks.archive, isPending: bulkPending.archive },
    subscribe: { mutateAsync: bulkMocks.subscribe, isPending: bulkPending.subscribe },
  }),
  BulkToolbar: (props: {
    count: number;
    busy: boolean;
    hideCycle: boolean;
    hideModule: boolean;
    hideEstimate: boolean;
    fromStateIds?: readonly (string | null)[];
    onUpdate: (properties: unknown) => void;
    onDelete: () => void;
    onArchive: () => void;
    onSubscribe: () => void;
    onClear: () => void;
  }) => (
    <div
      data-testid="bulk-toolbar"
      data-busy={props.busy ? "true" : undefined}
      data-hide-cycle={props.hideCycle ? "true" : undefined}
      data-hide-module={props.hideModule ? "true" : undefined}
      data-hide-estimate={props.hideEstimate ? "true" : undefined}
      data-from-states={JSON.stringify(props.fromStateIds ?? null)}
    >
      <span>{props.count} selected</span>
      <button type="button" onClick={() => props.onUpdate({ priority: "high" })}>
        Update
      </button>
      <button type="button" onClick={props.onDelete}>
        Delete
      </button>
      <button type="button" onClick={props.onArchive}>
        Archive
      </button>
      <button type="button" onClick={props.onSubscribe}>
        Subscribe
      </button>
      <button type="button" onClick={props.onClear}>
        Clear
      </button>
    </div>
  ),
}));

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const STATE = "323e4567-e89b-12d3-a456-426614174002";

function issue(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "First",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 1,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

const ISSUES = [
  issue(),
  issue({ id: "223e4567-e89b-12d3-a456-426614174010", name: "Second", sequence_id: 2, sort_order: 2, state_id: STATE }),
  issue({ id: "223e4567-e89b-12d3-a456-426614174011", name: "Third", sequence_id: 3, sort_order: 3 }),
];

function prefs(overrides: Partial<DisplayPrefs> = {}): DisplayPrefs {
  return {
    layout: "gantt_chart",
    groupBy: null,
    subGroupBy: null,
    orderBy: "sort_order",
    showEmptyGroups: true,
    showSubIssues: true,
    calendar: { mode: "month", showWeekends: true },
    properties: {},
    ...overrides,
  };
}

function renderLayout(
  options: {
    orderBy?: string;
    narrow?: boolean;
    totalCount?: number;
    issues?: IssueListItem[];
    projectRole?: number | null;
    workspaceRole?: number;
    completedCycle?: boolean;
    completedModule?: boolean;
    /** Seed the project list with an archived project (ISS-222). */
    archived?: boolean;
    /** Feature flags on the seeded project row (ISS-116 toolbar fields). */
    projectFlags?: { cycleView?: boolean; moduleView?: boolean; estimate?: string | null };
    /** Seed an empty project list (ISS-116 missing-row default). */
    missingProject?: boolean;
    /** Controlled peek the route would pass (ISS-167). */
    peekKey?: string | null;
    peekProjectId?: string | null;
    onPeekChange?: (request: unknown) => void;
    transport?: (url: string, init: { method?: string; body?: string }) => Promise<unknown>;
  } = {}
) {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  vi.stubGlobal("matchMedia", () => ({
    matches: options.narrow ?? false,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
  createAppClient(
    "https://api.test",
    (options.transport ??
      (() => {
        throw new Error("unexpected fetch in a seeded render");
      })) as never
  );
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(ganttStatesKey(SLUG, PROJECT), [
    {
      id: STATE,
      name: "Doing",
      color: "#123456",
      group: "started",
      sequence: 2,
      project_id: PROJECT,
      workspace_id: SLUG,
      description: "",
      default: false,
    },
  ]);
  queryClient.setQueryData(issueKeys.detail(SLUG, ISSUES[0]!.id), { ...issue(), id: ISSUES[0]!.id });
  queryClient.setQueryData(sessionKeys.workspaces, [
    { id: "ws-1", name: "Acme", slug: SLUG, logo_url: null, total_members: 3, role: options.workspaceRole ?? 15 },
  ]);
  queryClient.setQueryData(
    authKeys.projectRoles(SLUG),
    options.projectRole === null ? {} : { [PROJECT]: options.projectRole ?? 15 }
  );
  const flagSeed = { cycleView: true, moduleView: true, estimate: "estimate-1" as string | null };
  const projectFlags = { ...flagSeed, ...options.projectFlags };
  queryClient.setQueryData(
    projectKeys.list(SLUG),
    options.missingProject === true
      ? []
      : [
          {
            id: PROJECT,
            name: "Website",
            identifier: "WEB",
            archived_at: options.archived === true ? "2026-10-01" : null,
            cycle_view: projectFlags.cycleView,
            module_view: projectFlags.moduleView,
            estimate: projectFlags.estimate,
          },
        ]
  );
  render(
    <QueryClientProvider client={queryClient}>
      <React.Suspense fallback={<p>suspended</p>}>
        <GanttLayout
          workspaceSlug={SLUG}
          projectId={PROJECT}
          projectIdentifier="WEB"
          issues={options.issues ?? ISSUES}
          totalCount={options.totalCount ?? (options.issues ?? ISSUES).length}
          prefs={prefs({ orderBy: options.orderBy ?? "sort_order" })}
          weekStart={0}
          {...(options.completedCycle === undefined ? {} : { completedCycle: options.completedCycle })}
          {...(options.completedModule === undefined ? {} : { completedModule: options.completedModule })}
          {...(options.peekKey === undefined ? {} : { peekKey: options.peekKey })}
          {...(options.peekProjectId === undefined ? {} : { peekProjectId: options.peekProjectId })}
          {...(options.onPeekChange === undefined ? {} : { onPeekChange: options.onPeekChange })}
        />
      </React.Suspense>
    </QueryClientProvider>
  );
  return { queryClient };
}

afterEach(() => {
  window.history.replaceState({}, "", "/");
  delete edition.flags[BULK_OPERATIONS_FLAG];
  bulkPending.update = false;
  bulkPending.remove = false;
  bulkPending.archive = false;
  bulkPending.subscribe = false;
  resetAppClient();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe("ganttRowPeek", () => {
  it("opens the row's own project when the list column knows it", () => {
    const [row] = buildRows([issue({ project_id: "other-project" })], "WEB");
    expect(ganttRowPeek(row!, [issue({ project_id: "other-project" })], PROJECT)).toEqual({
      issueId: row!.id,
      projectId: "other-project",
      nestingLevel: 0,
    });
  });

  it("falls back to the route project for a row the list no longer holds", () => {
    const [row] = buildRows([issue()], "WEB");
    expect(ganttRowPeek(row!, [], PROJECT)).toEqual({ issueId: row!.id, projectId: PROJECT, nestingLevel: 0 });
  });
});

describe("orderedRows", () => {
  it("keeps the server order for every preference", () => {
    // The server answers `order_by`; a local reverse would double it.
    for (const orderBy of ["sort_order", "-sort_order", "-created_at", "start_date"]) {
      const rows = orderedRows(ISSUES, prefs({ orderBy }), "WEB");
      expect(rows.map((row) => row.name)).toEqual(["First", "Second", "Third"]);
    }
  });
});

describe("barColorsFor", () => {
  it("takes the color of each row's state and skips uncolored rows", () => {
    const rows = buildRows(ISSUES, "WEB");
    expect(barColorsFor(rows, [{ id: STATE, color: "#123456" } as never])).toEqual({ [ISSUES[1]!.id]: "#123456" });
  });

  it("returns no colors without states", () => {
    expect(barColorsFor(buildRows(ISSUES, "WEB"), [])).toEqual({});
  });
});

describe("GanttLayout", () => {
  it("renders the chart with one sidebar row per issue", () => {
    renderLayout();
    expect(screen.getByTestId("gantt-layout")).toBeInTheDocument();
    expect(screen.getAllByTestId("gantt-sidebar-row")).toHaveLength(3);
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("3 work items");
  });

  it("keeps the server order under a reverse manual sort", () => {
    // `order_by=-sort_order` already arrives descending; the layout must
    // not flip it back (ISS-044/045).
    renderLayout({ orderBy: "-sort_order" });
    const names = screen.getAllByTestId("gantt-sidebar-row").map((row) => row.textContent);
    expect(names[0]).toContain("First");
    expect(names[2]).toContain("Third");
  });

  it("shows the server total, not the loaded page, in the header (ISS-044)", () => {
    // The list query is one page; the header count is the issue total.
    renderLayout({ totalCount: 240 });
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("240 work items");
    expect(screen.getAllByTestId("gantt-sidebar-row")).toHaveLength(3);
  });

  it("never reports fewer rows than it renders", () => {
    renderLayout({ totalCount: 1 });
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("3 work items");
  });

  it("renders rows in the order the server sent them", () => {
    // Manual order is the server's `order_by=sort_order` order; the layout
    // must not re-sort or reverse it (ISS-044/045).
    const ranks = [
      issue({ id: "423e4567-e89b-12d3-a456-426614174020", name: "Rank 30", sequence_id: 30, sort_order: 30 }),
      issue({ id: "423e4567-e89b-12d3-a456-426614174021", name: "Rank 10", sequence_id: 10, sort_order: 10 }),
    ];
    renderLayout({ issues: ranks });
    const names = screen.getAllByTestId("gantt-sidebar-row").map((row) => row.textContent);
    expect(names[0]).toContain("Rank 30");
    expect(names[1]).toContain("Rank 10");
  });

  it("starts every sidebar link at the workspace browse route", () => {
    renderLayout();
    for (const link of screen.getAllByRole("link")) {
      expect(link).toHaveAttribute("href", expect.stringMatching(/^\/acme\/browse\/WEB-\d+$/));
    }
  });

  it("keeps an unconfirmed create visible but inert in the timeline", () => {
    // A quick create prepends a temp row to the list cache; it has no
    // server identity, so it must not link or open a 404ing peek (ISS-049).
    renderLayout({
      issues: [issue({ id: "temp-abc", name: "Draft title", sequence_id: 0 }), ...ISSUES],
    });
    expect(screen.getAllByTestId("gantt-sidebar-row")).toHaveLength(4);
    expect(screen.queryByRole("link", { name: "Draft title" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("gantt-row-unconfirmed"));
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();
  });

  it("opens the shared peek for a clicked row and closes it", async () => {
    renderLayout();
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: "First" }));
    const preview = await screen.findByTestId("issue-preview");
    expect(preview).toHaveAttribute("data-peeked", ISSUES[0]!.id);
    expect(document.querySelector(`[data-lane="${ISSUES[0]!.id}"]`)).toHaveAttribute("data-peeked", "true");
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    await waitFor(() => expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument());
    expect(document.querySelector(`[data-lane="${ISSUES[0]!.id}"]`)).not.toHaveAttribute("data-peeked");
  });

  it("names the opened row in the address and strips it on close (ISS-167)", async () => {
    const push = vi.spyOn(window.history, "pushState");
    renderLayout();
    fireEvent.click(screen.getByRole("link", { name: "First" }));
    await screen.findByTestId("issue-preview");
    expect(push).not.toHaveBeenCalled();
    expect(new URLSearchParams(window.location.search).get("peekIssueId")).toBe(ISSUES[0]!.id);
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    await waitFor(() => expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument());
    expect(new URLSearchParams(window.location.search).get("peekIssueId")).toBeNull();
  });

  it("opens the peek from a deep-linked id, keeping a foreign project", async () => {
    window.history.replaceState({}, "", `/acme/issues?peekIssueId=${ISSUES[1]!.id}&peekProjectId=other-project`);
    renderLayout();
    const preview = await screen.findByTestId("issue-preview");
    expect(preview).toHaveAttribute("data-peeked", ISSUES[1]!.id);
    expect(preview).toHaveAttribute("data-peek-project", "other-project");
  });

  it("hands a host-owned address the row it activated and never writes the URL itself", () => {
    const onPeekChange = vi.fn();
    renderLayout({ peekKey: null, onPeekChange });
    fireEvent.click(screen.getByRole("link", { name: "First" }));
    expect(onPeekChange).toHaveBeenCalledWith({
      issueId: ISSUES[0]!.id,
      projectId: PROJECT,
      nestingLevel: 0,
    });
    // Nothing mounts until the host writes the address back down.
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();
    expect(new URLSearchParams(window.location.search).get("peekIssueId")).toBeNull();
  });

  it("passes a nested child report from the mounted panel to the host", async () => {
    const onPeekChange = vi.fn();
    renderLayout({ peekKey: ISSUES[0]!.id, onPeekChange });
    await screen.findByTestId("issue-preview");
    fireEvent.click(screen.getByRole("button", { name: "Nest child" }));
    expect(onPeekChange).toHaveBeenLastCalledWith({ issueId: "child", projectId: "other", nestingLevel: 1 });
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onPeekChange).toHaveBeenLastCalledWith(null);
  });

  it("shows the panel the controlled address names, even for a foreign project", async () => {
    renderLayout({ peekKey: ISSUES[2]!.id, peekProjectId: "other-project", onPeekChange: vi.fn() });
    const preview = await screen.findByTestId("issue-preview");
    expect(preview).toHaveAttribute("data-peeked", ISSUES[2]!.id);
    expect(preview).toHaveAttribute("data-peek-project", "other-project");
  });
});

describe("GanttLayout hover preview (ISS-133)", () => {
  it("opens a preview card from a bar with its key, state and title", async () => {
    const user = userEvent.setup();
    renderLayout({
      issues: [
        issue({ start_date: "2026-10-01", target_date: "2026-10-05", state_id: STATE, name: "Dated row" }),
        ...ISSUES.slice(1),
      ],
    });
    const bar = screen.getAllByTestId("gantt-bar")[0]!;
    await user.hover(bar);
    const card = await screen.findByTestId("work-item-preview");
    expect(card).toHaveTextContent("WEB-1");
    expect(card).toHaveTextContent("Dated row");
    expect(card).toHaveTextContent("Doing");
    expect(screen.getByTestId("preview-card-dates")).toHaveTextContent("Oct 01, 2026 - Oct 05, 2026");
  });

  it("opens the same card from the sidebar reference", async () => {
    const user = userEvent.setup();
    renderLayout();
    await user.hover(screen.getByRole("link", { name: "First" }));
    const card = await screen.findByTestId("work-item-preview");
    expect(card).toHaveTextContent("WEB-1");
    // The first row carries no dates and no state, so neither line renders.
    expect(screen.queryByTestId("preview-card-dates")).not.toBeInTheDocument();
    expect(screen.queryByTestId("preview-card-state")).not.toBeInTheDocument();
  });
});

describe("GanttLayout edit standing (ISS-050–055)", () => {
  it("omits every edit affordance without standing", () => {
    renderLayout({ projectRole: null });
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
    expect(screen.queryAllByTestId("gantt-resize-left")).toHaveLength(0);
    expect(screen.queryAllByTestId("gantt-undated-lane")).toHaveLength(0);
    expect(screen.getByTestId("gantt-sidebar")).not.toHaveAttribute("data-reorder");
  });

  it("gives a project member the bars, add lane, quick-add and reorder", () => {
    renderLayout({
      projectRole: 15,
      issues: [issue({ start_date: "2026-10-01", target_date: "2026-10-05" }), ...ISSUES.slice(1)],
    });
    expect(screen.getAllByTestId("gantt-resize-left")).toHaveLength(1);
    expect(screen.getAllByTestId("gantt-undated-lane")).toHaveLength(2);
    expect(screen.getByTestId("gantt-quick-add")).toBeInTheDocument();
    expect(screen.getByTestId("gantt-sidebar")).toHaveAttribute("data-reorder", "true");
  });

  it("keeps a member's timeline read-only outside manual order", () => {
    renderLayout({
      projectRole: 15,
      orderBy: "-created_at",
      issues: [issue({ start_date: "2026-10-01", target_date: "2026-10-05" }), ...ISSUES.slice(1)],
    });
    expect(screen.getByTestId("gantt-sidebar")).not.toHaveAttribute("data-reorder");
    // Bars still move and lanes still plant outside manual order.
    expect(screen.getAllByTestId("gantt-resize-left")).toHaveLength(1);
  });

  it("treats a descending manual sort as non-manual for reorder (ISS-050)", () => {
    renderLayout({
      projectRole: 15,
      orderBy: "-sort_order",
      issues: [issue({ start_date: "2026-10-01", target_date: "2026-10-05" }), ...ISSUES.slice(1)],
    });
    expect(screen.getByTestId("gantt-sidebar")).not.toHaveAttribute("data-reorder");
  });

  it("gives a workspace admin with no project membership no edit affordance (ISS-058)", () => {
    renderLayout({ workspaceRole: 20, projectRole: null });
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
    expect(screen.queryAllByTestId("gantt-resize-left")).toHaveLength(0);
    expect(screen.queryAllByTestId("gantt-undated-lane")).toHaveLength(0);
    expect(screen.getByTestId("gantt-sidebar")).not.toHaveAttribute("data-reorder");
  });

  it("hides the quick-add in a completed cycle", () => {
    renderLayout({ projectRole: 15, completedCycle: true });
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
  });

  it("hides the quick-add in a completed module", () => {
    renderLayout({ projectRole: 15, completedModule: true });
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
  });

  it("hides every edit affordance in an archived project (ISS-222)", () => {
    renderLayout({
      projectRole: 15,
      archived: true,
      issues: [issue({ start_date: "2026-10-01", target_date: "2026-10-05" }), ...ISSUES.slice(1)],
    });
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
    expect(screen.queryAllByTestId("gantt-resize-left")).toHaveLength(0);
    expect(screen.queryAllByTestId("gantt-undated-lane")).toHaveLength(0);
    expect(screen.getByTestId("gantt-sidebar")).not.toHaveAttribute("data-reorder");
  });
});

describe("GanttLayout write wiring (ISS-050–055)", () => {
  interface Call {
    url: string;
    method: string;
    body: string;
  }

  function recordingTransport(status = 200) {
    const calls: Call[] = [];
    const transport = async (url: string, init: { method?: string; body?: string }) => {
      const method = init.method ?? "GET";
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ csrf_token: "csrf" }),
        };
      }
      calls.push({ url, method, body: init.body ?? "" });
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () =>
          method === "POST" && url.endsWith("/issues/")
            ? JSON.stringify({
                ...issue({ id: "623e4567-e89b-12d3-a456-426614174030", name: "New timeline item" }),
                start_date: null,
                target_date: null,
                deleted_at: null,
              })
            : JSON.stringify({ message: "ok" }),
      };
    };
    return { calls, transport: transport as never };
  }

  const DATED = issue({
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "First",
    start_date: "2026-10-01",
    target_date: "2026-10-05",
  });

  it("reorders through the single-issue PATCH with the neighbour-derived rank (ISS-050)", async () => {
    const { calls, transport } = recordingTransport(204);
    renderLayout({ projectRole: 15, issues: [DATED, ...ISSUES.slice(1)], transport });

    const rows = screen.getAllByTestId("gantt-sidebar-row");
    rows.forEach((row, index) => {
      row.getBoundingClientRect = () =>
        ({ left: 0, top: index * 44, width: 300, height: 44, right: 300, bottom: index * 44 + 44 }) as DOMRect;
    });
    fireEvent.mouseDown(rows[0]!, { button: 0, clientY: 22 });
    fireEvent.mouseMove(rows[2]!, { clientY: 100 });
    fireEvent.mouseUp(rows[2]!);

    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    const patch = calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toBe(`https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/issues/${DATED.id}/`);
    // Rank 1 lands between rank 2 and rank 3, so it takes the midpoint.
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ sort_order: 2.5 });
  });

  it("posts the quick-add with today and tomorrow seeded (ISS-055)", async () => {
    const { calls, transport } = recordingTransport(201);
    renderLayout({ projectRole: 15, transport });

    fireEvent.click(screen.getByRole("button", { name: /new work item/i }));
    fireEvent.change(screen.getByLabelText("Work item title"), { target: { value: "New timeline item" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    await waitFor(() => expect(calls.some((call) => call.url.endsWith("/issues/"))).toBe(true));
    const post = calls.find((call) => call.method === "POST" && call.url.endsWith("/issues/"));
    expect(post?.url).toBe(`https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/issues/`);
    const today = todayIsoDay();
    expect(JSON.parse(post?.body ?? "{}")).toEqual({
      name: "New timeline item",
      start_date: today,
      target_date: addDays(today, 1),
    });
  });

  it("persists a moved bar through the batch issue-dates call (ISS-051)", async () => {
    const { calls, transport } = recordingTransport();
    renderLayout({ projectRole: 15, issues: [DATED, ...ISSUES.slice(1)], transport });

    const lane = document.querySelector<HTMLElement>(`[data-lane="${DATED.id}"]`);
    const bar = lane?.querySelector<HTMLElement>('[data-testid="gantt-bar"]');
    if (bar === null || bar === undefined) throw new Error("no bar");
    fireEvent.mouseDown(bar, { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 180 });
    fireEvent.mouseUp(document);

    await waitFor(() => expect(calls.some((call) => call.url.endsWith("/issue-dates/"))).toBe(true));
    const post = calls.find((call) => call.url.endsWith("/issue-dates/"));
    expect(post?.method).toBe("POST");
    expect(JSON.parse(post?.body ?? "{}")).toEqual({
      updates: [{ id: DATED.id, start_date: "2026-10-03", target_date: "2026-10-07" }],
    });
  });

  it("plants a block through the single-issue PATCH (ISS-054)", async () => {
    const { calls, transport } = recordingTransport(204);
    renderLayout({ projectRole: 15, issues: [DATED, ...ISSUES.slice(1)], transport });

    const lane = screen.getAllByTestId("gantt-undated-lane")[0];
    if (lane === undefined) throw new Error("no undated lane");
    lane.getBoundingClientRect = () => ({ left: 0, top: 0, width: 800, height: 44, right: 800, bottom: 44 }) as DOMRect;
    fireEvent.mouseMove(lane, { clientX: 2 * 40 });
    fireEvent.click(screen.getByTestId("gantt-add-block"));

    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    const patch = calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toBe(`https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/issues/${ISSUES[1]!.id}/`);
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({
      start_date: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/),
      target_date: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/),
    });
  });
});

describe("GanttLayout guest standing (ISS-058)", () => {
  it("gives a project guest the chart without edit affordances", () => {
    renderLayout({
      projectRole: 5,
      issues: [issue({ start_date: "2026-10-01", target_date: "2026-10-05" }), ...ISSUES.slice(1)],
    });
    expect(screen.getAllByTestId("gantt-sidebar-row")).toHaveLength(3);
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
    expect(screen.queryAllByTestId("gantt-resize-left")).toHaveLength(0);
    expect(screen.queryAllByTestId("gantt-undated-lane")).toHaveLength(0);
    expect(screen.getByTestId("gantt-sidebar")).not.toHaveAttribute("data-reorder");
  });
});

describe("GanttLayout load-more sentinel (ISS-059)", () => {
  it("trails a pulsing placeholder while the server holds more rows", () => {
    renderLayout({ totalCount: ISSUES.length + 2 });
    expect(screen.getByTestId("gantt-load-more")).toBeInTheDocument();
    cleanup();
    renderLayout();
    expect(screen.queryByTestId("gantt-load-more")).toBeNull();
  });
});

describe("GanttLayout bulk selection (ISS-116)", () => {
  beforeEach(() => {
    for (const action of Object.values(bulkMocks)) action.mockResolvedValue(undefined);
  });

  function enableBulk(): void {
    edition.flags[BULK_OPERATIONS_FLAG] = true;
  }

  it("shows no checkbox without the bulk operations flag", () => {
    renderLayout({ projectRole: 15 });
    expect(screen.queryByLabelText("Select all work items")).toBeNull();
    expect(screen.queryByLabelText("Select WEB-1")).toBeNull();
    expect(screen.queryByTestId("bulk-toolbar")).toBeNull();
  });

  it("shows sidebar and header checkboxes for a member on a bulk build", () => {
    enableBulk();
    renderLayout({ projectRole: 15 });
    expect(screen.getByLabelText("Select all work items")).toBeInTheDocument();
    expect(screen.getByLabelText("Select WEB-1")).toBeInTheDocument();
    expect(screen.getByLabelText("Select WEB-2")).toBeInTheDocument();
    expect(screen.getByLabelText("Select WEB-3")).toBeInTheDocument();
    expect(screen.queryByTestId("bulk-toolbar")).toBeNull();
  });

  it("hides every checkbox from a guest even on a bulk build", () => {
    enableBulk();
    renderLayout({ projectRole: 5 });
    expect(screen.queryByLabelText("Select all work items")).toBeNull();
    expect(screen.queryByLabelText("Select WEB-1")).toBeNull();
  });

  it("hands the toolbar every selected row's state for the workflow gate (ISS-233)", () => {
    enableBulk();
    renderLayout({ projectRole: 15 });
    fireEvent.click(screen.getByLabelText("Select WEB-1"));
    fireEvent.click(screen.getByLabelText("Select WEB-2"));
    expect(screen.getByTestId("bulk-toolbar")).toHaveAttribute("data-from-states", JSON.stringify([null, STATE]));
  });

  it("reveals the toolbar on selection and clears it", async () => {
    enableBulk();
    renderLayout({ projectRole: 15 });
    fireEvent.click(screen.getByLabelText("Select WEB-1"));
    fireEvent.click(screen.getByLabelText("Select WEB-2"));
    expect(screen.getByTestId("bulk-toolbar")).toHaveTextContent("2 selected");
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    await waitFor(() => expect(screen.queryByTestId("bulk-toolbar")).toBeNull());
  });

  it("selects the whole list from the header tri-state", () => {
    enableBulk();
    renderLayout({ projectRole: 15 });
    fireEvent.click(screen.getByLabelText("Select all work items"));
    expect(screen.getByTestId("bulk-toolbar")).toHaveTextContent("3 selected");
  });

  it("runs the four bulk actions across the selection, then clears", async () => {
    enableBulk();
    const ids = [ISSUES[0]!.id, ISSUES[2]!.id];
    const cases = [
      { button: "Update", mock: bulkMocks.update, args: [{ issueIds: ids, properties: { priority: "high" } }] },
      { button: "Delete", mock: bulkMocks.remove, args: [ids] },
      { button: "Archive", mock: bulkMocks.archive, args: [ids] },
      { button: "Subscribe", mock: bulkMocks.subscribe, args: [ids] },
    ] as const;
    for (const { button, mock, args } of cases) {
      renderLayout({ projectRole: 15 });
      fireEvent.click(screen.getByLabelText("Select WEB-1"));
      fireEvent.click(screen.getByLabelText("Select WEB-3"));
      fireEvent.click(screen.getByRole("button", { name: button }));
      await waitFor(() => expect(mock).toHaveBeenCalledWith(...args));
      await waitFor(() => expect(screen.queryByTestId("bulk-toolbar")).toBeNull());
      cleanup();
    }
  });

  it("keeps the selection when a bulk action fails", async () => {
    enableBulk();
    const cases = [
      { button: "Update", mock: bulkMocks.update },
      { button: "Delete", mock: bulkMocks.remove },
      { button: "Archive", mock: bulkMocks.archive },
      { button: "Subscribe", mock: bulkMocks.subscribe },
    ] as const;
    for (const { button, mock } of cases) {
      mock.mockRejectedValueOnce(new Error("bulk failed"));
      renderLayout({ projectRole: 15 });
      fireEvent.click(screen.getByLabelText("Select WEB-1"));
      fireEvent.click(screen.getByRole("button", { name: button }));
      await waitFor(() => expect(mock).toHaveBeenCalledTimes(1));
      expect(screen.getByTestId("bulk-toolbar")).toHaveTextContent("1 selected");
      cleanup();
    }
  });

  it("marks the toolbar busy while any bulk mutation is pending", () => {
    enableBulk();
    for (const key of ["update", "remove", "archive", "subscribe"] as const) {
      bulkPending[key] = true;
      renderLayout({ projectRole: 15 });
      fireEvent.click(screen.getByLabelText("Select WEB-1"));
      expect(screen.getByTestId("bulk-toolbar")).toHaveAttribute("data-busy", "true");
      bulkPending[key] = false;
      cleanup();
    }
    renderLayout({ projectRole: 15 });
    fireEvent.click(screen.getByLabelText("Select WEB-1"));
    expect(screen.getByTestId("bulk-toolbar")).not.toHaveAttribute("data-busy");
  });

  it("excludes epics and unconfirmed rows from selection", () => {
    enableBulk();
    renderLayout({
      projectRole: 15,
      issues: [
        issue({ id: "epic-1", sequence_id: 9, name: "Epic row", is_epic: true }),
        issue({ id: "temp-draft", sequence_id: 10, name: "Draft row" }),
        ...ISSUES.slice(0, 1),
      ],
    });
    expect(screen.queryByLabelText("Select WEB-9")).toBeNull();
    expect(screen.queryByLabelText("Select WEB-10")).toBeNull();
    fireEvent.click(screen.getByLabelText("Select all work items"));
    expect(screen.getByTestId("bulk-toolbar")).toHaveTextContent("1 selected");
  });

  it("passes the project's feature flags to the toolbar", () => {
    enableBulk();
    renderLayout({
      projectRole: 15,
      projectFlags: { cycleView: false, moduleView: true, estimate: null },
    });
    fireEvent.click(screen.getByLabelText("Select WEB-1"));
    const toolbar = screen.getByTestId("bulk-toolbar");
    expect(toolbar).toHaveAttribute("data-hide-cycle", "true");
    expect(toolbar).not.toHaveAttribute("data-hide-module");
    expect(toolbar).toHaveAttribute("data-hide-estimate", "true");
  });

  it("shows every toolbar field when the project row is missing", () => {
    enableBulk();
    renderLayout({ projectRole: 15, missingProject: true });
    fireEvent.click(screen.getByLabelText("Select WEB-1"));
    const toolbar = screen.getByTestId("bulk-toolbar");
    expect(toolbar).not.toHaveAttribute("data-hide-cycle");
    expect(toolbar).not.toHaveAttribute("data-hide-module");
    expect(toolbar).not.toHaveAttribute("data-hide-estimate");
  });
});

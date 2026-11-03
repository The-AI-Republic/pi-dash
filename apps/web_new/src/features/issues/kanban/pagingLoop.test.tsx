// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Column paging loop (parity/specs/issues.md §5, ISS-042/043): the real
// board read drives the real board while a scripted transport answers
// page one and page two and a controllable observer plays the scroll.
// Scrolling a 32-issue column to its end must fetch the cursor page,
// merge it, and render the last card's name.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  prefs: { filters: {}, display_filters: {}, display_properties: {}, rich_filters: {} },
  navigate: vi.fn(),
  moveCard: vi.fn(),
  attachExisting: vi.fn(),
  notifyInfo: vi.fn(),
  copy: vi.fn(),
  createDialog: vi.fn(),
  addExisting: vi.fn(),
  deleteDialog: vi.fn(),
  toggleCollapsedGroup: vi.fn(),
  mutateAsync: vi.fn(),
  attach: vi.fn(),
  attachInput: vi.fn(),
  permissions: { role: 15, isMember: true, canEdit: true, isAdmin: false },
  me: { id: "me-1" } as { id: string } | null,
  groupLabels: {} as Record<string, Map<string, string>>,
}));

const script = vi.hoisted(() => ({
  calls: [] as string[],
  first: {} as Record<string, unknown>,
  pages: {} as Record<string, Record<string, unknown>>,
  client: null as unknown,
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => script.client }));
vi.mock("../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/index.js")>()),
  usePermissions: () => mocks.permissions,
  useMe: () => ({ data: mocks.me }),
}));
vi.mock("../list/reference.js", () => ({
  useListReferences: () => ({
    labels: mocks.groupLabels,
    stateRows: [],
    stateNames: new Map(),
    assigneeNames: new Map(),
    labelNames: new Map(),
    cycleNames: new Map(),
    moduleNames: new Map(),
    openCycleIds: new Set<string>(),
  }),
}));
vi.mock("../calendar/useMedia.js", () => ({
  useTouchInput: () => false,
}));
vi.mock("../list/attach.js", () => ({
  useAttachExisting: (input: unknown) => {
    mocks.attachInput(input);
    return { attach: mocks.attachExisting, isAttaching: false };
  },
}));
vi.mock("./mutations.js", () => ({
  useBoardCardMove: () => ({ isMoving: false, moveCard: mocks.moveCard }),
}));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/shell/index.js")>()),
  notifyInfo: (...args: unknown[]) => mocks.notifyInfo(...args),
}));
vi.mock("../components/WorkItemCreateDialog.js", () => ({
  WorkItemCreateDialog: () => null,
}));
vi.mock("../list/AddExistingModal.js", () => ({
  AddExistingModal: () => null,
}));
vi.mock("../lifecycle/DeleteWorkItemDialog.js", () => ({
  DeleteWorkItemDialog: () => null,
}));
vi.mock("../list/collapse.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../list/collapse.js")>()),
  useToggleCollapsedGroup: () => ({
    isSaving: false,
    toggleCollapsedGroup: mocks.toggleCollapsedGroup,
  }),
}));
vi.mock("../api/create.js", () => ({
  useCreateWorkItem: () => ({ mutateAsync: mocks.mutateAsync }),
}));
vi.mock("../list/mutations.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../list/mutations.js")>()),
  attachCreatedIssue: mocks.attach,
}));
vi.mock("../peek/ResolvingIssuePeek.js", () => ({
  ResolvingIssuePeek: () => null,
}));
vi.mock("../peek/peekLink.js", () => ({
  copyCardLink: mocks.copy,
}));
vi.mock("../../../shared/shell/narrowViewport.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/shell/narrowViewport.js")>()),
  useNarrowViewport: () => false,
}));
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => mocks.navigate,
}));

import { createClient, type IssueListItem, type Project, type ProjectDetails } from "@pidash/api-client";

import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { prefsQueryOptions } from "../prefs/prefsQuery.js";
import { projectDetailsQueryOptions, projectsQueryOptions } from "../../projects/index.js";
import { KanbanBoard } from "./KanbanBoard.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const STATE = "323e4567-e89b-12d3-a456-426614174002";
const DONE = "423e4567-e89b-12d3-a456-426614174003";
const ENTITY: IssuesEntity = { scope: "project", projectId: PROJECT };

function row(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "Fix login",
    state_id: STATE,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 42,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

const ENVELOPE = {
  grouped_by: "state_id",
  sub_grouped_by: null,
  total_count: 32,
  next_cursor: "30:1:0",
  prev_cursor: "30:0:0",
  next_page_results: true,
  prev_page_results: false,
  count: 32,
  total_results: 32,
};

function page(rows: IssueListItem[]): Record<string, unknown> {
  return {
    ...ENVELOPE,
    results: {
      [STATE]: { results: rows, total_results: 32 },
      [DONE]: { results: [], total_results: 0 },
    },
  };
}

function columnRows(start: number, count: number): IssueListItem[] {
  return Array.from({ length: count }, (_, index) => {
    const number = start + index + 1;
    return row({
      id: `123e4567-e89b-12d3-a456-42661417${String(4000 + start + index).slice(-4)}`,
      name: `KB page ${String(number).padStart(2, "0")}`,
      sequence_id: 100 + start + index,
      sort_order: start + index + 1,
    });
  });
}

type ObserverCallback = (entries: Array<{ isIntersecting: boolean }>) => void;

interface SeenObserver {
  callback: ObserverCallback;
  targets: Element[];
  disconnected: boolean;
}

const seen = vi.hoisted(() => ({ observers: [] as SeenObserver[] }));

function installObserver(): void {
  seen.observers = [];
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      record: SeenObserver;
      constructor(callback: ObserverCallback) {
        this.record = { callback, targets: [], disconnected: false };
        seen.observers.push(this.record);
      }
      observe(target: Element): void {
        this.record.targets.push(target);
      }
      unobserve(target: Element): void {
        this.record.targets = this.record.targets.filter((entry) => entry !== target);
      }
      disconnect(): void {
        this.record.disconnected = true;
      }
    }
  );
}

/** Live observers (never disconnected) watching one target. */
function liveObserversOf(target: Element): SeenObserver[] {
  return seen.observers.filter((observer) => !observer.disconnected && observer.targets.includes(target));
}

/** Fire one observed node by its observer index. */
function fireAt(index: number, visible: boolean): void {
  const observer = seen.observers[index];
  if (observer === undefined) throw new Error(`no observer at ${index}`);
  act(() => observer.callback([{ isIntersecting: visible }]));
}

/** A row's observed shell: the placeholder itself, or a content wrapper. */
function shellOf(card: HTMLElement): Element {
  if (card.dataset.virtualized === "true") return card;
  const wrapper = card.parentElement;
  if (wrapper === null) throw new Error("content card has no shell");
  return wrapper;
}

/** Fire one column row's shell, placeholder or content alike. */
function fireRow(scope: HTMLElement, row: number, visible: boolean): void {
  const cards = within(scope).getAllByTestId("kanban-card");
  const card = cards[row];
  if (card === undefined) throw new Error(`no card at row ${row}`);
  const target = shellOf(card);
  const index = seen.observers.findIndex((observer) => !observer.disconnected && observer.targets.includes(target));
  if (index === -1) throw new Error(`row ${row} has no live observer`);
  fireAt(index, visible);
}

function sentinelObserver(): SeenObserver {
  const sentinel = document.querySelector('[data-testid="kanban-column-sentinel"]');
  if (sentinel === null) throw new Error("no sentinel mounted");
  const observer = seen.observers.find((entry) => !entry.disconnected && entry.targets.includes(sentinel));
  if (observer === undefined) throw new Error("no live sentinel observer");
  return observer;
}

/**
 * Every mounted shell and sentinel is watched by a live observer whose
 * every target is still mounted: nothing observes a detached node and
 * no mounted node goes unwatched.
 */
function auditObservers(scope: HTMLElement): void {
  const cards = within(scope).queryAllByTestId("kanban-card");
  for (const [row, card] of cards.entries()) {
    const live = liveObserversOf(shellOf(card));
    expect(live.length, `row ${row} live observers`).toBeGreaterThan(0);
  }
  for (const sentinel of within(scope).queryAllByTestId("kanban-column-sentinel")) {
    expect(liveObserversOf(sentinel).length, "sentinel live observers").toBeGreaterThan(0);
  }
  for (const observer of seen.observers) {
    if (observer.disconnected) continue;
    for (const target of observer.targets) {
      expect(target.isConnected, "observer target stays mounted").toBe(true);
    }
  }
}

function prefs(): DisplayPrefs {
  return {
    layout: "kanban",
    groupBy: "state",
    subGroupBy: null,
    orderBy: "sort_order",
    showEmptyGroups: true,
    showSubIssues: false,
    calendar: { mode: "month", showWeekends: true },
    properties: {},
  };
}

function setup(strict = false) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const client = createClient({
    baseUrl: "https://api.test",
    transport: async (url) => {
      script.calls.push(url);
      const cursor = new URL(url).searchParams.get("cursor") ?? "";
      const body = cursor === "" ? script.first : (script.pages[cursor] ?? script.first);
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(body),
      };
    },
  });
  script.client = client;
  queryClient.setQueryData(prefsQueryOptions(client, "acme", ENTITY).queryKey, mocks.prefs);
  queryClient.setQueryData(projectsQueryOptions(client, "acme").queryKey, [
    { id: PROJECT, cycle_view: true, module_view: true } as Project,
  ]);
  queryClient.setQueryData(projectDetailsQueryOptions(client, "acme").queryKey, [
    { id: PROJECT, estimate: null, default_state: null, timezone: "UTC" } as ProjectDetails,
  ]);
  const board = (
    <QueryClientProvider client={queryClient}>
      <KanbanBoard
        workspaceSlug="acme"
        entity={ENTITY}
        projectId={PROJECT}
        projectIdentifier="WEB"
        prefs={prefs()}
        canEdit
        onPeekChange={() => {}}
      />
    </QueryClientProvider>
  );
  return strict ? render(<React.StrictMode>{board}</React.StrictMode>) : render(board);
}

function column(name: string): HTMLElement {
  const found = screen
    .getAllByTestId("kanban-column")
    .find((element) => element.getAttribute("data-column-name") === name);
  if (found === undefined) throw new Error(`no column ${name}`);
  return found;
}

/** One column's cards in order; virtualized placeholders read as "". */
function columnCards(scope: HTMLElement): string[] {
  return within(scope)
    .getAllByTestId("kanban-card")
    .map((card) => within(card).queryByTestId("kanban-card-name")?.textContent?.trim() ?? "");
}

beforeEach(() => {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  vi.clearAllMocks();
  mocks.me = { id: "me-1" };
  mocks.permissions = { role: 15, isMember: true, canEdit: true, isAdmin: false };
  mocks.mutateAsync.mockResolvedValue({ id: "created-1", sequence_id: 50 });
  mocks.groupLabels = {
    state: new Map([
      [STATE, "Todo"],
      [DONE, "Done"],
    ]),
  };
  script.calls = [];
  script.first = page(columnRows(0, 30));
  script.pages = { "30:1:0": page(columnRows(30, 2)) };
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("column paging loop", () => {
  it.each([false, true])(
    "renders the last page after scrolling a grouped column to its end (strict: %s)",
    async (strict) => {
      installObserver();
      setup(strict);
      await waitFor(() => expect(screen.queryByTestId("kanban-loading")).not.toBeInTheDocument());
      const home = column("Todo");
      expect(home).toBeInTheDocument();
      await waitFor(() => expect(within(home).getAllByTestId("kanban-card")).toHaveLength(30));
      expect(script.calls).toHaveLength(1);
      auditObservers(home);

      // The column top is in view; the sentinel sits below the fold.
      for (let index = 0; index < 5; index += 1) fireRow(home, index, true);
      expect(columnCards(home).slice(0, 5)).toEqual([
        "KB page 01",
        "KB page 02",
        "KB page 03",
        "KB page 04",
        "KB page 05",
      ]);
      expect(columnCards(home)).not.toContain("KB page 32");
      expect(within(home).getByTestId("kanban-column-sentinel")).toBeInTheDocument();
      auditObservers(home);

      // Scroll to end: the sentinel nears the viewport and the column
      // asks for its cursor page.
      act(() => sentinelObserver().callback([{ isIntersecting: true }]));
      await waitFor(() => expect(script.calls).toHaveLength(2));
      expect(new URL(script.calls[1]!).searchParams.get("cursor")).toBe("30:1:0");

      // Page two merges: 32 shells, and the end of the column renders
      // the last card's name once its shell nears the viewport.
      await waitFor(() => expect(within(home).getAllByTestId("kanban-card")).toHaveLength(32));
      auditObservers(home);
      fireRow(home, 30, true);
      fireRow(home, 31, true);
      auditObservers(home);
      expect(columnCards(home)).toContain("KB page 32");
      expect(within(home).queryByTestId("kanban-column-sentinel")).not.toBeInTheDocument();
    }
  );
});

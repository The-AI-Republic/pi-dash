// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Firing calendar composition (parity/specs/agents.md §5, AGT-015–017,
// 062): opens on the week view, steps and jumps home, refetches the next
// window, switches to the month grid, filters through the rail and
// persists that choice, shows the empty and truncation states, opens and
// closes the occurrence panel, and hands admins the shared edit dialog.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Scheduler, SchedulerBinding, SchedulerOccurrence } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createMemoryStorage } from "../../testSupport/memoryStorage.js";
import { readHiddenSchedulers } from "./visibility.js";

const { hooks } = vi.hoisted(() => ({
  hooks: {
    bindings: { data: [] as SchedulerBinding[] | undefined, isSuccess: true, isPending: false, refetch: vi.fn() },
    schedulers: { data: [] as Scheduler[] | undefined, isSuccess: true },
    occurrences: {
      data: { occurrences: [] as SchedulerOccurrence[], has_more: false, next_window_start: null as string | null } as
        | { occurrences: SchedulerOccurrence[]; has_more: boolean; next_window_start: string | null }
        | undefined,
      isPending: false,
    },
    windows: [] as { from: string; to: string }[],
  },
}));

vi.mock("./installData.js", () => ({
  useSchedulerBindings: () => hooks.bindings,
}));
vi.mock("./api.js", () => ({
  useSchedulers: () => hooks.schedulers,
}));
vi.mock("./occurrences.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("./occurrences.js")>();
  return {
    ...original,
    useSchedulerOccurrences: (_slug: string, _project: string, window: { from: string; to: string }) => {
      hooks.windows.push(window);
      return hooks.occurrences;
    },
  };
});
vi.mock("./EditInstallDialog.js", () => ({
  EditInstallDialog: ({
    binding,
    onSaved,
    onClose,
  }: {
    binding: SchedulerBinding | null;
    onSaved: () => void;
    onClose: () => void;
  }) => (
    <div data-testid="edit-dialog">
      {binding === null ? "closed" : `editing ${binding.scheduler_name}`}
      <button type="button" onClick={onSaved}>
        Settle after edit
      </button>
      <button type="button" onClick={onClose}>
        Cancel edit
      </button>
    </div>
  ),
}));

import { CLOCK_TICK_MS, FiringCalendar } from "./FiringCalendar.js";

const SLUG = "acme";
const PROJECT = "p1";
const NOW = new Date(2026, 9, 21, 10, 0);

function binding(overrides: Partial<SchedulerBinding> = {}): SchedulerBinding {
  return {
    id: "b1",
    scheduler: "s1",
    scheduler_slug: "alpha",
    scheduler_name: "Alpha",
    scheduler_color: "#111111",
    project: PROJECT,
    workspace: "w1",
    dtstart: "2026-10-01T09:00:00Z",
    tzid: "UTC",
    rrule: "FREQ=DAILY",
    rdates: [],
    exdates: [],
    extra_context: "",
    enabled: true,
    outcome_mode: "create_issue",
    pod: null,
    pod_name: null,
    next_run_at: null,
    last_run: null,
    last_run_status: null,
    last_run_ended_at: null,
    last_error: "",
    actor: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    ...overrides,
  };
}

function occurrence(dtstart: string, overrides: Partial<SchedulerOccurrence> = {}): SchedulerOccurrence {
  return {
    binding_id: "b1",
    scheduler_id: "s1",
    scheduler_name: "Alpha",
    scheduler_color: "#111111",
    dtstart,
    tzid: "UTC",
    kind: "scheduled",
    agent_run_id: null,
    status: null,
    ...overrides,
  };
}

function setup(props: Partial<React.ComponentProps<typeof FiringCalendar>> = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <FiringCalendar workspaceSlug={SLUG} projectId={PROJECT} canEdit={false} now={NOW} {...props} />
    </QueryClientProvider>
  );
  return queryClient;
}

beforeEach(() => {
  hooks.bindings.data = [binding()];
  hooks.bindings.isSuccess = true;
  hooks.bindings.isPending = false;
  hooks.schedulers.data = [{ id: "s1" } as Scheduler];
  hooks.occurrences.data = { occurrences: [], has_more: false, next_window_start: null };
  hooks.occurrences.isPending = false;
  hooks.windows.length = 0;
  Object.defineProperty(window, "localStorage", { value: createMemoryStorage(), configurable: true, writable: true });
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("FiringCalendar", () => {
  it("opens on the week view for the week holding today", () => {
    setup();
    expect(screen.getByTestId("calendar-week-view")).toBeInTheDocument();
    expect(screen.getByText("October 2026")).toBeInTheDocument();
    const first = hooks.windows[0];
    expect(new Date(first?.from as string).getDate()).toBe(18);
    // Every read asks for the same window: the key stays stable across renders.
    expect(new Set(hooks.windows.map((window) => `${window.from}|${window.to}`)).size).toBe(1);
  });

  it("steps a week at a time and returns home", async () => {
    setup();
    await userEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(new Date(hooks.windows.at(-1)?.from as string).getDate()).toBe(25);
    await userEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(new Date(hooks.windows.at(-1)?.from as string).getDate()).toBe(18);
    await userEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(screen.getByText("October 2026")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Next" }));
    await userEvent.click(screen.getByRole("button", { name: "Today" }));
    expect(new Date(hooks.windows.at(-1)?.from as string).getDate()).toBe(18);
  });

  it("switches to the month grid, stepping by month", async () => {
    setup();
    await userEvent.click(screen.getByRole("button", { name: "Month" }));
    expect(screen.getByTestId("calendar-month-view")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByText("November 2026")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(screen.getByText("October 2026")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Week" }));
    expect(screen.getByTestId("calendar-week-view")).toBeInTheDocument();
  });

  it("shows the empty state when nothing is installed", () => {
    hooks.bindings.data = [];
    setup();
    expect(screen.getByRole("heading", { name: "No schedulers installed" })).toBeInTheDocument();
  });

  it("shows the truncation hint when the window was capped", () => {
    hooks.occurrences.data = { occurrences: [], has_more: true, next_window_start: null };
    setup();
    expect(screen.getByText("Too many occurrences in this window")).toBeInTheDocument();
  });

  it("filters the grid through the rail and persists the choice", async () => {
    hooks.bindings.data = [binding(), binding({ id: "b2", scheduler: "s2", scheduler_name: "Beta" })];
    hooks.schedulers.data = [{ id: "s1" } as Scheduler, { id: "s2" } as Scheduler];
    hooks.occurrences.data = {
      occurrences: [
        occurrence(new Date(2026, 9, 22, 9, 0).toISOString()),
        occurrence(new Date(2026, 9, 22, 10, 0).toISOString(), {
          binding_id: "b2",
          scheduler_id: "s2",
          scheduler_name: "Beta",
        }),
      ],
      has_more: false,
      next_window_start: null,
    };
    setup();
    expect(screen.getAllByTitle(/Alpha|Beta/)).toHaveLength(2);
    await userEvent.click(screen.getByRole("checkbox", { name: "Alpha" }));
    expect(screen.getAllByTitle(/Alpha|Beta/)).toHaveLength(1);
    expect(readHiddenSchedulers(window.localStorage, SLUG, PROJECT)).toEqual(["s1"]);
    await userEvent.click(screen.getByRole("button", { name: "Hide all" }));
    expect(screen.queryAllByTitle(/Alpha|Beta/)).toHaveLength(0);
    await userEvent.click(screen.getByRole("button", { name: "Show all" }));
    expect(screen.getAllByTitle(/Alpha|Beta/)).toHaveLength(2);
  });

  it("keeps the rail choice independent when the project changes in place", async () => {
    hooks.bindings.data = [binding(), binding({ id: "b2", scheduler: "s2", scheduler_name: "Beta" })];
    hooks.schedulers.data = [{ id: "s1" } as Scheduler, { id: "s2" } as Scheduler];
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <FiringCalendar workspaceSlug={SLUG} projectId="p1" canEdit={false} now={NOW} />
      </QueryClientProvider>
    );
    await userEvent.click(screen.getByRole("checkbox", { name: "Beta" }));
    expect(readHiddenSchedulers(window.localStorage, SLUG, "p1")).toEqual(["s2"]);

    rerender(
      <QueryClientProvider client={queryClient}>
        <FiringCalendar workspaceSlug={SLUG} projectId="p2" canEdit={false} now={NOW} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("checkbox", { name: "Alpha" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Beta" })).toBeChecked();
    expect(readHiddenSchedulers(window.localStorage, SLUG, "p2")).toEqual([]);
    expect(readHiddenSchedulers(window.localStorage, SLUG, "p1")).toEqual(["s2"]);
  });

  it("opens the occurrence panel from a block and closes it", async () => {
    hooks.occurrences.data = {
      occurrences: [occurrence(new Date(2026, 9, 22, 9, 0).toISOString())],
      has_more: false,
      next_window_start: null,
    };
    setup();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await userEvent.click(screen.getByTitle(/Alpha/));
    const panel = screen.getByRole("dialog");
    expect(within(panel).getByRole("heading", { level: 2 })).toHaveTextContent("Alpha");
    expect(within(panel).getByRole("link", { name: /View scheduler/ })).toHaveAttribute(
      "href",
      "/acme/projects/p1/schedulers/b1"
    );
    await userEvent.click(within(panel.querySelector("header") as HTMLElement).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("drops the selection when the view changes", async () => {
    hooks.occurrences.data = {
      occurrences: [occurrence(new Date(2026, 9, 22, 9, 0).toISOString())],
      has_more: false,
      next_window_start: null,
    };
    setup();
    await userEvent.click(screen.getByTitle(/Alpha/));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Month" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("opens the shared edit dialog from the panel without leaving it behind for admins", async () => {
    hooks.occurrences.data = {
      occurrences: [occurrence(new Date(2026, 9, 22, 9, 0).toISOString())],
      has_more: false,
      next_window_start: null,
    };
    setup({ canEdit: true });
    await userEvent.click(screen.getByTitle(/Alpha/));
    await userEvent.click(screen.getByRole("button", { name: "Edit binding" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("edit-dialog")).toHaveTextContent("editing Alpha"));
  });

  it("re-reads occurrences and installs after a successful edit", async () => {
    hooks.occurrences.data = {
      occurrences: [occurrence(new Date(2026, 9, 22, 9, 0).toISOString())],
      has_more: false,
      next_window_start: null,
    };
    setup({ canEdit: true });
    await userEvent.click(screen.getByTitle(/Alpha/));
    await userEvent.click(screen.getByRole("button", { name: "Edit binding" }));
    await userEvent.click(await screen.findByRole("button", { name: "Settle after edit" }));
    await waitFor(() => expect(hooks.bindings.refetch).toHaveBeenCalled());
  });

  it("re-reads the clock on an interval when not pinned", () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(NOW);
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      render(
        <QueryClientProvider client={queryClient}>
          <FiringCalendar workspaceSlug={SLUG} projectId={PROJECT} canEdit={false} />
        </QueryClientProvider>
      );
      expect(screen.getByRole("button", { name: "Today" })).toBeInTheDocument();
      act(() => {
        vi.advanceTimersByTime(CLOCK_TICK_MS);
      });
      // Still today: the interval only refreshes the clock, it does not move the anchor.
      expect(screen.getByRole("button", { name: "Today" })).toBeInTheDocument();
      act(() => {
        vi.advanceTimersByTime(CLOCK_TICK_MS);
      });
      expect(screen.getByTestId("calendar-week-view")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("closes the edit dialog through its own close control", async () => {
    hooks.occurrences.data = {
      occurrences: [occurrence(new Date(2026, 9, 22, 9, 0).toISOString())],
      has_more: false,
      next_window_start: null,
    };
    setup({ canEdit: true });
    await userEvent.click(screen.getByTitle(/Alpha/));
    await userEvent.click(screen.getByRole("button", { name: "Edit binding" }));
    await userEvent.click(await screen.findByRole("button", { name: "Cancel edit" }));
    expect(screen.queryByTestId("edit-dialog")).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("opens the panel read-only when the occurrence's install is gone", async () => {
    hooks.bindings.data = [];
    hooks.bindings.isSuccess = false;
    hooks.occurrences.data = {
      occurrences: [occurrence(new Date(2026, 9, 22, 9, 0).toISOString())],
      has_more: false,
      next_window_start: null,
    };
    setup({ canEdit: true });
    await userEvent.click(screen.getByTitle(/Alpha/));
    const panel = screen.getByRole("dialog");
    expect(within(panel).getByRole("heading", { level: 2 })).toHaveTextContent("Alpha");
    expect(within(panel).queryByRole("button", { name: "Edit binding" })).not.toBeInTheDocument();
    expect(within(panel).getByText("No extra context")).toBeInTheDocument();
  });

  it("keeps Edit binding away from members", async () => {
    hooks.occurrences.data = {
      occurrences: [occurrence(new Date(2026, 9, 22, 9, 0).toISOString())],
      has_more: false,
      next_window_start: null,
    };
    setup({ canEdit: false });
    await userEvent.click(screen.getByTitle(/Alpha/));
    expect(screen.queryByRole("button", { name: "Edit binding" })).not.toBeInTheDocument();
  });

  it("draws an empty grid while the reads have no data yet", () => {
    hooks.bindings.data = undefined;
    hooks.bindings.isSuccess = false;
    hooks.schedulers.data = undefined;
    hooks.occurrences.data = undefined;
    setup();
    expect(screen.getByTestId("calendar-week-view")).toBeInTheDocument();
    expect(screen.queryAllByTitle(/Alpha/)).toHaveLength(0);
    expect(screen.queryByText("Too many occurrences in this window")).not.toBeInTheDocument();
  });

  it("shows a loading surface until the first reads settle", () => {
    hooks.occurrences.isPending = true;
    hooks.bindings.isPending = true;
    hooks.bindings.isSuccess = false;
    setup();
    expect(screen.getByRole("status", { name: "Loading calendar" })).toBeInTheDocument();
  });
});

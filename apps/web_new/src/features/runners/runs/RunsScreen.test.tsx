// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs screen container (NEWFRONT-545). Page navigation replaces history
// and drops page one's query; selection keeps the page; cancel toasts on
// failure; the pane state derives from the detail query.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const navigate = vi.fn(async () => undefined);
const notifyError = vi.fn();
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return { ...original, useNavigate: () => navigate };
});
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: (title: string, description?: string) => notifyError(title, description) };
});
vi.mock("../area.js", () => ({
  useRunnersArea: () => ({
    scope: { workspaceSlug: "acme" },
    workspaceId: "ws-id",
    access: "allowed",
    allowed: true,
    base: "/acme/runners",
    path: (child?: string) => (child === undefined ? "/acme/runners" : `/acme/runners/${child}`),
  }),
}));
const queryMocks = vi.hoisted(() => ({
  useRuns: vi.fn(),
  useRun: vi.fn(),
  useCancelRun: vi.fn(),
}));
vi.mock("../api/runs.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../api/runs.js")>();
  return { ...original, ...queryMocks };
});

const { schedulerBindingHref, RunsScreen } = await import("./RunsScreen.js");

function stubQueries(
  overrides: { list?: Record<string, unknown>; detail?: Record<string, unknown>; cancel?: Record<string, unknown> } = {}
) {
  queryMocks.useRuns.mockReturnValue({
    data: { results: [], count: 0, total_count: 0, total_pages: 1, page: 1, per_page: 30 },
    isPending: false,
    ...overrides.list,
  });
  queryMocks.useRun.mockReturnValue({ data: undefined, isError: false, ...overrides.detail });
  queryMocks.useCancelRun.mockReturnValue({
    mutateAsync: vi.fn(async () => ({})),
    isPending: false,
    ...overrides.cancel,
  });
}

function renderScreen(page = 2, selectedRunId: string | null = null) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RunsScreen search={{ page }} selectedRunId={selectedRunId} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  navigate.mockClear();
  notifyError.mockClear();
  stubQueries();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("schedulerBindingHref", () => {
  it("builds the project scheduler path only when binding and detail resolve", () => {
    expect(
      schedulerBindingHref({
        scheduler_binding: "b-1",
        scheduler_binding_detail: { project: "p-1" },
      } as never)
    ).toBe("/projects/p-1/schedulers/b-1");
    expect(
      schedulerBindingHref({ scheduler_binding: null, scheduler_binding_detail: { project: "p-1" } } as never)
    ).toBeNull();
    expect(schedulerBindingHref({ scheduler_binding: "b-1", scheduler_binding_detail: null } as never)).toBeNull();
  });
});

const PAGE_RESULT = {
  id: "run-1",
  status: "running",
  prompt: "hello",
  started_at: null,
  created_at: "2024-01-01T00:00:00Z",
};

describe("page navigation", () => {
  it("replaces history and keeps later pages in the query", async () => {
    stubQueries({ list: { data: { results: [PAGE_RESULT], total_count: 31, total_pages: 3, page: 2 } } });
    renderScreen(2);
    const next = screen.getByRole("button", { name: "Next" });
    await waitFor(() => expect(next).toBeEnabled());
    next.click();
    expect(navigate).toHaveBeenCalledWith({ href: "/acme/runners/runs?page=3", replace: true });
  });

  it("drops the query on page one", () => {
    stubQueries({ list: { data: { results: [PAGE_RESULT], total_count: 31, total_pages: 2, page: 2 } } });
    renderScreen(2);
    screen.getByRole("button", { name: "Previous" }).click();
    expect(navigate).toHaveBeenCalledWith({ href: "/acme/runners/runs", replace: true });
  });
});

describe("selection", () => {
  it("opens the run and preserves the current page", async () => {
    stubQueries({
      list: {
        data: {
          results: [
            { id: "run-1", status: "running", prompt: "hello", started_at: null, created_at: "2024-01-01T00:00:00Z" },
          ],
          total_count: 1,
          total_pages: 1,
        },
      },
    });
    renderScreen(2);
    screen.getByText("hello").click();
    expect(navigate).toHaveBeenCalledWith({ href: "/acme/runners/runs/run-1?page=2", replace: true });
  });

  it("omits the query when selecting from page one", () => {
    stubQueries({
      list: {
        data: {
          results: [
            { id: "run-1", status: "running", prompt: "hello", started_at: null, created_at: "2024-01-01T00:00:00Z" },
          ],
          total_count: 1,
          total_pages: 1,
        },
      },
    });
    renderScreen(1);
    screen.getByText("hello").click();
    expect(navigate).toHaveBeenCalledWith({ href: "/acme/runners/runs/run-1", replace: true });
  });
});

describe("detail states", () => {
  it("shows none without a selection", () => {
    renderScreen(1, null);
    expect(screen.getByText("Select a run on the left.")).toBeInTheDocument();
  });

  it("shows loading while the detail has no data and no error", () => {
    renderScreen(1, "run-1");
    expect(screen.getByText("Loading run details…")).toBeInTheDocument();
  });

  it("shows unavailable after a failed read", () => {
    stubQueries({ detail: { data: undefined, isError: true } });
    renderScreen(1, "run-1");
    expect(screen.getByText(/not available/)).toBeInTheDocument();
  });

  it("shows the loaded run with the scheduler link", () => {
    stubQueries({
      detail: {
        data: {
          id: "11111111-2222-3333-4444-555555555555",
          status: "running",
          executor_kind: "local_runner",
          prompt: "p",
          error: "",
          error_diagnostic: null,
          done_payload: null,
          tool_plan: {},
          tool_calls: [],
          events: [],
          scheduler_binding: "b-1",
          scheduler_binding_detail: { project: "p-1", scheduler_name: "Nightly" },
        },
      },
    });
    renderScreen(1, "11111111-2222-3333-4444-555555555555");
    expect(screen.getByTestId("run-detail")).toBeInTheDocument();
    expect(screen.getByTestId("run-scheduler-link")).toHaveAttribute("href", "/acme/projects/p-1/schedulers/b-1");
  });
});

describe("cancel failure", () => {
  it("toasts the failure", async () => {
    const mutateAsync = vi.fn(async () => {
      throw new Error("nope");
    });
    stubQueries({
      detail: {
        data: {
          id: "11111111-2222-3333-4444-555555555555",
          status: "running",
          executor_kind: "local_runner",
          prompt: "p",
          error: "",
          error_diagnostic: null,
          done_payload: null,
          tool_plan: {},
          tool_calls: [],
          events: [],
          scheduler_binding: null,
          scheduler_binding_detail: null,
        },
      },
      cancel: { mutateAsync, isPending: false },
    });
    renderScreen(1, "11111111-2222-3333-4444-555555555555");
    screen.getByTestId("run-cancel").click();
    const confirm = await screen.findByTestId("run-cancel-confirm");
    confirm.click();
    expect(mutateAsync).toHaveBeenCalledWith({ runId: "11111111-2222-3333-4444-555555555555", reason: "user" });
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't cancel run", "The run is still active. Try again.")
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});

describe("cancel success", () => {
  it("closes the confirmation after the server accepts", async () => {
    const mutateAsync = vi.fn(async () => ({}));
    stubQueries({
      detail: {
        data: {
          id: "11111111-2222-3333-4444-555555555555",
          status: "running",
          executor_kind: "local_runner",
          prompt: "p",
          error: "",
          error_diagnostic: null,
          done_payload: null,
          tool_plan: {},
          tool_calls: [],
          events: [],
          scheduler_binding: null,
          scheduler_binding_detail: null,
        },
      },
      cancel: { mutateAsync, isPending: false },
    });
    renderScreen(1, "11111111-2222-3333-4444-555555555555");
    screen.getByTestId("run-cancel").click();
    const confirm = await screen.findByTestId("run-cancel-confirm");
    confirm.click();
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(mutateAsync).toHaveBeenCalledWith({ runId: "11111111-2222-3333-4444-555555555555", reason: "user" });
  });
});

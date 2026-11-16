// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loop jobs list page: header, loading skeleton, jobs table with
// inline toggles, inline empty paragraph, and the load-failure retry.
// Toggle writes run through the real app client against a stub
// transport so the PATCH bodies and the notices are pinned here.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { LoopJobList, Transport } from "@pidash/api-client";
import { ToastHost } from "@pidash/kit";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { getToastManager } from "../../../shared/shell/index.js";
import {
  AdminLoopJobsPage,
  LOOP_BUILTIN_NO,
  LOOP_BUILTIN_YES,
  LOOP_JOBS_DESCRIPTION,
  LOOP_JOBS_HEADING,
  LOOP_LIST_HEADERS,
} from "./AdminLoopJobsPage.js";
import { LOOP_LIST_EMPTY_TEXT } from "../loopJobs.js";

function loopJob(overrides: Partial<LoopJobList[number]> = {}): LoopJobList[number] {
  return {
    id: "33333333-3333-4333-8333-333333333333",
    slug: "probe-job",
    name: "Probe job",
    public_name: "Probe public",
    public_description: "Probe description",
    prompt: "Probe prompt",
    min_role: 15,
    enabled: true,
    is_builtin: false,
    dtstart: null,
    rrule: "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
    tzid: "UTC",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

interface PatchCall {
  id: string;
  body: unknown;
}

interface LoopHarness {
  patches: PatchCall[];
  failList: () => void;
  allowList: () => void;
}

async function renderPage(options: {
  jobs: LoopJobList;
  listFails?: boolean;
  listGate?: Promise<void>;
  onPatch?: (id: string, body: unknown, jobs: LoopJobList) => Promise<{ status: number; body: unknown }>;
}): Promise<LoopHarness> {
  const patches: PatchCall[] = [];
  let listFails = options.listFails ?? false;
  const listGate = options.listGate ?? Promise.resolve();
  // Mutable server state: the default PATCH applies like the backend, so
  // settled revalidation reads the write back.
  const serverJobs: LoopJobList = options.jobs.map((job) => ({ ...job }));
  const onPatch =
    options.onPatch ??
    (async (id: string, body: unknown) => {
      const job = serverJobs.find((candidate) => candidate.id === id);
      const updated = { ...job, ...(body as Record<string, unknown>) };
      const index = serverJobs.findIndex((candidate) => candidate.id === id);
      if (index !== -1) serverJobs[index] = updated as LoopJobList[number];
      return { status: 200, body: updated };
    });
  const transport: Transport = async (url, init) => {
    const pathname = new URL(url, "https://api.test").pathname;
    if (init.method === "GET" && pathname === "/api/instances/loop/jobs/") {
      await listGate;
      if (listFails) {
        return {
          status: 500,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ detail: "boom" })),
        };
      }
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(serverJobs)),
      };
    }
    const patchMatch = /^\/api\/instances\/loop\/jobs\/([^/]+)\/$/.exec(pathname);
    if (init.method === "PATCH" && patchMatch?.[1] !== undefined) {
      const id = patchMatch[1];
      const body = init.body === undefined ? undefined : (JSON.parse(init.body) as unknown);
      patches.push({ id, body });
      const result = await onPatch(id, body, serverJobs);
      return {
        status: result.status,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(result.body)),
      };
    }
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify({ csrf_token: "csrf-1" })),
    };
  };
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const rootRoute = createRootRoute({
    component: () => (
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <AdminLoopJobsPage />
      </QueryClientProvider>
    ),
  });
  const consoleRoute = createRoute({ getParentRoute: () => rootRoute, path: "/god-mode/$" });
  const router = createRouter({
    routeTree: rootRoute.addChildren([consoleRoute]),
    history: createMemoryHistory({ initialEntries: ["/god-mode/loop/"] }),
    trailingSlash: "preserve",
  });
  render(<RouterProvider router={router} />);
  await screen.findByTestId("loop-jobs-page");
  return {
    patches,
    failList: () => {
      listFails = true;
    },
    allowList: () => {
      listFails = false;
    },
  };
}

afterEach(() => {
  resetAppClient();
});

describe("AdminLoopJobsPage", () => {
  it("shows the header while the list loads", async () => {
    let release!: () => void;
    const listGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    await renderPage({ jobs: [loopJob()], listGate });
    expect(screen.getByRole("heading", { name: LOOP_JOBS_HEADING })).toBeInTheDocument();
    expect(screen.getByText(LOOP_JOBS_DESCRIPTION)).toBeInTheDocument();
    release();
    await screen.findByTestId("loop-jobs-table");
  });

  it("opens the create modal from the header action and cancels cleanly (ADM-055)", async () => {
    const user = userEvent.setup();
    const harness = await renderPage({ jobs: [loopJob()] });
    await screen.findByTestId("loop-jobs-table");
    await user.click(screen.getByRole("button", { name: "New job" }));
    expect(await screen.findByTestId("loop-job-modal-form")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "New job" })).toBeInTheDocument();
    await user.type(screen.getByLabelText("Slug"), "probe-cancel");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByTestId("loop-job-modal-form")).not.toBeInTheDocument());
    expect(harness.patches).toEqual([]);
  });

  it("shows two 48px skeleton bars with no table while loading (ADM-054)", async () => {
    let release!: () => void;
    const listGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    await renderPage({ jobs: [loopJob()], listGate });
    const skeleton = screen.getByTestId("loop-jobs-skeleton");
    expect(skeleton.getAttribute("role")).toBe("status");
    const bars = skeleton.querySelectorAll(":scope > *");
    expect(bars).toHaveLength(2);
    expect((bars[0] as HTMLElement).style.height).toBe("48px");
    expect((bars[1] as HTMLElement).style.height).toBe("48px");
    expect(screen.queryByTestId("loop-jobs-table")).not.toBeInTheDocument();
    expect(screen.queryByTestId("loop-jobs-empty")).not.toBeInTheDocument();
    release();
    await screen.findByTestId("loop-jobs-table");
  });

  it("renders every job with the pinned columns matching the server (ADM-051)", async () => {
    const jobs = [
      loopJob({
        id: "11111111-1111-4111-8111-111111111111",
        name: "Probe one",
        slug: "probe-one",
        min_role: 20,
        is_builtin: true,
        enabled: true,
      }),
      loopJob({
        id: "22222222-2222-4222-8222-222222222222",
        name: "Probe two",
        slug: "probe-two",
        min_role: 5,
        is_builtin: false,
        enabled: false,
        rrule: "FREQ=WEEKLY;BYDAY=MO",
      }),
      loopJob({
        id: "33333333-3333-4333-8333-333333333333",
        name: "Probe three",
        slug: "probe-three",
        min_role: 10,
        enabled: true,
      }),
    ];
    await renderPage({ jobs });
    const table = await screen.findByTestId("loop-jobs-table");
    expect(screen.queryByTestId("loop-jobs-skeleton")).not.toBeInTheDocument();
    expect(screen.queryByTestId("loop-jobs-empty")).not.toBeInTheDocument();
    expect(LOOP_LIST_HEADERS).toEqual(["Name", "Slug", "Recurrence", "Min role", "Builtin", "Enabled"]);
    const headers = within(table).getAllByRole("columnheader");
    expect(headers.map((cell) => cell.textContent)).toEqual([
      "Name",
      "Slug",
      "Recurrence",
      "Min role",
      "Builtin",
      "Enabled",
    ]);
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(4);
    const first = within(rows[1] as HTMLElement);
    expect(first.getByRole("link", { name: "Probe one" }).getAttribute("href")).toBe(
      "/god-mode/loop/11111111-1111-4111-8111-111111111111"
    );
    expect(first.getByText("probe-one")).toBeInTheDocument();
    expect(first.getByText("FREQ=DAILY;BYHOUR=3;BYMINUTE=0")).toBeInTheDocument();
    expect(first.getByText("Admin")).toBeInTheDocument();
    expect(first.getByText(LOOP_BUILTIN_YES)).toBeInTheDocument();
    expect(first.getByRole("switch", { name: "Probe one enabled" }).getAttribute("aria-checked")).toBe("true");
    const second = within(rows[2] as HTMLElement);
    expect(second.getByRole("link", { name: "Probe two" }).getAttribute("href")).toBe(
      "/god-mode/loop/22222222-2222-4222-8222-222222222222"
    );
    expect(second.getByText("Guest")).toBeInTheDocument();
    expect(second.getByText(LOOP_BUILTIN_NO)).toBeInTheDocument();
    expect(second.getByRole("switch", { name: "Probe two enabled" }).getAttribute("aria-checked")).toBe("false");
    const third = within(rows[3] as HTMLElement);
    expect(third.getByText("10")).toBeInTheDocument();
    expect(third.getByRole("switch", { name: "Probe three enabled" }).getAttribute("aria-checked")).toBe("true");
  });

  it("persists a toggle with an enable-flag write and no notice (ADM-052)", async () => {
    const user = userEvent.setup();
    const jobs = [loopJob({ id: "11111111-1111-4111-8111-111111111111", enabled: false })];
    const harness = await renderPage({ jobs });
    const toggle = await screen.findByRole("switch", { name: "Probe job enabled" });
    await user.click(toggle);
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Probe job enabled" }).getAttribute("aria-checked")).toBe("true")
    );
    expect(harness.patches).toEqual([{ id: "11111111-1111-4111-8111-111111111111", body: { enabled: true } }]);
    await waitFor(() => expect(screen.queryByTestId("toast-notification")).not.toBeInTheDocument());
  });

  it("flips the toggle before the write lands (ADM-052 optimistic)", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const jobs = [loopJob({ id: "11111111-1111-4111-8111-111111111111", enabled: false })];
    await renderPage({
      jobs,
      onPatch: async (id, body, current) => {
        await gate;
        const index = current.findIndex((candidate) => candidate.id === id);
        if (index === -1) return { status: 404, body: { detail: "gone" } };
        const updated = { ...current[index], ...(body as Record<string, unknown>) };
        current[index] = updated as LoopJobList[number];
        return { status: 200, body: updated };
      },
    });
    const toggle = await screen.findByRole("switch", { name: "Probe job enabled" });
    await user.click(toggle);
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Probe job enabled" }).getAttribute("aria-checked")).toBe("true")
    );
    release();
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Probe job enabled" }).getAttribute("aria-checked")).toBe("true")
    );
  });

  it("shows the inline creation prompt with no table when empty (ADM-053)", async () => {
    await renderPage({ jobs: [] });
    const empty = await screen.findByTestId("loop-jobs-empty");
    expect(empty.tagName).toBe("P");
    expect(empty).toHaveTextContent(LOOP_LIST_EMPTY_TEXT);
    expect(LOOP_LIST_EMPTY_TEXT).toBe("No jobs yet. Create one to get started.");
    expect(screen.queryByTestId("loop-jobs-table")).not.toBeInTheDocument();
    expect(screen.queryByTestId("loop-jobs-skeleton")).not.toBeInTheDocument();
  });

  it("retries the list after a load failure", async () => {
    const user = userEvent.setup();
    const harness = await renderPage({ jobs: [loopJob()], listFails: true });
    expect(await screen.findByTestId("loop-jobs-error")).toBeInTheDocument();
    expect(screen.queryByTestId("loop-jobs-table")).not.toBeInTheDocument();
    harness.allowList();
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByTestId("loop-jobs-table")).toBeInTheDocument();
    expect(screen.queryByTestId("loop-jobs-error")).not.toBeInTheDocument();
  });

  // Last: the failure notice lingers in the shared toast manager, so no
  // later test may assert the absence of toasts.
  it("rolls back with an error notice when the write fails (ADM-052)", async () => {
    const user = userEvent.setup();
    const jobs = [loopJob({ id: "11111111-1111-4111-8111-111111111111", enabled: false })];
    const harness = await renderPage({
      jobs,
      onPatch: async () => ({ status: 500, body: { detail: "boom" } }),
    });
    const toggle = await screen.findByRole("switch", { name: "Probe job enabled" });
    await user.click(toggle);
    expect(await screen.findByText("Update failed")).toBeInTheDocument();
    expect(screen.getByText("Could not change the job state.")).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Probe job enabled" }).getAttribute("aria-checked")).toBe("false")
    );
    expect(harness.patches).toEqual([{ id: "11111111-1111-4111-8111-111111111111", body: { enabled: true } }]);
  });
});

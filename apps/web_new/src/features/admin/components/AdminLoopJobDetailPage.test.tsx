// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loop job detail page: loading skeleton, header with edit/delete
// actions, stat tiles, skip-reason filter, targets table with its
// empty placeholder, load-failure retries, and the confirmation-free
// delete. Reads and writes run through the real app client against a
// stub transport so request URLs, bodies, and notices are pinned here.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { LoopJobDetail, LoopTarget, Transport } from "@pidash/api-client";
import { ToastHost } from "@pidash/kit";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { getToastManager } from "../../../shared/shell/index.js";
import { adminKeys } from "../queries.js";
import {
  LOOP_JOB_DELETED_MESSAGE,
  LOOP_JOB_DELETED_TITLE,
  LOOP_JOB_DELETE_FAILED_MESSAGE,
  LOOP_JOB_DELETE_FAILED_TITLE,
  LOOP_SKIP_FILTER_OPTIONS,
  LOOP_TARGETS_EMPTY_TEXT,
  LOOP_TARGETS_HEADERS,
} from "../loopJobDetail.js";
import { AdminLoopJobDetailPage } from "./AdminLoopJobDetailPage.js";

const JOB_ID = "33333333-3333-4333-8333-333333333333";
const JOB_PATH = `/api/instances/loop/jobs/${JOB_ID}/`;
const TARGETS_PATH = `/api/instances/loop/jobs/${JOB_ID}/targets/`;

function detailJob(overrides: Partial<LoopJobDetail> = {}): LoopJobDetail {
  return {
    id: JOB_ID,
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
    stats: { target_count: 0, completed: 0, failed: 0, skipped: 0 },
    ...overrides,
  };
}

function loopTarget(overrides: Partial<LoopTarget> = {}): LoopTarget {
  return {
    id: "44444444-4444-4434-8434-444444444444",
    workspace_slug: "probe-workspace",
    user_email: "member@example.test",
    next_run_at: "2026-02-03T04:05:06Z",
    last_skipped_at: null,
    last_skip_reason: "",
    last_run: null,
    ...overrides,
  };
}

interface DetailHarness {
  deletes: string[];
  patches: unknown[];
  targetUrls: string[];
  pathname: () => string;
  serverJob: () => LoopJobDetail | null;
  failJob: () => void;
  allowJob: () => void;
  failTargets: () => void;
  allowTargets: () => void;
  failDelete: () => void;
}

async function renderPage(options: {
  job?: LoopJobDetail | null;
  seedDetail?: unknown;
  targets?: LoopTarget[];
  jobGate?: Promise<void>;
  targetsGate?: Promise<void>;
  deleteGate?: Promise<void>;
  jobFails?: boolean;
  targetsFail?: boolean;
  deleteFails?: boolean;
}): Promise<DetailHarness> {
  const deletes: string[] = [];
  const patches: unknown[] = [];
  const targetUrls: string[] = [];
  let serverJob: LoopJobDetail | null = options.job === undefined ? detailJob() : options.job;
  const serverTargets: LoopTarget[] = (options.targets ?? []).map((target) => ({ ...target }));
  let jobFails = options.jobFails ?? false;
  let targetsFail = options.targetsFail ?? false;
  let deleteFails = options.deleteFails ?? false;
  const jobGate = options.jobGate ?? Promise.resolve();
  const targetsGate = options.targetsGate ?? Promise.resolve();
  const deleteGate = options.deleteGate ?? Promise.resolve();
  const transport: Transport = async (url, init) => {
    const parsed = new URL(url, "https://api.test");
    if (init.method === "GET" && parsed.pathname === JOB_PATH) {
      await jobGate;
      if (jobFails || serverJob === null) {
        return {
          status: jobFails ? 500 : 404,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ detail: "boom" })),
        };
      }
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(serverJob)),
      };
    }
    if (init.method === "GET" && parsed.pathname === TARGETS_PATH) {
      targetUrls.push(url);
      await targetsGate;
      if (targetsFail) {
        return {
          status: 500,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ detail: "boom" })),
        };
      }
      const reason = parsed.searchParams.get("skip_reason");
      const results =
        reason === null || reason === ""
          ? serverTargets
          : serverTargets.filter((target) => target.last_skip_reason === reason);
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ page: 1, results })),
      };
    }
    if (init.method === "DELETE" && parsed.pathname === JOB_PATH) {
      deletes.push(url);
      await deleteGate;
      if (deleteFails) {
        return {
          status: 500,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ detail: "denied" })),
        };
      }
      serverJob = null;
      return {
        status: 204,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(""),
      };
    }
    if (init.method === "PATCH" && parsed.pathname === JOB_PATH && serverJob !== null) {
      const body = (init.body === undefined ? {} : JSON.parse(init.body)) as Record<string, unknown>;
      patches.push(body);
      serverJob = { ...serverJob, ...body };
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(serverJob)),
      };
    }
    return {
      status: 404,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
    };
  };
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.seedDetail !== undefined) {
    queryClient.setQueryData(adminKeys.loopJobDetail(JOB_ID), options.seedDetail);
  }
  const rootRoute = createRootRoute({
    component: () => (
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <AdminLoopJobDetailPage jobId={JOB_ID} />
      </QueryClientProvider>
    ),
  });
  const consoleRoute = createRoute({ getParentRoute: () => rootRoute, path: "/god-mode/$" });
  const router = createRouter({
    routeTree: rootRoute.addChildren([consoleRoute]),
    history: createMemoryHistory({ initialEntries: [`/god-mode/loop/${JOB_ID}/`] }),
    trailingSlash: "preserve",
  });
  render(<RouterProvider router={router} />);
  await screen.findByTestId("loop-job-detail-page");
  return {
    deletes,
    patches,
    targetUrls,
    pathname: () => router.state.location.pathname,
    serverJob: () => serverJob,
    failJob: () => {
      jobFails = true;
    },
    allowJob: () => {
      jobFails = false;
    },
    failTargets: () => {
      targetsFail = true;
    },
    allowTargets: () => {
      targetsFail = false;
    },
    failDelete: () => {
      deleteFails = true;
    },
  };
}

/** Data rows under the targets table header. */
function targetRows(): HTMLElement[] {
  const table = screen.getByTestId("loop-job-detail-targets");
  return within(table).getAllByRole("row").slice(1);
}

afterEach(() => {
  resetAppClient();
});

describe("AdminLoopJobDetailPage", () => {
  it("shows 48px/120px skeleton bars under the placeholder header until the job loads (ADM-061)", async () => {
    let release!: () => void;
    const jobGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    await renderPage({ jobGate });
    expect(screen.getByTestId("loop-job-detail-title")).toHaveTextContent("Loop job");
    const skeleton = screen.getByTestId("loop-job-detail-skeleton");
    expect(skeleton.getAttribute("role")).toBe("status");
    const bars = skeleton.querySelectorAll(":scope > *");
    expect(bars).toHaveLength(2);
    expect((bars[0] as HTMLElement).style.height).toBe("48px");
    expect((bars[1] as HTMLElement).style.height).toBe("120px");
    expect(screen.queryByTestId("loop-job-detail-edit")).not.toBeInTheDocument();
    expect(screen.queryByTestId("loop-job-detail-delete")).not.toBeInTheDocument();
    expect(screen.queryByTestId("loop-job-detail-targets")).not.toBeInTheDocument();
    release();
    await screen.findByTestId("loop-job-detail-targets");
    expect(screen.getByTestId("loop-job-detail-title")).toHaveTextContent("Probe job");
    expect(screen.queryByTestId("loop-job-detail-skeleton")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Edit" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete" })).toBeInTheDocument();
  });

  it("shows the job with stat tiles and populated targets (ADM-058)", async () => {
    await renderPage({
      job: detailJob({ stats: { target_count: 2, completed: 0, failed: 0, skipped: 1 } }),
      targets: [
        loopTarget({ user_email: "skipped@example.test", last_skip_reason: "user_disabled" }),
        loopTarget({
          id: "55555555-5555-5535-8535-555555555555",
          user_email: "fresh@example.test",
          next_run_at: null,
          last_run: {
            status: "completed",
            error_code: "",
            model_used: "probe-model",
            total_tokens: 42,
            completed_at: "2026-02-03T04:05:06Z",
          },
        }),
      ],
    });
    await screen.findByTestId("loop-job-detail-targets");
    expect(screen.getByTestId("loop-job-detail-title")).toHaveTextContent("Probe job");
    expect(screen.getByTestId("loop-job-detail-description")).toHaveTextContent("Probe description");
    expect(screen.getByTestId("loop-job-detail-stat-targets")).toHaveTextContent("2");
    expect(screen.getByTestId("loop-job-detail-stat-targets")).toHaveTextContent("Targets");
    expect(screen.getByTestId("loop-job-detail-stat-completed")).toHaveTextContent("Completed (24h)");
    expect(screen.getByTestId("loop-job-detail-stat-failed")).toHaveTextContent("Failed (24h)");
    expect(screen.getByTestId("loop-job-detail-stat-skipped")).toHaveTextContent("1");
    const headers = within(screen.getByTestId("loop-job-detail-targets")).getAllByRole("columnheader");
    expect(headers.map((header) => header.textContent)).toEqual([...LOOP_TARGETS_HEADERS]);
    const rows = targetRows();
    expect(rows).toHaveLength(2);
    const skipped = within(rows[0] as HTMLElement).getAllByRole("cell");
    expect(skipped[0]?.textContent).toBe("probe-workspace");
    expect(skipped[1]?.textContent).toBe("skipped@example.test");
    expect(skipped[2]?.textContent).not.toBe("—");
    expect(skipped[3]?.textContent).toBe("—");
    expect(skipped[4]?.textContent).toBe("—");
    expect(skipped[5]?.textContent).toBe("user_disabled");
    const fresh = within(rows[1] as HTMLElement).getAllByRole("cell");
    expect(fresh[1]?.textContent).toBe("fresh@example.test");
    expect(fresh[2]?.textContent).toBe("—");
    expect(fresh[3]?.textContent).toBe("completed");
    expect(fresh[4]?.textContent).toBe("42");
    expect(fresh[5]?.textContent).toBe("—");
    expect(screen.queryByTestId("loop-job-detail-targets-empty")).not.toBeInTheDocument();
  });

  it("shows the placeholder row when the targets read resolves empty (ADM-058)", async () => {
    await renderPage({ targets: [] });
    await screen.findByTestId("loop-job-detail-targets");
    expect(targetRows()).toHaveLength(1);
    expect(screen.getByTestId("loop-job-detail-targets-empty")).toHaveTextContent(LOOP_TARGETS_EMPTY_TEXT);
  });

  it("shows headers with an empty body while the targets load", async () => {
    let release!: () => void;
    const targetsGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    await renderPage({ targets: [loopTarget()], targetsGate });
    await screen.findByTestId("loop-job-detail-targets");
    expect(targetRows()).toHaveLength(0);
    expect(screen.queryByTestId("loop-job-detail-targets-empty")).not.toBeInTheDocument();
    release();
    await waitFor(() => expect(targetRows()).toHaveLength(1));
  });

  it("falls back to the detail description when the job carries none (ADM-058)", async () => {
    await renderPage({ job: detailJob({ public_description: "" }) });
    await screen.findByTestId("loop-job-detail-targets");
    expect(screen.getByTestId("loop-job-detail-description")).toHaveTextContent("Loop job detail");
  });

  it("hides the stat tiles when the job carries no stats (ADM-058)", async () => {
    await renderPage({ seedDetail: { ...detailJob(), stats: undefined }, targets: [] });
    await screen.findByTestId("loop-job-detail-targets");
    expect(screen.getByTestId("loop-job-detail-title")).toHaveTextContent("Probe job");
    expect(screen.queryByTestId("loop-job-detail-stats")).not.toBeInTheDocument();
  });

  it("retries the job read after a load failure", async () => {
    const user = userEvent.setup();
    const harness = await renderPage({ jobFails: true });
    expect(await screen.findByTestId("loop-job-detail-error")).toHaveTextContent("The loop job failed to load.");
    harness.allowJob();
    await user.click(screen.getByRole("button", { name: "Retry" }));
    await screen.findByTestId("loop-job-detail-targets");
    expect(screen.getByTestId("loop-job-detail-title")).toHaveTextContent("Probe job");
  });

  it("retries the targets read after a load failure", async () => {
    const user = userEvent.setup();
    const harness = await renderPage({ targets: [loopTarget()], targetsFail: true });
    await screen.findByTestId("loop-job-detail-targets-error");
    expect(screen.getByTestId("loop-job-detail-targets-error")).toHaveTextContent("The targets failed to load.");
    expect(screen.getByTestId("loop-job-detail-title")).toHaveTextContent("Probe job");
    harness.allowTargets();
    await user.click(
      within(screen.getByTestId("loop-job-detail-targets-error")).getByRole("button", { name: "Retry" })
    );
    await screen.findByTestId("loop-job-detail-targets");
    await waitFor(() => expect(targetRows()).toHaveLength(1));
  });

  it("filters by skip reason with a refetch and restores all from the cache (ADM-059)", async () => {
    const user = userEvent.setup();
    const harness = await renderPage({
      targets: [
        loopTarget({ user_email: "skipped@example.test", last_skip_reason: "user_disabled" }),
        loopTarget({
          id: "55555555-5555-5535-8535-555555555555",
          user_email: "other@example.test",
          last_skip_reason: "min_role",
        }),
      ],
    });
    await screen.findByTestId("loop-job-detail-targets");
    await waitFor(() => expect(targetRows()).toHaveLength(2));
    const select = screen.getByTestId("loop-job-detail-skip-filter");
    expect(
      within(select)
        .getAllByRole("option")
        .map((option) => option.getAttribute("value"))
    ).toEqual([...LOOP_SKIP_FILTER_OPTIONS]);
    expect(select).toHaveValue("");
    await user.selectOptions(select, "user_disabled");
    await waitFor(() => expect(targetRows()).toHaveLength(1));
    expect(targetRows()[0]).toHaveTextContent("skipped@example.test");
    expect(select).toHaveValue("user_disabled");
    expect(harness.targetUrls.length).toBeGreaterThan(0);
    expect(harness.targetUrls[harness.targetUrls.length - 1]).toContain("skip_reason=user_disabled");
    const reads = harness.targetUrls.length;
    await user.selectOptions(select, "");
    await waitFor(() => expect(targetRows()).toHaveLength(2));
    expect(select).toHaveValue("");
    expect(harness.targetUrls.length).toBe(reads);
  });

  it("opens the edit modal prefilled and cancels without writing", async () => {
    const user = userEvent.setup();
    const harness = await renderPage({});
    await screen.findByTestId("loop-job-detail-targets");
    await user.click(screen.getByRole("button", { name: "Edit" }));
    expect(await screen.findByRole("heading", { name: "Edit job" })).toBeInTheDocument();
    expect(screen.getByLabelText("Slug")).toHaveValue("probe-job");
    expect(screen.getByLabelText("Admin name")).toHaveValue("Probe job");
    await user.clear(screen.getByLabelText("Admin name"));
    await user.type(screen.getByLabelText("Admin name"), "Discarded rename");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByTestId("loop-job-modal-form")).not.toBeInTheDocument());
    expect(harness.patches).toEqual([]);
    expect(screen.getByTestId("loop-job-detail-title")).toHaveTextContent("Probe job");
  });

  it("saves the edit modal and the detail revalidates with the rename", async () => {
    const user = userEvent.setup();
    const harness = await renderPage({});
    await screen.findByTestId("loop-job-detail-targets");
    await user.click(screen.getByRole("button", { name: "Edit" }));
    await screen.findByTestId("loop-job-modal-form");
    await user.clear(screen.getByLabelText("Admin name"));
    await user.type(screen.getByLabelText("Admin name"), "Renamed probe");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.queryByTestId("loop-job-modal-form")).not.toBeInTheDocument());
    expect(harness.patches.length).toBe(1);
    await waitFor(() => expect(screen.getByTestId("loop-job-detail-title")).toHaveTextContent("Renamed probe"));
  });

  it("deletes at once with no dialog, toasts, and returns to the list (ADM-060/073)", async () => {
    const user = userEvent.setup();
    const confirmSpy = vi.fn();
    window.confirm = confirmSpy;
    let release!: () => void;
    const deleteGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const harness = await renderPage({ deleteGate });
    await screen.findByTestId("loop-job-detail-targets");
    await user.click(screen.getByRole("button", { name: "Delete" }));
    // The write is already in flight while a gating confirm would
    // still be open: toasts carry the dialog role too, but only a
    // confirm carries buttons (mirrors the oracle's delete watch).
    await waitFor(() => expect(harness.deletes).toHaveLength(1));
    const gated = screen.queryAllByRole("dialog").filter((node) => within(node).queryByRole("button") !== null);
    expect(gated).toEqual([]);
    expect(screen.queryByTestId("loop-job-modal-form")).not.toBeInTheDocument();
    expect(confirmSpy).not.toHaveBeenCalled();
    release();
    expect(await screen.findByText(LOOP_JOB_DELETED_TITLE)).toBeInTheDocument();
    expect(screen.getByText(LOOP_JOB_DELETED_MESSAGE)).toBeInTheDocument();
    await waitFor(() => expect(harness.pathname()).toBe("/god-mode/loop/"));
    expect(harness.serverJob()).toBeNull();
  });

  it("failed delete toasts an error and stays on the detail (ADM-060)", async () => {
    const user = userEvent.setup();
    const harness = await renderPage({ deleteFails: true });
    await screen.findByTestId("loop-job-detail-targets");
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(await screen.findByText(LOOP_JOB_DELETE_FAILED_TITLE)).toBeInTheDocument();
    expect(screen.getByText(LOOP_JOB_DELETE_FAILED_MESSAGE)).toBeInTheDocument();
    expect(screen.getByTestId("loop-job-detail-title")).toHaveTextContent("Probe job");
    expect(screen.getByRole("button", { name: "Edit" })).toBeInTheDocument();
    expect(harness.pathname()).toContain(JOB_ID);
    expect(harness.serverJob()?.slug).toBe("probe-job");
  });

  it("disables delete while one is in flight and sends a single write", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const deleteGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const harness = await renderPage({ deleteGate });
    await screen.findByTestId("loop-job-detail-targets");
    const remove = screen.getByRole("button", { name: "Delete" });
    expect(remove).toBeEnabled();
    await user.click(remove);
    await waitFor(() => expect(harness.deletes).toHaveLength(1));
    expect(remove).toBeDisabled();
    await user.click(remove);
    release();
    await waitFor(() => expect(harness.pathname()).toBe("/god-mode/loop/"));
    expect(harness.deletes).toHaveLength(1);
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The cycle quick look: the shared frame with the cycle name, the
// summary body behind a skeleton, an honest error with a retry, and
// the copy and open-in-new-tab actions.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { CycleRow as CycleRowData } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/toaster.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { notifySuccess } from "../../../shared/shell/toaster.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID, jsonResponse, makeCycleRow } from "../testing.js";
import { CyclePeekPanel } from "./CyclePeekPanel.js";

const DETAIL_PATH = `/api/workspaces/acme/projects/${CYCLES_PROJECT_ID}/cycles/${CYCLES_CYCLE_ID}/`;

let detail: { row: CycleRowData; status: number; delayMs: number };

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

beforeEach(() => {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  detail = { row: makeCycleRow({ name: "Cycle 7", status: "upcoming" }), status: 200, delayMs: 0 };
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith(DETAIL_PATH)) {
      if (detail.delayMs > 0) await sleep(detail.delayMs);
      return jsonResponse(url, detail.status, detail.status === 200 ? detail.row : { detail: "boom" });
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.mocked(notifySuccess).mockClear();
});

function renderPeek(props: { fallbackName?: string; onClose?: () => void } = {}): void {
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const wsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const listRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "projects/$projectId/cycles",
    component: () => (
      <CyclePeekPanel
        workspaceSlug="acme"
        projectId={CYCLES_PROJECT_ID}
        cycleId={CYCLES_CYCLE_ID}
        fallbackName={props.fallbackName ?? ""}
        projectTimeZone="UTC"
        onClose={props.onClose ?? (() => undefined)}
      />
    ),
  });
  const detailRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "projects/$projectId/cycles/$cycleId",
    component: () => <p>detail</p>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([listRoute, detailRoute])]),
    history: createMemoryHistory({ initialEntries: [`/acme/projects/${CYCLES_PROJECT_ID}/cycles`] }),
    defaultPreload: false,
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

describe("CyclePeekPanel", () => {
  it("skeletons with the list name while the detail read runs, then shows the summary", async () => {
    detail.delayMs = 50;
    renderPeek({ fallbackName: "Cycle 7" });
    expect(await screen.findByTestId("cycle-preview")).toBeInTheDocument();
    expect(screen.getByTestId("cycle-peek-skeleton")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Cycle name" })).toHaveValue("Cycle 7");
    expect(await screen.findByTestId("cycle-peek-body")).toBeInTheDocument();
    expect(screen.queryByTestId("cycle-peek-skeleton")).not.toBeInTheDocument();
  });

  it("shows the marker, dates, ring, counts, and the full-detail link", async () => {
    detail.row = makeCycleRow({
      name: "Cycle 7",
      status: "upcoming",
      total_issues: 4,
      completed_issues: 1,
      cancelled_issues: 1,
    });
    renderPeek();
    expect(await screen.findByTestId("cycle-peek-body")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Cycle name" })).toHaveValue("Cycle 7");
    expect(screen.getByText("Yet to start")).toBeInTheDocument();
    expect(screen.getByTestId("cycle-date-span")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "33% complete" })).toBeInTheDocument();
    expect(screen.getByText("1 of 3 work items")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open full detail" })).toHaveAttribute(
      "href",
      `/acme/projects/${CYCLES_PROJECT_ID}/cycles/${CYCLES_CYCLE_ID}`
    );
  });

  it("singularizes the single work item", async () => {
    detail.row = makeCycleRow({
      name: "Cycle 7",
      status: "current",
      total_issues: 1,
      completed_issues: 1,
      cancelled_issues: 0,
    });
    renderPeek();
    expect(await screen.findByText("1 of 1 work item")).toBeInTheDocument();
  });

  it("retries a failed detail read", async () => {
    detail.status = 500;
    renderPeek();
    expect(await screen.findByTestId("cycle-peek-error")).toBeInTheDocument();
    expect(screen.getByText("Couldn't load this cycle.")).toBeInTheDocument();
    detail.status = 200;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByTestId("cycle-peek-body")).toBeInTheDocument();
  });

  it("closes from the frame", async () => {
    const onClose = vi.fn();
    renderPeek({ onClose });
    expect(await screen.findByTestId("cycle-preview")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("copies the cycle address from the peek actions", async () => {
    renderPeek();
    expect(await screen.findByTestId("cycle-peek-body")).toBeInTheDocument();
    const address = `https://app.example.test/acme/projects/${CYCLES_PROJECT_ID}/cycles/${CYCLES_CYCLE_ID}`;
    vi.spyOn(platform, "shareableUrl").mockReturnValue(address);
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    fireEvent.click(screen.getByRole("button", { name: "Copy cycle link" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(address));
    expect(notifySuccess).toHaveBeenCalledWith("Cycle link copied", undefined);
  });

  it("opens the cycle in a new tab from the peek actions", async () => {
    renderPeek();
    expect(await screen.findByTestId("cycle-peek-body")).toBeInTheDocument();
    const address = `https://app.example.test/acme/projects/${CYCLES_PROJECT_ID}/cycles/${CYCLES_CYCLE_ID}`;
    vi.spyOn(platform, "shareableUrl").mockReturnValue(address);
    const openExternal = vi.spyOn(platform, "openExternal").mockResolvedValue(undefined);
    fireEvent.click(screen.getByRole("button", { name: "Open cycle in new tab" }));
    await waitFor(() => expect(openExternal).toHaveBeenCalledWith(address));
  });
});

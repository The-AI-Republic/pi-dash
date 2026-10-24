// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived-cycle quick look: the archived record's state marker,
// dates and progress summary from the archived detail read, with the
// live panel's editing affordances left out. A skeleton covers the
// load and a failed read offers a retry.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import type { ArchivedCycleDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const share = vi.hoisted(() => ({ copyCycleLink: vi.fn(), openCycleInNewTab: vi.fn() }));

vi.mock("../../cycles/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../cycles/index.js")>();
  return { ...original, copyCycleLink: share.copyCycleLink, openCycleInNewTab: share.openCycleInNewTab };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { jsonResponse } from "../../cycles/testing.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { ArchivedCyclePeekPanel } from "./ArchivedCyclePeekPanel.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";
const DETAIL_PATH = `/api/workspaces/acme/projects/${PROJECT}/archived-cycles/${CYCLE}/`;

let detail: ArchivedCycleDetail | null;
let status: number;

function archivedDetail(): ArchivedCycleDetail {
  return {
    id: CYCLE,
    workspace_id: "123e4567-e89b-12d3-a456-426614174000",
    project_id: PROJECT,
    name: "Archived cycle",
    description: "",
    start_date: "2026-01-01T00:00:00Z",
    end_date: "2026-02-01T00:00:00Z",
    owned_by_id: "123e4567-e89b-12d3-a456-426614174010",
    status: "COMPLETED",
    archived_at: "2026-03-01T00:00:00Z",
    is_favorite: false,
    total_issues: 4,
    completed_issues: 3,
    started_issues: 1,
    unstarted_issues: 0,
    cancelled_issues: 0,
    backlog_issues: 0,
    assignee_ids: [],
    sub_issues: 0,
    completed_estimate_points: 0,
    total_estimate_points: 0,
    estimate_distribution: {},
    distribution: { assignees: [], labels: [], completion_chart: null },
  };
}

beforeEach(() => {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  detail = archivedDetail();
  status = 200;
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith(DETAIL_PATH)) return jsonResponse(url, status, detail ?? { detail: "missing" });
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

function renderPeek(onClose: () => void = () => undefined): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ArchivedCyclePeekPanel
        workspaceSlug="acme"
        projectId={PROJECT}
        cycleId={CYCLE}
        fallbackName="Row name"
        projectTimeZone="UTC"
        onClose={onClose}
      />
    </QueryClientProvider>
  );
}

describe("ArchivedCyclePeekPanel", () => {
  it("covers the load with a skeleton under the row's name", () => {
    renderPeek();

    expect(screen.getByTestId("archived-cycle-preview")).toBeInTheDocument();
    expect(screen.getByTestId("archived-cycle-peek-skeleton")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Row name" })).toBeInTheDocument();
  });

  it("renders the archived record's marker, dates and progress summary", async () => {
    renderPeek();

    const body = await screen.findByTestId("archived-cycle-peek-body");
    expect(screen.getByRole("heading", { name: "Archived cycle" })).toBeInTheDocument();
    expect(body).toHaveTextContent("Completed");
    expect(body).toHaveTextContent("3 of 4 work items");
    expect(screen.getByTestId("archived-cycle-peek-summary")).toBeInTheDocument();
    expect(screen.getByTestId("cycle-date-span")).toBeInTheDocument();
  });

  it("reads a lone work item in the singular", async () => {
    detail = { ...archivedDetail(), total_issues: 1, completed_issues: 1 };
    renderPeek();

    await screen.findByTestId("archived-cycle-peek-body");
    expect(screen.getByTestId("archived-cycle-peek-summary")).toHaveTextContent(/1 of 1 work item$/);
  });

  it("offers no status picker and no add-link control", async () => {
    renderPeek();
    await screen.findByTestId("archived-cycle-peek-body");

    const panel = screen.getByTestId("archived-cycle-preview");
    expect(panel.querySelector('[role="combobox"], select')).toBeNull();
    const buttons = Array.from(panel.querySelectorAll("button"))
      .map((button) => button.getAttribute("aria-label") ?? "")
      .join(" ");
    expect(buttons).not.toMatch(/status/i);
    expect(buttons).not.toMatch(/add link|attach/i);
  });

  it("copies and opens the cycle link without leaving the peek", async () => {
    renderPeek();
    await screen.findByTestId("archived-cycle-peek-body");

    fireEvent.click(screen.getByRole("button", { name: "Copy cycle link" }));
    fireEvent.click(screen.getByRole("button", { name: "Open cycle in new tab" }));

    expect(share.copyCycleLink).toHaveBeenCalledWith("acme", PROJECT, CYCLE);
    expect(share.openCycleInNewTab).toHaveBeenCalledWith("acme", PROJECT, CYCLE);
    expect(screen.getByTestId("archived-cycle-preview")).toBeInTheDocument();
  });

  it("closes through the dismiss control", async () => {
    const onClose = vi.fn();
    renderPeek(onClose);
    await screen.findByTestId("archived-cycle-peek-body");

    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("retries a failed read instead of a blank panel", async () => {
    status = 404;
    detail = null;
    renderPeek();

    expect(await screen.findByTestId("archived-cycle-peek-error")).toBeInTheDocument();
    status = 200;
    detail = archivedDetail();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));

    await screen.findByTestId("archived-cycle-peek-body");
    expect(screen.getByRole("heading", { name: "Archived cycle" })).toBeInTheDocument();
  });
});

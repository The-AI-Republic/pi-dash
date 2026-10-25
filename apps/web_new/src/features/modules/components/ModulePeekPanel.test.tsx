// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module quick look (parity/specs/modules.md §5, MOD-019): the panel
// mounts the shared peek chrome with the module's name, a work-item
// summary and the analytics sidebar read-only (plus slice L's progress
// section), reads the archived record on the archived surface, shows a
// missing state for a module that does not resolve, and closes through the
// chrome.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ArchivedModuleDetail, ModuleDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const rolesHarness = vi.hoisted(() => ({ role: 20 as number | null }));

vi.mock("../../auth/index.js", () => ({
  useProjectRoles: () => ({ data: rolesHarness.role === null ? {} : { [PROJECT]: rolesHarness.role } }),
}));

// The sidebar's own suite covers its fields; here the panel's job is to
// mount it read-only with the progress section and the right read.
vi.mock("./sidebar/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return {
    ...original,
    ModuleAnalyticsSidebar: (props: Record<string, unknown>) => (
      <div
        data-testid="sidebar-stub"
        data-module-id={String(props["moduleId"])}
        data-read-only={String(props["readOnly"])}
        data-archived={String(props["archived"])}
        data-can-edit={String(props["canEdit"])}
        data-has-progress={String(props["progress"] !== undefined && props["progress"] !== null)}
      >
        {props["progress"] as React.ReactNode}
      </div>
    ),
    ModuleProgressSection: (props: Record<string, unknown>) => (
      <div
        data-testid="progress-stub"
        data-read-only={String(props["readOnly"])}
        data-archived={String(props["archived"])}
      />
    ),
  };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { makeModuleRow } from "../testing.js";
import { MODULE_PEEK_MISSING_TITLE, ModulePeekPanel, modulePeekSummary } from "./ModulePeekPanel.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";
const DETAIL_PATH = `/api/workspaces/${SLUG}/projects/${PROJECT}/modules/${MODULE}/`;
const ARCHIVED_PATH = `/api/workspaces/${SLUG}/projects/${PROJECT}/archived-modules/${MODULE}/`;

function detailFixture(overrides: Partial<ModuleDetail> = {}): ModuleDetail {
  return {
    ...makeModuleRow({ name: "Delivery", total_issues: 2, completed_issues: 1 }),
    description_text: null,
    description_html: null,
    total_estimate_points: 0,
    completed_estimate_points: 0,
    backlog_estimate_points: 0,
    unstarted_estimate_points: 0,
    started_estimate_points: 0,
    cancelled_estimate_points: 0,
    link_module: [],
    sub_issues: null,
    distribution: { assignees: [], labels: [], completion_chart: {} },
    estimate_distribution: { assignees: [], labels: [], completion_chart: {} },
    ...overrides,
  } as ModuleDetail;
}

function archivedFixture(overrides: Partial<ArchivedModuleDetail> = {}): ArchivedModuleDetail {
  return {
    ...detailFixture(),
    description_text: null,
    description_html: null,
    archived_at: "2026-03-01T00:00:00Z",
    ...overrides,
  } as ArchivedModuleDetail;
}

interface Harness {
  detail: ModuleDetail | ArchivedModuleDetail;
  detailStatus: number;
  detailDelay: number;
  detailCalls: number;
  archivedCalls: number;
}

let harness: Harness;
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

beforeEach(() => {
  // The peek paints into the shell's portal root, which index.html serves.
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  rolesHarness.role = 20;
  harness = { detail: detailFixture(), detailStatus: 200, detailDelay: 0, detailCalls: 0, archivedCalls: 0 };
  createAppClient("https://api.test", (async (url: string) => {
    const response = (status: number, body: unknown) =>
      ({ status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) }) as never;
    if (url.endsWith(DETAIL_PATH)) {
      harness.detailCalls += 1;
      if (harness.detailDelay > 0) await sleep(harness.detailDelay);
      return response(harness.detailStatus, harness.detailStatus === 200 ? harness.detail : { detail: "boom" });
    }
    if (url.endsWith(ARCHIVED_PATH)) {
      harness.archivedCalls += 1;
      return response(200, archivedFixture());
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
});

function renderPanel(overrides: Partial<React.ComponentProps<typeof ModulePeekPanel>> = {}) {
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ModulePeekPanel workspaceSlug={SLUG} projectId={PROJECT} moduleId={MODULE} onClose={onClose} {...overrides} />
    </QueryClientProvider>
  );
  return { onClose };
}

describe("ModulePeekPanel", () => {
  it("mounts the shared peek chrome beside the body with a loading skeleton first", () => {
    harness.detailDelay = 50;
    renderPanel({ fallbackName: "Delivery" });
    expect(screen.getByTestId("modules-peek")).toBeInTheDocument();
    expect(screen.getByTestId("modules-peek-skeleton")).toBeInTheDocument();
  });

  it("fills the panel with the summary and the sidebar read-only", async () => {
    renderPanel();
    await waitFor(() => expect(screen.getByTestId("modules-peek-body")).toBeInTheDocument());
    expect(screen.getByRole("heading", { name: "Delivery" })).toBeInTheDocument();
    expect(screen.getByText("Module")).toBeInTheDocument();
    expect(screen.getByTestId("modules-peek-summary")).toHaveTextContent("1/2 work items");
    const sidebar = screen.getByTestId("sidebar-stub");
    expect(sidebar).toHaveAttribute("data-module-id", MODULE);
    expect(sidebar).toHaveAttribute("data-read-only", "true");
    expect(sidebar).toHaveAttribute("data-archived", "false");
    // An editor's role still mounts the peek read-only.
    expect(sidebar).toHaveAttribute("data-can-edit", "true");
    expect(sidebar).toHaveAttribute("data-has-progress", "true");
    const progress = screen.getByTestId("progress-stub");
    expect(progress).toHaveAttribute("data-read-only", "true");
    expect(progress).toHaveAttribute("data-archived", "false");
  });

  it("falls back to the list's name while the detail read lands", async () => {
    harness.detailDelay = 40;
    renderPanel({ fallbackName: "From the list" });
    expect(screen.getByRole("heading", { name: "From the list" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Delivery" })).toBeInTheDocument());
  });

  it("reads the archived record and marks the panel archived", async () => {
    renderPanel({ archived: true });
    await waitFor(() => expect(screen.getByTestId("modules-peek-body")).toBeInTheDocument());
    expect(harness.archivedCalls).toBe(1);
    expect(harness.detailCalls).toBe(0);
    expect(screen.getByText("Archived module")).toBeInTheDocument();
    expect(screen.getByTestId("sidebar-stub")).toHaveAttribute("data-archived", "true");
    expect(screen.getByTestId("sidebar-stub")).toHaveAttribute("data-read-only", "true");
    expect(screen.getByTestId("progress-stub")).toHaveAttribute("data-archived", "true");
  });

  it("reads the archived record read-only for a viewer with no role yet", async () => {
    rolesHarness.role = null;
    renderPanel({ archived: true });
    await waitFor(() => expect(screen.getByTestId("modules-peek-body")).toBeInTheDocument());
    expect(screen.getByTestId("sidebar-stub")).toHaveAttribute("data-can-edit", "false");
    expect(screen.getByTestId("sidebar-stub")).toHaveAttribute("data-read-only", "true");
  });

  it("shows the missing state when the module read fails, and still closes", async () => {
    harness.detailStatus = 404;
    const { onClose } = renderPanel();
    await waitFor(() => expect(screen.getByTestId("modules-peek-missing")).toBeInTheDocument());
    const missing = screen.getByTestId("modules-peek-missing");
    expect(missing).toHaveTextContent(MODULE_PEEK_MISSING_TITLE);
    fireEvent.click(within(missing).getByRole("button", { name: "Close preview" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes through the chrome's close control", async () => {
    const { onClose } = renderPanel();
    await waitFor(() => expect(screen.getByTestId("modules-peek-body")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape", async () => {
    const { onClose } = renderPanel();
    await waitFor(() => expect(screen.getByTestId("modules-peek-body")).toBeInTheDocument());
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("modulePeekSummary", () => {
  it("reads the same done-over-total line the card does", () => {
    expect(modulePeekSummary({ completed_issues: 3, total_issues: 7 })).toBe("3/7 work items");
    expect(modulePeekSummary({ completed_issues: 0, total_issues: 0 })).toBe("0 work items");
  });
});

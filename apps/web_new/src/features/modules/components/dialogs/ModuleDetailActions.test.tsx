// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail page's module menu (MOD-024/025): Edit opens the shared
// update dialog prefilled from the detail record, Delete confirms and the
// landed delete returns to the modules list, and entries other slices own
// sit between them without a second menu.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ModuleDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => routerHarness.navigate,
}));

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../shared/shell/index.js")>()),
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { jsonResponse, makeProjectRow, MODULES_MODULE_ID, MODULES_PROJECT_ID, MODULES_WS_ID } from "../../testing.js";
import { ModuleDetailActions } from "./ModuleDetailActions.js";

const PROJECT_PATH = `/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/`;
const DETAIL_PATH = `${PROJECT_PATH}modules/${MODULES_MODULE_ID}/`;
const LIST_PATH = `/acme/projects/${MODULES_PROJECT_ID}/modules/`;

function detail(overrides: Partial<ModuleDetail> = {}): ModuleDetail {
  return {
    id: MODULES_MODULE_ID,
    workspace_id: MODULES_WS_ID,
    project_id: MODULES_PROJECT_ID,
    name: "Delivery",
    description: "",
    start_date: null,
    target_date: null,
    status: "planned",
    lead_id: null,
    member_ids: [],
    sort_order: 0,
    is_favorite: false,
    total_issues: 0,
    completed_issues: 0,
    cancelled_issues: 0,
    started_issues: 0,
    unstarted_issues: 0,
    backlog_issues: 0,
    total_estimate_points: 0,
    completed_estimate_points: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    link_module: [],
    sub_issues: null,
    distribution: { assignees: [], labels: [], completion_chart: {} },
    estimate_distribution: { assignees: [], labels: [], completion_chart: {} },
    ...overrides,
  };
}

let harness: { deleteStatus: number };

beforeEach(() => {
  harness = { deleteStatus: 204 };
  routerHarness.navigate.mockReset();
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url === `https://api.test${PROJECT_PATH}`) return jsonResponse(url, 200, makeProjectRow());
    if (url === `https://api.test${DETAIL_PATH}` && (init.method ?? "GET") === "GET") {
      return jsonResponse(url, 200, detail());
    }
    if (url === `https://api.test${DETAIL_PATH}` && init.method === "DELETE") {
      if (harness.deleteStatus === 204) return jsonResponse(url, 204, null);
      return jsonResponse(url, harness.deleteStatus, { detail: "Refused." });
    }
    if (url === "https://api.test/api/workspaces/acme/projects/") return jsonResponse(url, 200, []);
    if (url.endsWith("/api/workspaces/acme/members/")) return jsonResponse(url, 200, []);
    if (url.endsWith("/api/users/me/")) return jsonResponse(url, 500, {});
    return jsonResponse(url, 500, {});
  }) as never);
});

afterEach(() => {
  resetAppClient();
  notify.notifyError.mockReset();
  notify.notifySuccess.mockReset();
});

function renderActions(props: Partial<React.ComponentProps<typeof ModuleDetailActions>> = {}) {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ModuleDetailActions
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        moduleId={MODULES_MODULE_ID}
        {...props}
      />
    </QueryClientProvider>
  );
}

async function openMenu(): Promise<void> {
  const trigger = await screen.findByRole("button", { name: "Module actions" });
  await waitFor(() => expect(trigger).toBeEnabled());
  fireEvent.click(trigger);
}

describe("ModuleDetailActions", () => {
  it("keeps the trigger disabled until the module reads", async () => {
    renderActions();
    expect(screen.getByRole("button", { name: "Module actions" })).toBeDisabled();
    await waitFor(() => expect(screen.getByRole("button", { name: "Module actions" })).toBeEnabled());
  });

  it("opens the update dialog prefilled from the record", async () => {
    renderActions();
    await openMenu();
    fireEvent.click(await screen.findByRole("menuitem", { name: "Edit" }));
    expect(await screen.findByRole("heading", { name: "Update module" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("Delivery");
    expect(screen.queryByTestId("module-dialog-project")).not.toBeInTheDocument();
  });

  it("places other slices' entries between Edit and Delete", async () => {
    const onSelect = vi.fn();
    renderActions({ extraEntries: [{ label: "Archive", onSelect }] });
    await openMenu();
    const labels = (await screen.findAllByRole("menuitem")).map((entry) => entry.textContent?.trim());
    expect(labels).toEqual(["Edit", "Archive", "Delete"]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Archive" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("confirms deletion and returns to the modules list once it lands", async () => {
    renderActions();
    await openMenu();
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete" }));
    expect(await screen.findByRole("heading", { name: "Delete module" })).toBeInTheDocument();
    expect(screen.getByTestId("module-delete-body")).toHaveTextContent("Delivery");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(routerHarness.navigate).toHaveBeenCalledWith({ to: LIST_PATH, search: {}, replace: true })
    );
    expect(notify.notifySuccess).toHaveBeenCalledWith("Success!", expect.stringContaining("Delivery"));
  });

  it("stays on the page when the server refuses the delete", async () => {
    harness.deleteStatus = 403;
    renderActions();
    await openMenu();
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete" }));
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith("You don't have permission to perform this action.", "Refused.")
    );
    expect(routerHarness.navigate).not.toHaveBeenCalled();
  });
});

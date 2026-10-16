// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { loadWorkItemDraft } from "../lib/createDraft.js";
import { WorkItemCreateDialog, type WorkItemCreateProject } from "./WorkItemCreateDialog.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError } from "../../../shared/shell/index.js";

const PROJECT_ATLAS = "223e4567-e89b-12d3-a456-426614174001";
const PROJECT_BEACON = "323e4567-e89b-12d3-a456-426614174002";
const PROJECT_NOVA = "423e4567-e89b-12d3-a456-426614174003";

const PROJECTS: WorkItemCreateProject[] = [
  { id: PROJECT_ATLAS, name: "Atlas" },
  { id: PROJECT_BEACON, name: "Beacon" },
];

function createdRow(projectId: string): Record<string, unknown> {
  return {
    id: "323e4567-e89b-12d3-a456-426614174002",
    name: "Fresh report",
    state_id: null,
    sort_order: 65535,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    start_date: null,
    target_date: null,
    sequence_id: 7,
    project_id: projectId,
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
    deleted_at: null,
  };
}

function stubClient(status: number) {
  const calls: Array<{ url: string; method: string; body: string }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    const match = /\/projects\/([^/]+)\/issues\//.exec(url);
    return {
      status,
      url,
      headers: { get: () => null },
      text: async () => (status === 201 ? JSON.stringify(createdRow(match?.[1] ?? PROJECT_ATLAS)) : "{}"),
    };
  }) as never);
  return { calls };
}

function setup(status: number, projects: WorkItemCreateProject[] = PROJECTS) {
  const { calls } = stubClient(status);
  const onOpenChange = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <WorkItemCreateDialog open workspaceSlug="acme" projects={projects} onOpenChange={onOpenChange} />
    </QueryClientProvider>
  );
  return { calls, onOpenChange, view, queryClient };
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
  globalThis.localStorage.clear();
});

describe("WorkItemCreateDialog", () => {
  it("renders the title, the first project and an empty form", () => {
    setup(201);
    expect(screen.getByRole("dialog", { name: "Create new work item" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Target project" })).toHaveTextContent("Atlas");
    expect(screen.getByLabelText("Title")).toHaveValue("");
  });

  it("prefills the kept draft", () => {
    globalThis.localStorage.setItem(
      "pidash:work-item-draft:acme",
      JSON.stringify({ title: "Half typed", projectId: PROJECT_BEACON })
    );
    setup(201);
    expect(screen.getByLabelText("Title")).toHaveValue("Half typed");
    expect(screen.getByRole("button", { name: "Target project" })).toHaveTextContent("Beacon");
  });

  it("falls back to the first project for a stale draft target", () => {
    globalThis.localStorage.setItem(
      "pidash:work-item-draft:acme",
      JSON.stringify({ title: "Half typed", projectId: "gone" })
    );
    setup(201);
    expect(screen.getByLabelText("Title")).toHaveValue("Half typed");
    expect(screen.getByRole("button", { name: "Target project" })).toHaveTextContent("Atlas");
  });

  it("keeps typing and the picked project as the draft", async () => {
    const user = userEvent.setup();
    setup(201);
    await user.type(screen.getByLabelText("Title"), "Draft me");
    await user.click(screen.getByRole("button", { name: "Target project" }));
    await user.click(await screen.findByRole("menuitem", { name: "Beacon" }));
    expect(loadWorkItemDraft("acme")).toEqual({ title: "Draft me", projectId: PROJECT_BEACON });
  });

  it("flags an empty title without posting", async () => {
    const user = userEvent.setup();
    const h = setup(201);
    await user.click(screen.getByRole("button", { name: "Create work item" }));
    expect(screen.getByText("Give the work item a title.")).toBeInTheDocument();
    expect(h.calls.some((call) => call.method === "POST")).toBe(false);
    await user.type(screen.getByLabelText("Title"), "Now titled");
    expect(screen.queryByText("Give the work item a title.")).not.toBeInTheDocument();
  });

  it("posts the trimmed title, clears the draft and closes", async () => {
    const user = userEvent.setup();
    const h = setup(201);
    await user.click(screen.getByRole("button", { name: "Target project" }));
    await user.click(await screen.findByRole("menuitem", { name: "Beacon" }));
    await user.type(screen.getByLabelText("Title"), "  Fresh report  ");
    await user.click(screen.getByRole("button", { name: "Create work item" }));
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    const post = h.calls.find((call) => call.method === "POST");
    expect(post?.url).toContain(`/api/workspaces/acme/projects/${PROJECT_BEACON}/issues/`);
    expect(JSON.parse(post?.body ?? "{}")).toEqual({ name: "Fresh report" });
    expect(loadWorkItemDraft("acme")).toBeNull();
  });

  it("stays open and toasts when the submit fails", async () => {
    const user = userEvent.setup();
    const h = setup(400);
    await user.type(screen.getByLabelText("Title"), "Fresh report");
    await user.click(screen.getByRole("button", { name: "Create work item" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(h.onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Create new work item" })).toBeInTheDocument();
  });

  it("cancels without posting", async () => {
    const user = userEvent.setup();
    const h = setup(201);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(h.onOpenChange).toHaveBeenCalledWith(false);
    expect(h.calls.some((call) => call.method === "POST")).toBe(false);
  });

  it("names the missing target without projects", () => {
    const h = setup(201, []);
    expect(screen.getByText(/Join a project first/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create work item" })).not.toBeInTheDocument();
    expect(h.onOpenChange).not.toHaveBeenCalled();
  });

  it("drops the submit when no target project is held", async () => {
    const user = userEvent.setup();
    const h = setup(201, []);
    h.view.rerender(
      <QueryClientProvider client={h.queryClient}>
        <WorkItemCreateDialog open workspaceSlug="acme" projects={PROJECTS} onOpenChange={h.onOpenChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Target project" })).toHaveTextContent("Pick a project");
    await user.type(screen.getByLabelText("Title"), "No home");
    await user.click(screen.getByRole("button", { name: "Create work item" }));
    expect(h.calls.some((call) => call.method === "POST")).toBe(false);
    expect(h.onOpenChange).not.toHaveBeenCalled();
  });

  it("prompts for a project when the list no longer holds the pick", () => {
    const { view, queryClient } = setup(201);
    expect(screen.getByRole("button", { name: "Target project" })).toHaveTextContent("Atlas");
    view.rerender(
      <QueryClientProvider client={queryClient}>
        <WorkItemCreateDialog
          open
          workspaceSlug="acme"
          projects={[{ id: PROJECT_NOVA, name: "Nova" }]}
          onOpenChange={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Target project" })).toHaveTextContent("Pick a project");
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Epic create/edit dialog (ISS-232): absent in the community build, and in
// a cloud build it seeds from the epic, refuses a nameless form locally,
// and writes through the epics collection.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Issue } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import { EPICS_FLAG, EpicDialog } from "./EpicDialog.js";
import { WORKFLOW_FLAG } from "./flags.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "./providers.js";

const PROJECT = "323e4567-e89b-12d3-a456-426614174002";
const EPIC = "623e4567-e89b-12d3-a456-426614174010";
const TODO = "723e4567-e89b-12d3-a456-426614174011";
const DONE = "823e4567-e89b-12d3-a456-426614174012";

const STATES = [
  {
    id: TODO,
    project_id: PROJECT,
    workspace_id: PROJECT,
    name: "Todo",
    color: "#aaa",
    group: "unstarted",
    default: true,
    description: "",
    sequence: 1,
  },
  {
    id: DONE,
    project_id: PROJECT,
    workspace_id: PROJECT,
    name: "Done",
    color: "#2f9e5b",
    group: "completed",
    default: false,
    description: "",
    sequence: 2,
  },
];

const calls: { method: string; url: string; body: unknown }[] = [];
let status = 200;
/** Holds a POST open so a test can look at the form mid-write. */
let postGate: Promise<void> | null = null;

function serve(): void {
  createAppClient("https://api.test", (async (url: string, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "c" }) };
    }
    calls.push({ method, url, body: init?.body === undefined ? undefined : JSON.parse(String(init.body)) });
    if (method === "POST" && postGate !== null) await postGate;
    const body = method === "POST" ? createdRow() : url.endsWith("/states/") ? STATES : {};
    return { status, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(body) };
  }) as never);
}

function createdRow() {
  return {
    id: EPIC,
    name: "Big rock",
    state_id: TODO,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 3,
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
    deleted_at: null,
  };
}

function epic(): Issue {
  return {
    id: EPIC,
    name: "Big rock",
    state_id: TODO,
    sort_order: 2,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    start_date: "2026-10-01",
    target_date: "2026-10-20",
    sequence_id: 3,
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
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>words</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
  };
}

function renderDialog(props: Partial<React.ComponentProps<typeof EpicDialog>> = {}) {
  const onOpenChange = vi.fn();
  const onSaved = vi.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <EpicDialog
        workspaceSlug="acme"
        projectId={PROJECT}
        open
        onOpenChange={onOpenChange}
        onSaved={onSaved}
        {...props}
      />
    </QueryClientProvider>
  );
  return { onOpenChange, onSaved };
}

beforeEach(() => {
  calls.length = 0;
  status = 200;
  postGate = null;
  serve();
});
afterEach(() => {
  cleanup();
  resetAppClient();
  resetEditionIssuesProviders();
  delete edition.flags[EPICS_FLAG];
  delete edition.flags[WORKFLOW_FLAG];
});

describe("EpicDialog", () => {
  it("renders nothing in the community build", () => {
    renderDialog();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByTestId("epic-dialog")).toBeNull();
  });

  it("creates an epic from a trimmed name and the chosen fields", async () => {
    edition.flags[EPICS_FLAG] = true;
    const { onOpenChange, onSaved } = renderDialog();
    expect(screen.getByRole("dialog", { name: "Create epic" })).toBeInTheDocument();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("Epic name"), "  Big rock  ");
    await user.type(screen.getByLabelText("Description"), "Ship it");
    await user.click(screen.getByRole("button", { name: "Create epic" }));
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    const post = calls.find((call) => call.method === "POST");
    expect(post?.url).toContain(`/projects/${PROJECT}/epics/`);
    expect(post?.body).toMatchObject({
      name: "Big rock",
      description_html: "Ship it",
      priority: "none",
      assignee_ids: [],
      label_ids: [],
      start_date: null,
      target_date: null,
    });
    expect(post?.body).not.toHaveProperty("state_id");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("seeds an edit from the epic and PATCHes the changed name", async () => {
    edition.flags[EPICS_FLAG] = true;
    const { onSaved } = renderDialog({ epic: epic() });
    expect(screen.getByRole("dialog", { name: "Edit epic" })).toBeInTheDocument();
    const name = screen.getByLabelText("Epic name");
    expect(name).toHaveValue("Big rock");
    expect(screen.getByLabelText("Description")).toHaveValue("<p>words</p>");
    const user = userEvent.setup();
    await user.clear(name);
    await user.type(name, "Bigger rock");
    await user.click(screen.getByRole("button", { name: "Save epic" }));
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    const patch = calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(`/projects/${PROJECT}/epics/${EPIC}/`);
    expect(patch?.body).toMatchObject({
      name: "Bigger rock",
      state_id: TODO,
      priority: "high",
      start_date: "2026-10-01",
      target_date: "2026-10-20",
    });
  });

  it("refuses a blank name without writing", async () => {
    edition.flags[EPICS_FLAG] = true;
    const { onOpenChange, onSaved } = renderDialog();
    await userEvent.setup().click(screen.getByRole("button", { name: "Create epic" }));
    expect(await screen.findByText("Give the epic a name.")).toBeInTheDocument();
    expect(calls.filter((call) => call.method !== "GET")).toEqual([]);
    expect(onSaved).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
  });

  it("refuses every dismissal while the write is in flight", async () => {
    edition.flags[EPICS_FLAG] = true;
    let release: () => void = () => undefined;
    postGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { onOpenChange, onSaved } = renderDialog();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("Epic name"), "Big rock");
    await user.click(screen.getByRole("button", { name: "Create epic" }));
    const save = await screen.findByRole("button", { name: "Saving…" });
    expect(save).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Create epic" })).toBeInTheDocument();
    release();
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("lets the dialog dismiss itself when no write is in flight", async () => {
    edition.flags[EPICS_FLAG] = true;
    const { onOpenChange } = renderDialog();
    fireEvent.keyDown(screen.getByRole("dialog", { name: "Create epic" }), { key: "Escape" });
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
  });

  it("starts each open from the row it edits", async () => {
    edition.flags[EPICS_FLAG] = true;
    const onOpenChange = vi.fn();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    function Harness({ open, epic }: { open: boolean; epic?: Issue | undefined }): React.ReactElement {
      return (
        <QueryClientProvider client={client}>
          <EpicDialog
            workspaceSlug="acme"
            projectId={PROJECT}
            open={open}
            onOpenChange={onOpenChange}
            {...(epic === undefined ? {} : { epic })}
          />
        </QueryClientProvider>
      );
    }
    const view = render(<Harness open epic={epic()} />);
    const user = userEvent.setup();
    await user.clear(screen.getByLabelText("Epic name"));
    await user.type(screen.getByLabelText("Epic name"), "Edited draft");
    view.rerender(<Harness open={false} epic={epic()} />);
    view.rerender(<Harness open epic={epic()} />);
    expect(await screen.findByLabelText("Epic name")).toHaveValue("Big rock");
  });

  it("greys the states the workflow rules refuse", async () => {
    edition.flags[EPICS_FLAG] = true;
    edition.flags[WORKFLOW_FLAG] = true;
    registerEditionIssuesProviders({
      workflowRules: {
        refusal: ({ toStateId }) => (toStateId === DONE ? { reason: "Close the last sub-item first" } : null),
      },
    });
    const { onOpenChange } = renderDialog({ epic: epic() });
    const typed = await screen.findByText("Todo", {}, { timeout: 10_000 });
    const trigger = typed.closest("button");
    expect(trigger).not.toBeNull();
    fireEvent.click(trigger!);
    const refused = await screen.findByRole("option", { name: /Done/ }, { timeout: 10_000 });
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(refused).toBeDisabled();
    expect(refused).toHaveTextContent("Close the last sub-item first");
    expect(screen.getByRole("option", { name: "Todo" })).toBeEnabled();
  }, 20_000);

  it("keeps the dialog open and names the failure when the write is refused", async () => {
    edition.flags[EPICS_FLAG] = true;
    status = 500;
    const { onOpenChange, onSaved } = renderDialog();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("Epic name"), "Big rock");
    await user.click(screen.getByRole("button", { name: "Create epic" }));
    await waitFor(() => expect(calls.some((call) => call.method === "POST")).toBe(true));
    expect(onSaved).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
    expect(screen.getByRole("dialog", { name: "Create epic" })).toBeInTheDocument();
  });

  it("closes without writing on cancel", async () => {
    edition.flags[EPICS_FLAG] = true;
    const { onOpenChange } = renderDialog();
    await userEvent.setup().click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(calls.filter((call) => call.method !== "GET")).toEqual([]);
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Triage action area (INT-020–023, 025): the Accept and Decline
// buttons, the shared accept form's gate and hand-back, the four
// intake dialogs, and the member refusals. The shared form is a stub
// here — its own file proves the gate mechanics — so these cases pin
// the props intake hands it and what each callback does.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  INTAKE_STATUS,
  type IntakeIssueDetailItem,
  type IssueSearchHit,
  type TransportResponse,
} from "@pidash/api-client";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

interface StubAcceptProps {
  open?: boolean;
  editTitle?: string;
  editSaveLabel?: string;
  edit?: { kind: string; id: string; projectId: string | null; snapshot: unknown };
  onBeforeEditSubmit?: () => Promise<void>;
  onEditSaved?: (changedFields: readonly string[]) => void;
  onOpenChange?: (open: boolean) => void;
}

const dialogHarness = vi.hoisted(() => ({ props: null as StubAcceptProps | null }));

vi.mock("../../issues/index.js", async () => {
  const actual = await vi.importActual<typeof import("../../issues/index.js")>("../../issues/index.js");
  return {
    ...actual,
    WorkItemCreateDialog: (props: StubAcceptProps) => {
      dialogHarness.props = props;
      if (props.open !== true) return null;
      return <div data-testid="stub-accept-dialog">{props.editTitle ?? ""}</div>;
    },
  };
});

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/index.js";
import { useTriageControls, type TriageControlsScope } from "../api/triage.js";
import { ACCEPT_SAVE_LABEL, IntakeTriageActions } from "./IntakeTriageActions.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "11111111-1111-4111-8111-111111111111";
const TARGET = "22222222-2222-4222-8222-222222222222";
const DETAIL_PATH = `/api/workspaces/${WS}/projects/${PROJECT}/intake-issues/${ISSUE}/`;
const SEARCH_PATH = `/api/workspaces/${WS}/projects/${PROJECT}/search-issues/`;

function detailItem(): IntakeIssueDetailItem {
  return {
    id: ISSUE,
    name: "A request",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: null,
    label_ids: [],
    assignee_ids: [],
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>Body</p>",
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
  };
}

function hit(): IssueSearchHit {
  return {
    name: "Original work",
    id: TARGET,
    start_date: null,
    sequence_id: 3,
    project__name: "Atlas",
    project__identifier: "APO",
    project_id: PROJECT,
    workspace__slug: WS,
    state__name: "Todo",
    state__group: "unstarted",
    state__color: "#444444",
  };
}

function acceptedDetail(): unknown {
  return {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-000000000001",
    status: INTAKE_STATUS.accepted,
    duplicate_to: null,
    snoozed_till: null,
    duplicate_issue_detail: null,
    source: null,
    issue: detailItem(),
  };
}

interface ActionServer {
  patches: unknown[];
  deletes: number;
  patchStatus: number;
}

function stubTransport(server: ActionServer): void {
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
    const method = init?.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    if (url.includes(DETAIL_PATH) && method === "PATCH") {
      server.patches.push(init?.body === undefined ? null : JSON.parse(init.body));
      if (server.patchStatus >= 400) {
        return { status: server.patchStatus, url, headers: { get: () => null }, text: async () => '{"error":"no"}' };
      }
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(acceptedDetail()) };
    }
    if (url.includes(DETAIL_PATH) && method === "DELETE") {
      server.deletes += 1;
      return { status: 204, url, headers: { get: () => null }, text: async () => "" };
    }
    if (url.includes(SEARCH_PATH) && method === "GET") {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify([hit()]) };
    }
    // Reference reads the suite never asserts on fail into their defaults.
    return { status: 500, url, headers: { get: () => null }, text: async () => '{"error":"boom"}' };
  }) as never as (url: string) => Promise<TransportResponse>);
}

function scope(overrides: Partial<TriageControlsScope> = {}): TriageControlsScope {
  return {
    workspaceSlug: WS,
    projectId: PROJECT,
    projectName: "Atlas",
    projectIdentifier: "APO",
    issueId: ISSUE,
    issueKey: "APO-7",
    issue: detailItem(),
    status: INTAKE_STATUS.pending,
    snoozedTill: null,
    isAdmin: true,
    isAuthorized: true,
    isCreator: false,
    onAdvance: vi.fn(() => false),
    ...overrides,
  };
}

const harnessControls = vi.hoisted(() => ({ current: null as ReturnType<typeof useTriageControls> | null }));

function Harness({ current }: { current: TriageControlsScope }) {
  const controls = useTriageControls(current);
  harnessControls.current = controls;
  return (
    <IntakeTriageActions
      controls={controls}
      workspaceSlug={current.workspaceSlug}
      projectId={current.projectId}
      projectName={current.projectName}
      projectIdentifier={current.projectIdentifier}
      issueId={current.issueId}
    />
  );
}

function setup(current: TriageControlsScope, server: ActionServer) {
  stubTransport(server);
  dialogHarness.props = null;
  harnessControls.current = null;
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <Harness current={current} />
    </QueryClientProvider>
  );
  return queryClient;
}

function acceptProps(): StubAcceptProps {
  const props = dialogHarness.props;
  if (props === null) throw new Error("the accept form never rendered");
  return props;
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("IntakeTriageActions", () => {
  it("opens the accept form on the request's work item and saves through the gate", async () => {
    const user = userEvent.setup();
    const server: ActionServer = { patches: [], deletes: 0, patchStatus: 200 };
    const current = scope();
    setup(current, server);

    expect(screen.queryByTestId("stub-accept-dialog")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Accept" }));
    expect(screen.getByTestId("stub-accept-dialog")).toHaveTextContent("Add APO-7 to project work items");

    const props = acceptProps();
    expect(props.editSaveLabel).toBe(ACCEPT_SAVE_LABEL);
    expect(props.edit).toMatchObject({ kind: "issue", id: ISSUE, projectId: PROJECT });
    expect(props.edit?.snapshot).toEqual(detailItem());

    await React.act(async () => {
      await props.onBeforeEditSubmit?.();
    });
    expect(server.patches).toEqual([{ status: INTAKE_STATUS.accepted }]);

    React.act(() => {
      props.onEditSaved?.(["name"]);
    });
    expect(current.onAdvance).toHaveBeenCalledWith(ISSUE);
    expect(screen.queryByTestId("stub-accept-dialog")).not.toBeInTheDocument();
  });

  it("refuses a member accept without opening the form", async () => {
    const user = userEvent.setup();
    const server: ActionServer = { patches: [], deletes: 0, patchStatus: 200 };
    setup(scope({ isAdmin: false }), server);

    await user.click(screen.getByRole("button", { name: "Accept" }));
    expect(notifyError).toHaveBeenCalledWith("Permission denied", expect.stringContaining("admin"));
    expect(screen.queryByTestId("stub-accept-dialog")).not.toBeInTheDocument();
    expect(server.patches).toEqual([]);
  });

  it("keeps the accept form open when the gate fails, then closes cleanly", async () => {
    const user = userEvent.setup();
    const server: ActionServer = { patches: [], deletes: 0, patchStatus: 400 };
    setup(scope(), server);

    await user.click(screen.getByRole("button", { name: "Accept" }));
    const props = acceptProps();
    await React.act(async () => {
      await expect(props.onBeforeEditSubmit?.()).rejects.toThrow();
    });
    expect(notifyError).toHaveBeenCalledWith("Couldn't accept the request", expect.any(String));
    expect(screen.getByTestId("stub-accept-dialog")).toBeInTheDocument();

    React.act(() => {
      props.onOpenChange?.(false);
    });
    expect(screen.queryByTestId("stub-accept-dialog")).not.toBeInTheDocument();
  });

  it("declines through the confirmation and advances", async () => {
    const user = userEvent.setup();
    const server: ActionServer = { patches: [], deletes: 0, patchStatus: 200 };
    const current = scope();
    setup(current, server);

    await user.click(screen.getByRole("button", { name: "Decline" }));
    expect(screen.getByRole("dialog", { name: "Decline work item" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Decline" }));
    await waitFor(() => expect(server.patches).toEqual([{ status: INTAKE_STATUS.declined }]));
    expect(current.onAdvance).toHaveBeenCalledWith(ISSUE);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("snoozes a picked day and advances", async () => {
    const user = userEvent.setup();
    const server: ActionServer = { patches: [], deletes: 0, patchStatus: 200 };
    const current = scope();
    setup(current, server);

    React.act(() => {
      harnessControls.current?.fireAction("snooze");
    });
    await waitFor(() => expect(screen.getByRole("dialog", { name: "Snooze request" })).toBeInTheDocument());
    const tomorrow = new Date();
    tomorrow.setDate(tomorrow.getDate() + 1);
    const month = String(tomorrow.getMonth() + 1).padStart(2, "0");
    const day = String(tomorrow.getDate()).padStart(2, "0");
    const iso = `${tomorrow.getFullYear()}-${month}-${day}`;
    const cell = document.querySelector(`[data-day="${iso}"]`) as HTMLElement;
    await user.click(cell);
    await user.click(screen.getByRole("button", { name: "Snooze" }));
    await waitFor(() => expect(server.patches).toHaveLength(1));
    const body = server.patches[0] as { status: number; snoozed_till: string };
    expect(body.status).toBe(INTAKE_STATUS.snoozed);
    expect(new Date(body.snoozed_till).toDateString()).toBe(tomorrow.toDateString());
    expect(current.onAdvance).toHaveBeenCalledWith(ISSUE);
  });

  it("marks duplicate and stays on the request", async () => {
    const user = userEvent.setup();
    const server: ActionServer = { patches: [], deletes: 0, patchStatus: 200 };
    const current = scope();
    setup(current, server);

    React.act(() => {
      harnessControls.current?.fireAction("duplicate");
    });
    await waitFor(() => expect(screen.getAllByTestId("intake-duplicate-option")).toHaveLength(1));
    await user.click(screen.getByTestId("intake-duplicate-option"));
    await waitFor(() => expect(server.patches).toEqual([{ status: INTAKE_STATUS.duplicate, duplicate_to: TARGET }]));
    expect(current.onAdvance).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("deletes through the confirmation and advances", async () => {
    const user = userEvent.setup();
    const server: ActionServer = { patches: [], deletes: 0, patchStatus: 200 };
    const current = scope();
    setup(current, server);

    React.act(() => {
      harnessControls.current?.fireAction("delete");
    });
    await waitFor(() => expect(screen.getByRole("dialog", { name: "Delete work item" })).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(server.deletes).toBe(1));
    expect(current.onAdvance).toHaveBeenCalledWith(ISSUE);
  });

  it("cancels the decline, snooze, duplicate and delete dialogs without writing", async () => {
    const user = userEvent.setup();
    const server: ActionServer = { patches: [], deletes: 0, patchStatus: 200 };
    setup(scope(), server);

    for (const action of ["decline", "snooze", "duplicate", "delete"] as const) {
      React.act(() => {
        harnessControls.current?.fireAction(action);
      });
      await waitFor(() => expect(screen.getByRole("dialog")).toBeInTheDocument());
      await user.click(screen.getByRole("button", { name: "Cancel" }));
      await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    }
    expect(server.patches).toEqual([]);
    expect(server.deletes).toBe(0);
  });

  it("treats the decline dialog's Escape as cancel", async () => {
    const server: ActionServer = { patches: [], deletes: 0, patchStatus: 200 };
    setup(scope(), server);

    React.act(() => {
      harnessControls.current?.fireAction("decline");
    });
    await waitFor(() => expect(screen.getByRole("dialog")).toBeInTheDocument());
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(server.patches).toEqual([]);
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archive/delete/move confirmations (parity/specs/issues.md §11,
// ISS-134–136). Each case runs the real dialog over a recording transport:
// the copy, the request it sends (or does not), the notice, and the
// close/navigation callback.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ArchiveWorkItemDialog } from "./ArchiveWorkItemDialog.js";
import { DeleteWorkItemDialog } from "./DeleteWorkItemDialog.js";
import { MoveWorkItemDialog } from "./MoveWorkItemDialog.js";
import {
  ARCHIVE_SUCCESS_TITLE,
  DELETE_PERMISSION_TITLE,
  DELETE_SUCCESS_TITLE,
  MOVE_EMPTY_NOTE,
  MOVE_SEARCH_PLACEHOLDER,
  MOVE_SUCCESS_BODY,
} from "./rules.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const TARGET = "323e4567-e89b-12d3-a456-426614174002";
const ISSUE = "123e4567-e89b-12d3-a456-42661417400a";
const SCOPE = { workspaceSlug: SLUG, projectId: PROJECT, issueId: ISSUE };

/** A contract-valid row as the move endpoint answers it. */
function movedRow() {
  return {
    id: ISSUE,
    name: "Moved record",
    state_id: null,
    sort_order: 65535,
    completed_at: null,
    priority: "none",
    complexity_score: 0,
    sequence_id: 9,
    project_id: TARGET,
    parent_id: null,
    agent_executor: null,
    created_at: "2026-03-01T00:00:00Z",
    updated_at: "2026-03-01T00:00:00Z",
    created_by: "user-1",
    updated_by: "user-1",
    is_draft: false,
    archived_at: null,
    is_synced: true,
  };
}

interface StubOptions {
  status?: number;
  body?: unknown;
  moveStatus?: number;
  moveBody?: unknown;
  /** Holds the write until the returned release runs. */
  hold?: { release: () => void };
}

function stubTransport(options: StubOptions = {}) {
  const calls: Array<{ url: string; method: string; body: string }> = [];
  let held: Promise<void> | null = null;
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    if (options.hold !== undefined && init.method !== "GET" && !url.endsWith("/auth/get-csrf-token/")) {
      held = new Promise<void>((resolve) => (options.hold!.release = resolve));
      await held;
    }
    const json = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
    if (url.includes("/move/")) {
      return json(options.moveStatus ?? 200, options.moveBody ?? movedRow());
    }
    if (init.method === "DELETE") return json(options.status ?? 204, options.body ?? {});
    if (init.method === "POST" && url.endsWith("/archive/")) {
      return json(options.status ?? 200, options.body ?? { archived_at: "2026-03-01" });
    }
    return json(options.status ?? 200, {});
  }) as never);
  return { calls };
}

function renderDialog(node: React.ReactElement) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={queryClient}>{node as never}</QueryClientProvider>);
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("ArchiveWorkItemDialog", () => {
  it("names the record, says it can be restored and archives on confirm", async () => {
    const user = userEvent.setup();
    const { calls } = stubTransport();
    const onOpenChange = vi.fn();
    const onArchived = vi.fn();
    renderDialog(
      <ArchiveWorkItemDialog
        open
        onOpenChange={onOpenChange}
        scope={SCOPE}
        projectIdentifier="WEB"
        sequenceId={42}
        onArchived={onArchived}
      />
    );
    expect(screen.getByRole("dialog")).toHaveTextContent("Archive work item WEB 42");
    expect(screen.getByRole("dialog")).toHaveTextContent("restored later");
    await user.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(ARCHIVE_SUCCESS_TITLE, expect.any(String)));
    const post = calls.find((call) => call.method === "POST" && call.url.endsWith("/archive/"));
    expect(post?.url).toBe(`https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/issues/${ISSUE}/archive/`);
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onArchived).toHaveBeenCalledTimes(1);
  });

  it("names the sub-work-item kind", () => {
    stubTransport();
    renderDialog(
      <ArchiveWorkItemDialog
        open
        onOpenChange={vi.fn()}
        scope={SCOPE}
        projectIdentifier="WEB"
        sequenceId={7}
        kind="sub_issue"
      />
    );
    expect(screen.getByRole("dialog")).toHaveTextContent("Archive sub-work item WEB 7");
  });

  it("keeps the dialog open and notices a refused archive", async () => {
    const user = userEvent.setup();
    stubTransport({ status: 400, body: { error: "Only completed or cancelled work items can be archived" } });
    const onOpenChange = vi.fn();
    renderDialog(
      <ArchiveWorkItemDialog open onOpenChange={onOpenChange} scope={SCOPE} projectIdentifier="WEB" sequenceId={42} />
    );
    await user.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("sends one archive while the first is still in flight", async () => {
    const user = userEvent.setup();
    const hold: { release: () => void } = { release: () => {} };
    const { calls } = stubTransport({ hold });
    renderDialog(
      <ArchiveWorkItemDialog open onOpenChange={vi.fn()} scope={SCOPE} projectIdentifier="WEB" sequenceId={42} />
    );
    const confirm = screen.getByRole("button", { name: "Archive" });
    await user.click(confirm);
    await waitFor(() => expect(screen.getByRole("button", { name: "Archiving…" })).toBeDisabled());
    await user.click(screen.getByRole("button", { name: "Archiving…" }));
    expect(calls.filter((call) => call.url.endsWith("/archive/"))).toHaveLength(1);
    hold.release();
  });

  it("closes on cancel without a request", async () => {
    const user = userEvent.setup();
    const { calls } = stubTransport();
    const onOpenChange = vi.fn();
    renderDialog(
      <ArchiveWorkItemDialog open onOpenChange={onOpenChange} scope={SCOPE} projectIdentifier="WEB" sequenceId={42} />
    );
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/archive/"))).toBe(false);
  });
});

describe("DeleteWorkItemDialog", () => {
  const base = {
    open: true,
    scope: SCOPE,
    projectIdentifier: "WEB",
    sequenceId: 9,
  } as const;

  it("warns the removal is permanent and deletes for the creator", async () => {
    const user = userEvent.setup();
    const { calls } = stubTransport();
    const onDeleted = vi.fn();
    renderDialog(
      <DeleteWorkItemDialog
        {...base}
        onOpenChange={vi.fn()}
        viewerId="user-1"
        creatorId="user-1"
        projectRole={15}
        onDeleted={onDeleted}
      />
    );
    expect(screen.getByRole("dialog")).toHaveTextContent("Delete work item");
    expect(screen.getByRole("dialog")).toHaveTextContent("WEB-9");
    expect(screen.getByRole("dialog")).toHaveTextContent("permanently removed");
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(notifySuccess).toHaveBeenCalledWith(DELETE_SUCCESS_TITLE, "work item deleted successfully")
    );
    const del = calls.find((call) => call.method === "DELETE");
    expect(del?.url).toBe(`https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/issues/${ISSUE}/`);
    expect(onDeleted).toHaveBeenCalledTimes(1);
  });

  it("uses the epic noun in copy and success", async () => {
    const user = userEvent.setup();
    stubTransport();
    renderDialog(
      <DeleteWorkItemDialog
        {...base}
        kind="epic"
        onOpenChange={vi.fn()}
        viewerId="admin"
        creatorId="someone"
        projectRole={20}
      />
    );
    expect(screen.getByRole("dialog")).toHaveTextContent("Delete epic");
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(DELETE_SUCCESS_TITLE, "epic deleted successfully"));
  });

  it("refuses a member who is not the creator without sending a request", async () => {
    const user = userEvent.setup();
    const { calls } = stubTransport();
    const onOpenChange = vi.fn();
    renderDialog(
      <DeleteWorkItemDialog
        {...base}
        onOpenChange={onOpenChange}
        viewerId="user-2"
        creatorId="user-1"
        projectRole={15}
      />
    );
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(notifyError).toHaveBeenCalledWith(DELETE_PERMISSION_TITLE);
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(calls.some((call) => call.method === "DELETE")).toBe(false);
  });

  it("maps a server permission refusal to the permission notice", async () => {
    const user = userEvent.setup();
    stubTransport({ status: 403, body: { error: "Only admin or creator can delete the work item" } });
    renderDialog(
      <DeleteWorkItemDialog {...base} onOpenChange={vi.fn()} viewerId="admin" creatorId="someone" projectRole={20} />
    );
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(DELETE_PERMISSION_TITLE));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("sends one delete while the first is still in flight", async () => {
    const user = userEvent.setup();
    const hold: { release: () => void } = { release: () => {} };
    const { calls } = stubTransport({ hold });
    renderDialog(
      <DeleteWorkItemDialog {...base} onOpenChange={vi.fn()} viewerId="user-1" creatorId="user-1" projectRole={15} />
    );
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Deleting…" })).toBeDisabled());
    await user.click(screen.getByRole("button", { name: "Deleting…" }));
    expect(calls.filter((call) => call.method === "DELETE")).toHaveLength(1);
    hold.release();
  });

  it("notices an ordinary server failure without the permission wording", async () => {
    const user = userEvent.setup();
    stubTransport({ status: 500, body: { error: "server is on fire" } });
    renderDialog(
      <DeleteWorkItemDialog {...base} onOpenChange={vi.fn()} viewerId="user-1" creatorId="user-1" projectRole={15} />
    );
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(expect.anything(), expect.stringContaining("server is on fire"))
    );
    expect(notifyError).not.toHaveBeenCalledWith(DELETE_PERMISSION_TITLE);
  });
});

describe("MoveWorkItemDialog", () => {
  const projects = [
    { id: PROJECT, name: "Parity Project", identifier: "PAR" },
    { id: TARGET, name: "Second Home", identifier: "SEC" },
  ];

  it("excludes the current project and filters on search", async () => {
    const user = userEvent.setup();
    stubTransport();
    renderDialog(
      <MoveWorkItemDialog
        open
        onOpenChange={vi.fn()}
        scope={SCOPE}
        projects={projects}
        identifierOf={(id) => (id === TARGET ? "SEC" : undefined)}
      />
    );
    expect(screen.getByRole("dialog")).toHaveTextContent("Move to project");
    expect(screen.getByPlaceholderText(MOVE_SEARCH_PLACEHOLDER)).toBeInTheDocument();
    expect(screen.queryByText("Parity Project")).not.toBeInTheDocument();
    expect(screen.getByText("Second Home")).toBeInTheDocument();
    await user.type(screen.getByPlaceholderText(MOVE_SEARCH_PLACEHOLDER), "zzz");
    expect(screen.getByText(MOVE_EMPTY_NOTE)).toBeInTheDocument();
    await user.clear(screen.getByPlaceholderText(MOVE_SEARCH_PLACEHOLDER));
    expect(screen.getByText("Second Home")).toBeInTheDocument();
  });

  it("renders the empty note when no other project exists", () => {
    stubTransport();
    renderDialog(
      <MoveWorkItemDialog
        open
        onOpenChange={vi.fn()}
        scope={SCOPE}
        projects={[projects[0]!]}
        identifierOf={() => undefined}
      />
    );
    expect(screen.getByText(MOVE_EMPTY_NOTE)).toBeInTheDocument();
  });

  it("moves the record, toasts and navigates to its new address", async () => {
    const user = userEvent.setup();
    const { calls } = stubTransport();
    const navigate = vi.fn();
    const onOpenChange = vi.fn();
    renderDialog(
      <MoveWorkItemDialog
        open
        onOpenChange={onOpenChange}
        scope={SCOPE}
        projects={projects}
        identifierOf={(id) => (id === TARGET ? "SEC" : "PAR")}
        navigate={navigate}
      />
    );
    await user.click(screen.getByRole("button", { name: /Second Home/ }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(expect.anything(), MOVE_SUCCESS_BODY));
    const post = calls.find((call) => call.url.includes("/move/"));
    expect(post?.url).toBe(`https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/work-items/${ISSUE}/move/`);
    expect(JSON.parse(post?.body ?? "{}")).toEqual({ project: TARGET });
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(navigate).toHaveBeenCalledWith(`/${SLUG}/browse/SEC-9/`);
  });

  it("navigates the window when no router callback is given", async () => {
    const user = userEvent.setup();
    stubTransport();
    const assign = vi.fn();
    const original = globalThis.location;
    Object.defineProperty(globalThis, "location", { value: { ...original, assign }, configurable: true });
    try {
      renderDialog(
        <MoveWorkItemDialog
          open
          onOpenChange={vi.fn()}
          scope={SCOPE}
          projects={projects}
          identifierOf={(id) => (id === TARGET ? "SEC" : "PAR")}
        />
      );
      await user.click(screen.getByRole("button", { name: /Second Home/ }));
      await waitFor(() => expect(assign).toHaveBeenCalledWith(`/${SLUG}/browse/SEC-9/`));
    } finally {
      Object.defineProperty(globalThis, "location", { value: original, configurable: true });
    }
  });

  it("sends one move while the first is still in flight", async () => {
    const user = userEvent.setup();
    const hold: { release: () => void } = { release: () => {} };
    const { calls } = stubTransport({ hold });
    renderDialog(
      <MoveWorkItemDialog
        open
        onOpenChange={vi.fn()}
        scope={SCOPE}
        projects={projects}
        identifierOf={(id) => (id === TARGET ? "SEC" : "PAR")}
        navigate={vi.fn()}
      />
    );
    const target = screen.getByRole("button", { name: /Second Home/ });
    await user.click(target);
    await waitFor(() => expect(target).toBeDisabled());
    await user.click(target);
    expect(calls.filter((call) => call.url.includes("/move/"))).toHaveLength(1);
    hold.release();
  });

  it("still navigates when the target code is unknown", async () => {
    const user = userEvent.setup();
    stubTransport();
    const navigate = vi.fn();
    renderDialog(
      <MoveWorkItemDialog
        open
        onOpenChange={vi.fn()}
        scope={SCOPE}
        projects={projects}
        identifierOf={() => undefined}
        navigate={navigate}
      />
    );
    await user.click(screen.getByRole("button", { name: /Second Home/ }));
    await waitFor(() => expect(navigate).toHaveBeenCalledWith(`/${SLUG}/browse/-9/`));
  });

  it("keeps the list open and notices a refused move", async () => {
    const user = userEvent.setup();
    stubTransport({ moveStatus: 400, moveBody: { error: "Projects must share a workspace" } });
    const navigate = vi.fn();
    const onOpenChange = vi.fn();
    renderDialog(
      <MoveWorkItemDialog
        open
        onOpenChange={onOpenChange}
        scope={SCOPE}
        projects={projects}
        identifierOf={(id) => (id === TARGET ? "SEC" : "PAR")}
        navigate={navigate}
      />
    );
    await user.click(screen.getByRole("button", { name: /Second Home/ }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(navigate).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("clears the search when it closes", async () => {
    const user = userEvent.setup();
    stubTransport();
    const { rerender } = renderDialog(
      <MoveWorkItemDialog
        open
        onOpenChange={vi.fn()}
        scope={SCOPE}
        projects={projects}
        identifierOf={() => undefined}
      />
    );
    await user.type(screen.getByPlaceholderText(MOVE_SEARCH_PLACEHOLDER), "sec");
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    rerender(
      <QueryClientProvider client={queryClient}>
        <MoveWorkItemDialog
          open={false}
          onOpenChange={vi.fn()}
          scope={SCOPE}
          projects={projects}
          identifierOf={() => undefined}
        />
      </QueryClientProvider>
    );
    rerender(
      <QueryClientProvider client={queryClient}>
        <MoveWorkItemDialog
          open
          onOpenChange={vi.fn()}
          scope={SCOPE}
          projects={projects}
          identifierOf={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByPlaceholderText(MOVE_SEARCH_PLACEHOLDER)).toHaveValue("");
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Triage writes (INT-020–023, 025): the status-move and delete
// envelopes with notice and revalidate, the duplicate-target search,
// and the controls hook's dialogs, refusals, accept hand-off and
// advance wiring.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import {
  INTAKE_STATUS,
  type IntakeIssueDetail,
  type IntakeIssueDetailItem,
  type IssueSearchHit,
  type TransportResponse,
} from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { issueKeys } from "../../issues/index.js";
import { intakeKeys } from "./keys.js";
import {
  ACCEPT_FAILED_TITLE,
  DELETE_FAILED_TITLE,
  DELETE_SUCCESS_TITLE,
  excludeCurrentTarget,
  invalidateTriageCaches,
  useDeleteIntakeRequest,
  useDuplicateTargetSearch,
  useTriageControls,
  useTriageStatusMove,
  type TriageControlsScope,
} from "./triage.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "11111111-1111-4111-8111-111111111111";
const TARGET = "22222222-2222-4222-8222-222222222222";
const DETAIL_PATH = `/api/workspaces/${WS}/projects/${PROJECT}/intake-issues/${ISSUE}/`;
const SEARCH_PATH = `/api/workspaces/${WS}/projects/${PROJECT}/search-issues/`;

function detailItem(overrides: Partial<IntakeIssueDetailItem> = {}): IntakeIssueDetailItem {
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
    ...overrides,
  };
}

function seedDetail(item: IntakeIssueDetailItem, status = INTAKE_STATUS.pending): IntakeIssueDetail {
  return {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-000000000001",
    status,
    duplicate_to: null,
    snoozed_till: null,
    duplicate_issue_detail: null,
    source: null,
    issue: item,
  };
}

function hit(overrides: Partial<IssueSearchHit> = {}): IssueSearchHit {
  return {
    name: "A work item",
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
    ...overrides,
  };
}

function response(url: string, status: number, body: unknown): TransportResponse {
  return {
    status,
    url,
    headers: { get: () => null },
    text: async () => (body === undefined ? "" : JSON.stringify(body)),
  } as TransportResponse;
}

interface TriageServer {
  patches: unknown[];
  deletes: number;
  searches: string[];
  patchStatus: number;
  deleteStatus: number;
  searchStatus: number;
  refuse: unknown;
  hits: IssueSearchHit[];
}

function stubTransport(server: TriageServer): void {
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
    const method = init?.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) return response(url, 200, { csrf_token: "csrf" });
    if (url.includes(DETAIL_PATH) && method === "PATCH") {
      server.patches.push(init?.body === undefined ? null : JSON.parse(init.body));
      if (server.patchStatus >= 400) return response(url, server.patchStatus, server.refuse);
      return response(url, server.patchStatus, seedDetail(detailItem()));
    }
    if (url.includes(DETAIL_PATH) && method === "DELETE") {
      server.deletes += 1;
      if (server.deleteStatus >= 400) return response(url, server.deleteStatus, server.refuse);
      return response(url, server.deleteStatus, undefined);
    }
    if (url.includes(SEARCH_PATH) && method === "GET") {
      server.searches.push(url);
      if (server.searchStatus >= 400) return response(url, server.searchStatus, server.refuse);
      return response(url, server.searchStatus, server.hits);
    }
    throw new Error(`unexpected ${method} ${url}`);
  }) as never);
}

function freshServer(): TriageServer {
  return {
    patches: [],
    deletes: 0,
    searches: [],
    patchStatus: 200,
    deleteStatus: 204,
    searchStatus: 200,
    refuse: { error: "refused" },
    hits: [],
  };
}

function freshClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
}

function wrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

function seedCaches(queryClient: QueryClient): void {
  queryClient.setQueryData(intakeKeys.detail(WS, PROJECT, ISSUE), seedDetail(detailItem()));
  queryClient.setQueryData(intakeKeys.lists(WS, PROJECT), { pages: [], pageParams: [] });
  queryClient.setQueryData(issueKeys.descriptionVersions(WS, PROJECT, ISSUE), []);
}

function invalidated(queryClient: QueryClient, key: readonly unknown[]): boolean | undefined {
  return queryClient.getQueryState(key as never)?.isInvalidated;
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
  vi.resetAllMocks();
});

describe("invalidateTriageCaches", () => {
  it("revalidates the tab lists and the detail", async () => {
    const queryClient = freshClient();
    seedCaches(queryClient);
    await invalidateTriageCaches(queryClient, WS, PROJECT, ISSUE);
    expect(invalidated(queryClient, intakeKeys.lists(WS, PROJECT))).toBe(true);
    expect(invalidated(queryClient, intakeKeys.detail(WS, PROJECT, ISSUE))).toBe(true);
    expect(invalidated(queryClient, issueKeys.descriptionVersions(WS, PROJECT, ISSUE))).toBe(false);
  });

  it("also revalidates the versions list when the description changed", async () => {
    const queryClient = freshClient();
    seedCaches(queryClient);
    await invalidateTriageCaches(queryClient, WS, PROJECT, ISSUE, { descriptionChanged: true });
    expect(invalidated(queryClient, issueKeys.descriptionVersions(WS, PROJECT, ISSUE))).toBe(true);
  });
});

describe("useTriageStatusMove", () => {
  function renderMove(queryClient: QueryClient) {
    return renderHook(
      () =>
        useTriageStatusMove({
          workspaceSlug: WS,
          projectId: PROJECT,
          issueId: ISSUE,
          successTitle: "Moved",
          failureTitle: "Couldn't move",
        }),
      { wrapper: wrapper(queryClient) }
    );
  }

  it("sends the intake envelope without optimistic paint, then toasts and revalidates", async () => {
    const queryClient = freshClient();
    seedCaches(queryClient);
    const server = freshServer();
    stubTransport(server);
    const { result } = renderMove(queryClient);

    let settled!: IntakeIssueDetail;
    await React.act(async () => {
      settled = await result.current.mutateAsync({ status: INTAKE_STATUS.declined });
    });

    expect(server.patches).toEqual([{ status: INTAKE_STATUS.declined }]);
    expect(settled.status).toBe(INTAKE_STATUS.pending);
    expect(notifySuccess).toHaveBeenCalledWith("Moved");
    expect(invalidated(queryClient, intakeKeys.lists(WS, PROJECT))).toBe(true);
    expect(invalidated(queryClient, intakeKeys.detail(WS, PROJECT, ISSUE))).toBe(true);
  });

  it("notices when the server refuses, keeping the cache", async () => {
    const queryClient = freshClient();
    const seeded = seedDetail(detailItem());
    queryClient.setQueryData(intakeKeys.detail(WS, PROJECT, ISSUE), seeded);
    const server = freshServer();
    server.patchStatus = 403;
    stubTransport(server);
    const { result } = renderMove(queryClient);

    await React.act(async () => {
      await expect(result.current.mutateAsync({ status: INTAKE_STATUS.accepted })).rejects.toThrow();
    });

    expect(notifyError).toHaveBeenCalledWith("Couldn't move", expect.any(String));
    expect(queryClient.getQueryData(intakeKeys.detail(WS, PROJECT, ISSUE))).toEqual(seeded);
  });
});

describe("useDeleteIntakeRequest", () => {
  function renderDelete(queryClient: QueryClient) {
    return renderHook(() => useDeleteIntakeRequest({ workspaceSlug: WS, projectId: PROJECT, issueId: ISSUE }), {
      wrapper: wrapper(queryClient),
    });
  }

  it("deletes, toasts and revalidates", async () => {
    const queryClient = freshClient();
    seedCaches(queryClient);
    const server = freshServer();
    stubTransport(server);
    const { result } = renderDelete(queryClient);

    let status!: number;
    await React.act(async () => {
      status = await result.current.mutateAsync(undefined);
    });

    expect(server.deletes).toBe(1);
    expect(status).toBe(204);
    expect(notifySuccess).toHaveBeenCalledWith(DELETE_SUCCESS_TITLE);
    expect(invalidated(queryClient, intakeKeys.lists(WS, PROJECT))).toBe(true);
    expect(invalidated(queryClient, intakeKeys.detail(WS, PROJECT, ISSUE))).toBe(true);
  });

  it("notices when the server refuses", async () => {
    const queryClient = freshClient();
    const server = freshServer();
    server.deleteStatus = 403;
    stubTransport(server);
    const { result } = renderDelete(queryClient);

    await React.act(async () => {
      await expect(result.current.mutateAsync(undefined)).rejects.toThrow();
    });

    expect(notifyError).toHaveBeenCalledWith(DELETE_FAILED_TITLE, expect.any(String));
  });
});

describe("useDuplicateTargetSearch", () => {
  function renderSearch(queryClient: QueryClient, term: string, enabled = true) {
    return renderHook(() => useDuplicateTargetSearch(WS, PROJECT, term, enabled), {
      wrapper: wrapper(queryClient),
    });
  }

  it("lists recent project items on an empty search", async () => {
    const queryClient = freshClient();
    const server = freshServer();
    server.hits = [hit(), hit({ id: "33333333-3333-4333-8333-333333333333", name: "Second" })];
    stubTransport(server);
    const { result } = renderSearch(queryClient, "");

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toHaveLength(2);
    expect(server.searches).toHaveLength(1);
    expect(server.searches[0]).toContain("workspace_search=false");
    expect(server.searches[0]).not.toMatch(/[?&]search=/);
  });

  it("sends the typed term project-scoped", async () => {
    const queryClient = freshClient();
    const server = freshServer();
    server.hits = [hit({ name: "Target match" })];
    stubTransport(server);
    const { result } = renderSearch(queryClient, "target");

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(server.searches[0]).toContain("search=target");
    expect(server.searches[0]).toContain("workspace_search=false");
  });

  it("reads nothing while disabled", async () => {
    const queryClient = freshClient();
    const server = freshServer();
    stubTransport(server);
    const { result } = renderSearch(queryClient, "", false);

    expect(result.current.isPending).toBe(true);
    expect(result.current.fetchStatus).toBe("idle");
    expect(server.searches).toHaveLength(0);
  });
});

describe("excludeCurrentTarget", () => {
  it("drops the acted request's own work item", () => {
    const hits = [hit({ id: ISSUE, name: "Own request" }), hit()];
    expect(excludeCurrentTarget(hits, ISSUE).map((row) => row.id)).toEqual([TARGET]);
  });
});

describe("useTriageControls", () => {
  function triageScope(overrides: Partial<TriageControlsScope> = {}): TriageControlsScope {
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
      onStay: vi.fn(),
      ...overrides,
    };
  }

  function renderControls(queryClient: QueryClient, current: TriageControlsScope) {
    return renderHook(() => useTriageControls(current), { wrapper: wrapper(queryClient) });
  }

  function labels(entries: readonly unknown[]): unknown[] {
    return entries.map((entry) => (entry as { label?: unknown }).label ?? null);
  }

  function select(entries: readonly unknown[], index: number): void {
    const entry = entries[index] as { onSelect: () => void };
    React.act(() => {
      entry.onSelect();
    });
  }

  it("exposes the admin entries and accept hand-off on an open request", () => {
    const queryClient = freshClient();
    stubTransport(freshServer());
    const current = triageScope();
    const { result } = renderControls(queryClient, current);

    expect(result.current.visibility).toEqual({ showTriage: true, showDelete: true });
    expect(labels(result.current.leadEntries)).toEqual(["Snooze", "Mark as duplicate"]);
    expect(labels(result.current.trailEntries)).toEqual(["Delete"]);
    expect(result.current.openDialog).toBeNull();
    expect(result.current.acceptTitle).toBe("Add APO-7 to project work items");
    expect(result.current.acceptTarget).toMatchObject({ kind: "issue", id: ISSUE, projectId: PROJECT });
    expect(result.current.acceptTarget.snapshot).toEqual(detailItem());
    expect(typeof result.current.acceptTarget.onChanged).toBe("function");
    expect(result.current.snoozeInitialDay).toBeNull();
    expect(result.current.deleteAccepted).toBe(false);
  });

  it("swaps snooze for un-snooze and seeds the picker on a snoozed request", () => {
    const queryClient = freshClient();
    stubTransport(freshServer());
    const at = new Date(2026, 9, 31, 0, 0, 0).toISOString();
    const { result } = renderControls(queryClient, triageScope({ status: INTAKE_STATUS.snoozed, snoozedTill: at }));

    expect(labels(result.current.leadEntries)).toEqual(["Un snooze", "Mark as duplicate"]);
    expect(result.current.snoozeInitialDay).toBe("2026-10-31");
  });

  it("omits delete for a member on someone else's request", () => {
    const queryClient = freshClient();
    stubTransport(freshServer());
    const { result } = renderControls(queryClient, triageScope({ isAdmin: false }));

    expect(result.current.trailEntries).toEqual([]);
  });

  it("opens dialogs for an admin and refuses a member with a notice", () => {
    const adminClient = freshClient();
    stubTransport(freshServer());
    const admin = renderControls(adminClient, triageScope());
    select(admin.result.current.leadEntries, 0);
    expect(admin.result.current.openDialog).toBe("snooze");
    React.act(() => {
      admin.result.current.setOpenDialog(null);
    });
    expect(admin.result.current.openDialog).toBeNull();

    const memberClient = freshClient();
    const member = renderControls(memberClient, triageScope({ isAdmin: false }));
    React.act(() => {
      member.result.current.fireAction("accept");
    });
    expect(notifyError).toHaveBeenCalledWith("Permission denied", expect.stringContaining("admin"));
    expect(member.result.current.openDialog).toBeNull();
    select(member.result.current.leadEntries, 1);
    expect(member.result.current.openDialog).toBeNull();
    expect(notifyError).toHaveBeenCalledTimes(2);
  });

  it("un-snoozes directly, advancing past the restored request", async () => {
    const queryClient = freshClient();
    seedCaches(queryClient);
    const server = freshServer();
    stubTransport(server);
    const current = triageScope({ status: INTAKE_STATUS.snoozed });
    const { result } = renderControls(queryClient, current);

    await React.act(async () => {
      result.current.fireAction("unsnooze");
    });

    expect(server.patches).toEqual([{ status: INTAKE_STATUS.pending, snoozed_till: null }]);
    expect(notifySuccess).toHaveBeenCalledWith("Snooze removed");
    expect(current.onAdvance).toHaveBeenCalledWith(ISSUE);
    expect(result.current.openDialog).toBeNull();
  });

  it("refuses a member un-snooze without writing", async () => {
    const queryClient = freshClient();
    const server = freshServer();
    stubTransport(server);
    const { result } = renderControls(queryClient, triageScope({ isAdmin: false }));

    await React.act(async () => {
      result.current.fireAction("unsnooze");
    });

    expect(notifyError).toHaveBeenCalledWith("Permission denied", expect.stringContaining("admin"));
    expect(server.patches).toEqual([]);
  });

  it("writes the accept before the form save and re-reads when the save fails", async () => {
    const queryClient = freshClient();
    seedCaches(queryClient);
    const server = freshServer();
    stubTransport(server);
    const { result } = renderControls(queryClient, triageScope());

    await React.act(async () => {
      await result.current.runAcceptWrite();
    });
    expect(server.patches).toEqual([{ status: INTAKE_STATUS.accepted }]);

    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    React.act(() => {
      result.current.handleAcceptClosed();
    });
    // Committed but unsaved: the detail re-reads the accepted state.
    expect(invalidate).toHaveBeenCalled();
  });

  it("keeps the form open with the error when the accept write fails", async () => {
    const queryClient = freshClient();
    seedCaches(queryClient);
    const server = freshServer();
    server.patchStatus = 400;
    stubTransport(server);
    const { result } = renderControls(queryClient, triageScope());

    await React.act(async () => {
      await expect(result.current.runAcceptWrite()).rejects.toThrow();
    });
    expect(notifyError).toHaveBeenCalledWith(ACCEPT_FAILED_TITLE, expect.any(String));

    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    React.act(() => {
      result.current.handleAcceptClosed();
    });
    // Nothing committed: closing re-reads nothing.
    expect(invalidate).not.toHaveBeenCalled();
  });

  it("advances once on an accept save and ignores the closing echo", async () => {
    const queryClient = freshClient();
    seedCaches(queryClient);
    stubTransport(freshServer());
    const current = triageScope();
    const { result } = renderControls(queryClient, current);
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    React.act(() => {
      result.current.handleAcceptSaved(["name", "description_html"]);
    });

    expect(current.onAdvance).toHaveBeenCalledWith(ISSUE);
    expect(result.current.openDialog).toBeNull();
    expect(invalidate).toHaveBeenCalled();
    expect(invalidated(queryClient, issueKeys.descriptionVersions(WS, PROJECT, ISSUE))).toBe(true);

    invalidate.mockClear();
    React.act(() => {
      result.current.handleAcceptClosed();
    });
    expect(invalidate).not.toHaveBeenCalled();
    expect(current.onAdvance).toHaveBeenCalledTimes(1);
  });

  it("declines through the dialog and advances", async () => {
    const queryClient = freshClient();
    seedCaches(queryClient);
    const server = freshServer();
    stubTransport(server);
    const current = triageScope();
    const { result } = renderControls(queryClient, current);

    React.act(() => {
      result.current.fireAction("decline");
    });
    expect(result.current.openDialog).toBe("decline");
    expect(result.current.declinePending).toBe(false);

    await React.act(async () => {
      result.current.confirmDecline();
    });

    expect(server.patches).toEqual([{ status: INTAKE_STATUS.declined }]);
    expect(notifySuccess).toHaveBeenCalledWith("Request declined");
    expect(current.onAdvance).toHaveBeenCalledWith(ISSUE);
    expect(result.current.openDialog).toBeNull();
  });

  it("snoozes a picked day as its midnight and advances", async () => {
    const queryClient = freshClient();
    seedCaches(queryClient);
    const server = freshServer();
    stubTransport(server);
    const current = triageScope();
    const { result } = renderControls(queryClient, current);

    await React.act(async () => {
      result.current.confirmSnooze("2026-10-31");
    });

    expect(server.patches).toEqual([
      { status: INTAKE_STATUS.snoozed, snoozed_till: new Date(2026, 9, 31).toISOString() },
    ]);
    expect(notifySuccess).toHaveBeenCalledWith("Request snoozed");
    expect(current.onAdvance).toHaveBeenCalledWith(ISSUE);
  });

  it("ignores a malformed snooze day", async () => {
    const queryClient = freshClient();
    const server = freshServer();
    stubTransport(server);
    const { result } = renderControls(queryClient, triageScope());

    await React.act(async () => {
      result.current.confirmSnooze("not-a-day");
    });

    expect(server.patches).toEqual([]);
  });

  it("marks duplicate and stays on the request", async () => {
    const queryClient = freshClient();
    seedCaches(queryClient);
    const server = freshServer();
    stubTransport(server);
    const current = triageScope();
    const { result } = renderControls(queryClient, current);

    React.act(() => {
      result.current.fireAction("duplicate");
    });
    expect(result.current.openDialog).toBe("duplicate");

    await React.act(async () => {
      result.current.pickDuplicateTarget(TARGET);
    });

    expect(server.patches).toEqual([{ status: INTAKE_STATUS.duplicate, duplicate_to: TARGET }]);
    expect(notifySuccess).toHaveBeenCalledWith("Marked as duplicate");
    expect(current.onAdvance).not.toHaveBeenCalled();
    expect(current.onStay).toHaveBeenCalledWith(ISSUE);
    expect(result.current.openDialog).toBeNull();
  });

  it("deletes and advances", async () => {
    const queryClient = freshClient();
    seedCaches(queryClient);
    const server = freshServer();
    stubTransport(server);
    const current = triageScope();
    const { result } = renderControls(queryClient, current);

    React.act(() => {
      result.current.fireAction("delete");
    });
    expect(result.current.openDialog).toBe("delete");

    await React.act(async () => {
      result.current.confirmDelete();
    });

    expect(server.deletes).toBe(1);
    expect(current.onAdvance).toHaveBeenCalledWith(ISSUE);
    expect(result.current.openDialog).toBeNull();
  });
});

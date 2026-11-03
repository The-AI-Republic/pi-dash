// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reaction data (ISS-203/204): reads, both toggle directions, the notices
// and the query keys each surface refetches.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { CommentReaction, IssueReaction, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { issueKeys } from "../api/keys.js";
import {
  REACTION_CREATED_TITLE,
  REACTION_CREATE_FAILED_TITLE,
  REACTION_FAILURE_FALLBACK,
  REACTION_REMOVE_FAILED_TITLE,
  REACTION_REMOVED_TITLE,
  commentReactionsQueryOptions,
  issueReactionsQueryOptions,
  reactionFailureDetail,
  useCommentReactions,
  useIssueReactions,
  useToggleCommentReaction,
  useToggleIssueReaction,
} from "./api.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const COMMENT = "323e4567-e89b-12d3-a456-426614174002";
const ADA = "523e4567-e89b-12d3-a456-426614174004";
const SCOPE = { workspaceSlug: SLUG, projectId: PROJECT, issueId: ISSUE };

function issueReaction(overrides: Partial<IssueReaction> = {}): IssueReaction {
  return {
    id: "623e4567-e89b-12d3-a456-426614174006",
    actor: ADA,
    actor_detail: {
      id: ADA,
      first_name: "Ada",
      last_name: "L",
      avatar: "",
      avatar_url: null,
      display_name: "Ada L",
      is_bot: false,
    },
    issue: ISSUE,
    reaction: "128077",
    project: PROJECT,
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    created_at: "2026-10-16T10:00:00Z",
    updated_at: "2026-10-16T10:00:00Z",
    created_by: ADA,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

function commentReaction(overrides: Partial<CommentReaction> = {}): CommentReaction {
  return {
    id: "723e4567-e89b-12d3-a456-426614174007",
    actor: ADA,
    comment: COMMENT,
    reaction: "127881",
    display_name: "Ada L",
    project: PROJECT,
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    created_at: "2026-10-16T10:00:00Z",
    updated_at: "2026-10-16T10:00:00Z",
    created_by: ADA,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

interface Recorded {
  method: string;
  url: string;
  body: string;
}

function stubClient(handler: (call: Recorded) => { status: number; body: unknown }) {
  const calls: Recorded[] = [];
  const client = createAppClient("https://api.test", async (url: string, init: TransportInit) => {
    const call = { method: init.method, url, body: init.body ?? "" };
    calls.push(call);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "x" }) };
    }
    const answer = handler(call);
    return {
      status: answer.status,
      url,
      headers: { get: () => null },
      text: async () => (answer.body === "" ? "" : JSON.stringify(answer.body)),
    };
  });
  return { client, calls };
}

function harness() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const wrapper = function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return (
      <QueryClientProvider client={queryClient}>
        <React.Fragment>{children as React.ReactNode}</React.Fragment>
      </QueryClientProvider>
    );
  };
  return { queryClient, wrapper };
}

beforeEach(() => {
  vi.mocked(notifyError).mockClear();
  vi.mocked(notifySuccess).mockClear();
});

afterEach(() => {
  resetAppClient();
});

describe("reaction reads", () => {
  it("maps issue rows to the reactor's display name", async () => {
    const { client, calls } = stubClient(() => ({ status: 200, body: [issueReaction()] }));
    const options = issueReactionsQueryOptions(client, SCOPE);
    expect(options.queryKey).toEqual(issueKeys.reactions(SLUG, PROJECT, ISSUE));
    const rows = await (options.queryFn as unknown as () => Promise<{ displayName: string }[]>)();
    expect(rows[0]?.displayName).toBe("Ada L");
    expect(calls[0]?.url).toContain(`/issues/${ISSUE}/reactions/`);
  });

  it("maps comment rows from their flat payload", async () => {
    const { client, calls } = stubClient(() => ({ status: 200, body: [commentReaction()] }));
    const options = commentReactionsQueryOptions(client, SCOPE, COMMENT);
    expect(options.queryKey).toEqual(issueKeys.commentReactions(SLUG, PROJECT, COMMENT));
    const rows = await (options.queryFn as unknown as () => Promise<{ displayName: string }[]>)();
    expect(rows[0]?.displayName).toBe("Ada L");
    expect(calls[0]?.url).toContain(`/comments/${COMMENT}/reactions/`);
  });

  it("exposes both hooks against the app client", async () => {
    stubClient((call) =>
      call.url.includes("/comments/")
        ? { status: 200, body: [commentReaction()] }
        : { status: 200, body: [issueReaction()] }
    );
    const { queryClient, wrapper } = harness();
    const issue = renderHook(() => useIssueReactions(SCOPE), { wrapper });
    await waitFor(() => expect(issue.result.current.data).toHaveLength(1));
    const comment = renderHook(() => useCommentReactions(SCOPE, COMMENT), { wrapper });
    await waitFor(() => expect(comment.result.current.data).toHaveLength(1));
    expect(queryClient.getQueryData(issueKeys.reactions(SLUG, PROJECT, ISSUE))).toHaveLength(1);
    expect(queryClient.getQueryData(issueKeys.commentReactions(SLUG, PROJECT, COMMENT))).toHaveLength(1);
  });
});

describe("useToggleIssueReaction", () => {
  it("posts a new reaction, notices and refetches the list", async () => {
    const { calls } = stubClient(() => ({ status: 201, body: issueReaction() }));
    const { queryClient, wrapper } = harness();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useToggleIssueReaction(SCOPE), { wrapper });
    await result.current.mutateAsync({ code: "128077", action: "add" });
    const post = calls.find((call) => call.method === "POST");
    expect(post?.url).toContain(`/issues/${ISSUE}/reactions/`);
    expect(post?.body).toBe(JSON.stringify({ reaction: "128077" }));
    expect(notifySuccess).toHaveBeenCalledWith(REACTION_CREATED_TITLE);
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({ queryKey: issueKeys.reactions(SLUG, PROJECT, ISSUE) })
    );
  });

  it("deletes an existing reaction with its own notice", async () => {
    const { calls } = stubClient(() => ({ status: 204, body: "" }));
    const { wrapper } = harness();
    const { result } = renderHook(() => useToggleIssueReaction(SCOPE), { wrapper });
    await result.current.mutateAsync({ code: "128077", action: "remove" });
    const del = calls.find((call) => call.method === "DELETE");
    expect(del?.url).toContain(`/issues/${ISSUE}/reactions/${encodeURIComponent("128077")}/`);
    expect(notifySuccess).toHaveBeenCalledWith(REACTION_REMOVED_TITLE);
  });

  it("reports a refused write with its server detail", async () => {
    stubClient(() => ({ status: 403, body: { error: "Not allowed." } }));
    const { wrapper } = harness();
    const { result } = renderHook(() => useToggleIssueReaction(SCOPE), { wrapper });
    await result.current.mutateAsync({ code: "128077", action: "add" }).catch(() => undefined);
    expect(notifyError).toHaveBeenCalledWith(REACTION_CREATE_FAILED_TITLE, "Not allowed.");
  });

  it("uses the removal wording when a removal is refused", async () => {
    stubClient(() => ({ status: 500, body: { error: "server fell over" } }));
    const { wrapper } = harness();
    const { result } = renderHook(() => useToggleIssueReaction(SCOPE), { wrapper });
    await result.current.mutateAsync({ code: "128077", action: "remove" }).catch(() => undefined);
    expect(notifyError).toHaveBeenCalledWith(REACTION_REMOVE_FAILED_TITLE, "server fell over");
  });

  it("refuses a duplicate add with one failure notice and re-reads the list", async () => {
    stubClient(() => ({ status: 400, body: { error: "The payload is not valid" } }));
    const { queryClient, wrapper } = harness();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useToggleIssueReaction(SCOPE), { wrapper });
    await result.current.mutateAsync({ code: "128077", action: "add" }).catch(() => undefined);
    expect(notifyError).toHaveBeenCalledTimes(1);
    expect(notifyError).toHaveBeenCalledWith(REACTION_CREATE_FAILED_TITLE, "The payload is not valid");
    expect(notifySuccess).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({ queryKey: issueKeys.reactions(SLUG, PROJECT, ISSUE) })
    );
  });
});

describe("useToggleCommentReaction", () => {
  it("posts and deletes against the comment collection", async () => {
    const { calls } = stubClient((call) =>
      call.method === "POST" ? { status: 201, body: commentReaction() } : { status: 204, body: "" }
    );
    const { queryClient, wrapper } = harness();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useToggleCommentReaction(SCOPE, COMMENT), { wrapper });
    await result.current.mutateAsync({ code: "127881", action: "add" });
    await result.current.mutateAsync({ code: "127881", action: "remove" });
    expect(calls.filter((call) => call.method === "POST")[0]?.url).toContain(`/comments/${COMMENT}/reactions/`);
    expect(calls.find((call) => call.method === "DELETE")?.url).toContain(
      `/comments/${COMMENT}/reactions/${encodeURIComponent("127881")}/`
    );
    expect(notifySuccess).toHaveBeenNthCalledWith(1, REACTION_CREATED_TITLE);
    expect(notifySuccess).toHaveBeenNthCalledWith(2, REACTION_REMOVED_TITLE);
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({ queryKey: issueKeys.commentReactions(SLUG, PROJECT, COMMENT) })
    );
  });

  it("reports a refusal with the comment collection's phrasing", async () => {
    stubClient(() => ({ status: 400, body: { error: "Bad reaction." } }));
    const { wrapper } = harness();
    const { result } = renderHook(() => useToggleCommentReaction(SCOPE, COMMENT), { wrapper });
    await result.current.mutateAsync({ code: "zzz", action: "add" }).catch(() => undefined);
    expect(notifyError).toHaveBeenCalledWith(REACTION_CREATE_FAILED_TITLE, "Bad reaction.");
  });

  it("refuses a duplicate add with one failure notice and re-reads the list", async () => {
    stubClient(() => ({ status: 400, body: { error: "Reaction already exists for the user" } }));
    const { queryClient, wrapper } = harness();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useToggleCommentReaction(SCOPE, COMMENT), { wrapper });
    await result.current.mutateAsync({ code: "127881", action: "add" }).catch(() => undefined);
    expect(notifyError).toHaveBeenCalledTimes(1);
    expect(notifyError).toHaveBeenCalledWith(REACTION_CREATE_FAILED_TITLE, "Reaction already exists for the user");
    expect(notifySuccess).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({ queryKey: issueKeys.commentReactions(SLUG, PROJECT, COMMENT) })
    );
  });
});

describe("reaction failure notices", () => {
  it("matches the old app's copy for both directions", () => {
    expect(REACTION_CREATE_FAILED_TITLE).toBe("Reaction creation failed");
    expect(REACTION_REMOVE_FAILED_TITLE).toBe("Reaction remove failed");
  });
});

describe("reactionFailureDetail", () => {
  it("falls back when the error carries no message", () => {
    expect(reactionFailureDetail(new Error(""))).toBe(REACTION_FAILURE_FALLBACK);
    expect(reactionFailureDetail("plain")).toBe(REACTION_FAILURE_FALLBACK);
  });
});

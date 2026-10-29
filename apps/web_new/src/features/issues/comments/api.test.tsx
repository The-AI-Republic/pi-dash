// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { IssueComment, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import {
  COMMENT_CREATED_TITLE,
  COMMENT_CREATE_FAILED_TITLE,
  COMMENT_IMAGE_UPLOAD_FAILED_TITLE,
  COMMENT_LINK_COPY_FAILED_TITLE,
  COMMENT_REMOVE_FAILED_TITLE,
  COMMENT_REMOVED_TITLE,
  COMMENT_SYNCED_REMOVE_FAILED_TITLE,
  COMMENT_UPDATE_FAILED_TITLE,
  COMMENT_UPDATED_TITLE,
  accessPatch,
  commentFailureDetail,
  commentMentionSearchKey,
  commentsQueryOptions,
  COMMENT_FAILURE_FALLBACK,
  COMMENT_MENTION_LIMIT,
  duplicateKeptCommentImages,
  foldPatch,
  paintComment,
  pendingCommentPatch,
  removeCommentRow,
  requestCommentMentions,
  useCommentMentionSearch,
  useCreateComment,
  useDeleteComment,
  useIssueComments,
  useUpdateComment,
  useUploadCommentImage,
} from "./api.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const COMMENT = "323e4567-e89b-12d3-a456-426614174002";
const ADA = "523e4567-e89b-12d3-a456-426614174004";
const SCOPE = { workspaceSlug: SLUG, projectId: PROJECT, issueId: ISSUE };

function comment(overrides: Partial<IssueComment> = {}): IssueComment {
  return {
    id: COMMENT,
    issue: ISSUE,
    project: PROJECT,
    workspace: "423e4567-e89b-12d3-a456-426614174003",
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
    comment_html: "<p>Hello</p>",
    comment_stripped: "Hello",
    comment_json: {},
    attachments: [],
    labels: [],
    access: "INTERNAL",
    edited_at: null,
    comment_reactions: [],
    speaker_type: "human",
    speaker_label: "",
    speaker_agent_run_id: null,
    parent: null,
    external_source: null,
    external_id: null,
    is_synced: false,
    description: null,
    created_at: "2026-10-16T22:00:00Z",
    updated_at: "2026-10-16T22:00:00Z",
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

/**
 * A transport whose answers the test releases by hand, so a mutation can
 * be observed between `onMutate` and its PATCH resolving. Installs the
 * client the hooks read through `getAppClient` too.
 */
function stubDeferredClient(handler: (call: Recorded) => Promise<{ status: number; body: unknown }>) {
  const calls: Recorded[] = [];
  createAppClient("https://api.test", async (url: string, init: TransportInit) => {
    const call = { method: init.method, url, body: init.body ?? "" };
    calls.push(call);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "x" }) };
    }
    const answer = await handler(call);
    return {
      status: answer.status,
      url,
      headers: { get: () => null },
      text: async () => (answer.body === "" ? "" : JSON.stringify(answer.body)),
    };
  });
  return calls;
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

describe("commentsQueryOptions", () => {
  it("reads through the history endpoint and orders by creation", async () => {
    const older = comment({ id: "823e4567-e89b-12d3-a456-426614174008", created_at: "2026-10-15T00:00:00Z" });
    const { client, calls } = stubClient(() => ({ status: 200, body: [comment(), older] }));
    const options = commentsQueryOptions(client, SCOPE);
    expect(options.queryKey).toEqual(issueKeys.comments(SLUG, PROJECT, ISSUE));
    const rows = await (options.queryFn as () => Promise<IssueComment[]>)();
    expect(rows.map((row) => row.id)).toEqual([older.id, COMMENT]);
    expect(calls[0]?.url).toContain(`/issues/${ISSUE}/history/`);
    expect(calls[0]?.url).not.toContain("created_at__gt");
  });

  it("exposes the hook against the app client", async () => {
    stubClient(() => ({ status: 200, body: [comment()] }));
    const { queryClient, wrapper } = harness();
    const { result } = renderHook(() => useIssueComments(SCOPE), { wrapper });
    await waitFor(() => expect(result.current.data).toHaveLength(1));
    expect(queryClient.getQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE))).toHaveLength(1);
  });

  it("refetches only the delta once rows are cached, keeping creation order", async () => {
    const known = comment({ created_at: "2026-10-16T22:00:00Z" });
    const fresh = comment({
      id: "923e4567-e89b-12d3-a456-426614174009",
      created_at: "2026-10-16T23:00:00Z",
    });
    const { client, calls } = stubClient(() => ({ status: 200, body: [fresh] }));
    const { queryClient } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [known]);
    const options = commentsQueryOptions(client, SCOPE, queryClient);
    const rows = await (options.queryFn as () => Promise<IssueComment[]>)();
    expect(new URL(calls[0]?.url ?? "").searchParams.get("created_at__gt")).toBe("2026-10-16T22:00:00Z");
    expect(rows.map((row) => row.id)).toEqual([known.id, fresh.id]);
  });

  it("reads the whole list when the cache is empty or unreadable", async () => {
    const { client, calls } = stubClient(() => ({ status: 200, body: [comment()] }));
    const { queryClient } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), []);
    const empty = commentsQueryOptions(client, SCOPE, queryClient);
    await (empty.queryFn as () => Promise<IssueComment[]>)();
    expect(calls[0]?.url).not.toContain("created_at__gt");
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment({ created_at: "not a date" })]);
    const unreadable = commentsQueryOptions(client, SCOPE, queryClient);
    await (unreadable.queryFn as () => Promise<IssueComment[]>)();
    expect(calls[1]?.url).not.toContain("created_at__gt");
  });

  it("surfaces a refused delta read", async () => {
    const { client } = stubClient(() => ({ status: 500, body: { error: "boom" } }));
    const { queryClient } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment()]);
    const options = commentsQueryOptions(client, SCOPE, queryClient);
    await expect((options.queryFn as () => Promise<IssueComment[]>)()).rejects.toBeTruthy();
  });

  it("picks up new rows in place when the read is invalidated", async () => {
    const known = comment({ created_at: "2026-10-16T22:00:00Z" });
    const fresh = comment({
      id: "923e4567-e89b-12d3-a456-426614174009",
      created_at: "2026-10-16T23:00:00Z",
    });
    const { calls } = stubClient((call) => ({
      status: 200,
      body: call.url.includes("created_at__gt=") ? [fresh] : [known],
    }));
    const { queryClient, wrapper } = harness();
    const { result } = renderHook(() => useIssueComments(SCOPE), { wrapper });
    await waitFor(() => expect(result.current.data).toHaveLength(1));
    await queryClient.invalidateQueries({ queryKey: issueKeys.comments(SLUG, PROJECT, ISSUE) });
    await waitFor(() => expect(result.current.data).toHaveLength(2));
    expect(calls).toHaveLength(2);
    expect(new URL(calls[1]?.url ?? "").searchParams.get("created_at__gt")).toBe("2026-10-16T22:00:00Z");
    expect(result.current.data?.map((row) => row.id)).toEqual([known.id, fresh.id]);
  });
});

describe("useUpdateComment", () => {
  it("paints the change, toasts on success and confirms from the server row", async () => {
    const updated = comment({ labels: ["fold"] });
    const { calls } = stubClient(() => ({ status: 200, body: updated }));
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment()]);
    const { result } = renderHook(() => useUpdateComment(SCOPE, COMMENT_UPDATE_FAILED_TITLE, COMMENT_UPDATED_TITLE), {
      wrapper,
    });
    await result.current.mutateAsync({
      commentId: COMMENT,
      patch: { comment_html: "<p>New</p>", labels: ["fold"] },
    });
    expect(notifySuccess).toHaveBeenCalledTimes(1);
    expect(notifySuccess).toHaveBeenCalledWith(COMMENT_UPDATED_TITLE);
    expect(calls.some((call) => call.method === "PATCH" && call.url.includes(`/comments/${COMMENT}/`))).toBe(true);
    await waitFor(() =>
      expect(queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE))?.[0]?.labels).toEqual([
        "fold",
      ])
    );
  });

  it("rolls the cache back with a notice when the write is refused", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment()]);
    stubClient(() => ({ status: 403, body: { error: "Only the author can edit." } }));
    const { result } = renderHook(() => useUpdateComment(SCOPE, COMMENT_UPDATE_FAILED_TITLE, COMMENT_UPDATED_TITLE), {
      wrapper,
    });
    await result.current
      .mutateAsync({ commentId: COMMENT, patch: { comment_html: "<p>REJECTED</p>" } })
      .catch(() => undefined);
    expect(notifyError).toHaveBeenCalledWith(COMMENT_UPDATE_FAILED_TITLE, "Only the author can edit.");
    await waitFor(() =>
      expect(
        queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE))?.[0]?.comment_html
      ).toBe("<p>Hello</p>")
    );
  });

  it("rolls a refused fold back to the unfolded row", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment()]);
    stubClient(() => ({ status: 403, body: { error: "You don't have the required permissions." } }));
    const { result } = renderHook(() => useUpdateComment(SCOPE, COMMENT_UPDATE_FAILED_TITLE, COMMENT_UPDATED_TITLE), {
      wrapper,
    });
    await result.current.mutateAsync({ commentId: COMMENT, patch: foldPatch(comment(), true) }).catch(() => undefined);
    expect(notifyError).toHaveBeenCalledTimes(1);
    expect(notifyError).toHaveBeenCalledWith(COMMENT_UPDATE_FAILED_TITLE, "You don't have the required permissions.");
    await waitFor(() =>
      expect(queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE))?.[0]?.labels).toEqual(
        []
      )
    );
  });

  it("paints one comment optimistically and snapshots it for rollback", async () => {
    const { queryClient } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment()]);
    const snapshots = await paintComment(queryClient, SCOPE, COMMENT, {
      comment_html: "<p>Draft</p>",
      edited_at: "2026-10-18T00:00:00Z",
    });
    expect(queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE))?.[0]?.comment_html).toBe(
      "<p>Draft</p>"
    );
    expect(snapshots[0]?.data).toEqual([comment()]);
  });

  it("withholds an edited body and its marker until the PATCH lands (ISS-198)", async () => {
    let release: ((value: { status: number; body: unknown }) => void) | undefined;
    const calls = stubDeferredClient(() => {
      const answer = new Promise<{ status: number; body: unknown }>((resolve) => {
        release = resolve;
      });
      return answer;
    });
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment()]);
    const { result } = renderHook(() => useUpdateComment(SCOPE, COMMENT_UPDATE_FAILED_TITLE, COMMENT_UPDATED_TITLE), {
      wrapper,
    });
    const pending = result.current.mutateAsync({ commentId: COMMENT, patch: { comment_html: "<p>Revised</p>" } });
    // Give onMutate its turn: it cancels and snapshots before the write.
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    const midFlight = queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE))?.[0];
    // The stored row a fresh read would agree with is still the pre-edit
    // body, so the card must not claim the edit landed either.
    expect(midFlight?.comment_html).toBe("<p>Hello</p>");
    expect(midFlight?.edited_at).toBeNull();

    (release as ((value: { status: number; body: unknown }) => void) | undefined)?.({
      status: 200,
      body: comment({ comment_html: "<p>Revised</p>", edited_at: "2026-10-18T00:00:00Z" }),
    });
    await pending;
    expect(JSON.parse(calls.find((call) => call.method === "PATCH")?.body ?? "{}")).toEqual({
      comment_html: "<p>Revised</p>",
    });
    const confirmed = queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE))?.[0];
    expect(confirmed?.comment_html).toBe("<p>Revised</p>");
    expect(confirmed?.edited_at).toBe("2026-10-18T00:00:00Z");
  });

  it("keeps a fold or access flip immediate while a body change waits for the server", () => {
    // Fold and access are presentation flags the card may show at once.
    expect(pendingCommentPatch({ comment_html: "<p>Draft</p>", labels: ["fold"] })).toEqual({ labels: ["fold"] });
    expect(pendingCommentPatch({ access: "EXTERNAL" })).toEqual({ access: "EXTERNAL" });
    // A body-only patch paints nothing until the confirmed row arrives.
    expect(pendingCommentPatch({ comment_html: "<p>Draft</p>" })).toEqual({});
  });

  it("paints labels and access without touching the body or the edited marker when both are absent", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment()]);
    stubClient((call) =>
      call.method === "PATCH"
        ? { status: 200, body: comment({ access: "EXTERNAL", labels: ["fold"] }) }
        : { status: 200, body: [comment()] }
    );
    const { result } = renderHook(() => useUpdateComment(SCOPE, COMMENT_UPDATE_FAILED_TITLE, COMMENT_UPDATED_TITLE), {
      wrapper,
    });
    await result.current.mutateAsync({ commentId: COMMENT, patch: { access: "EXTERNAL" } });
    const row = queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE))?.[0];
    expect(row?.access).toBe("EXTERNAL");
    expect(row?.edited_at).toBeNull();
  });

  it("passes the client's own message through when the server body carries none", async () => {
    stubClient(() => ({ status: 500, body: "" }));
    const { wrapper } = harness();
    const { result } = renderHook(() => useUpdateComment(SCOPE, COMMENT_UPDATE_FAILED_TITLE, COMMENT_UPDATED_TITLE), {
      wrapper,
    });
    await result.current.mutateAsync({ commentId: COMMENT, patch: { labels: [] } }).catch(() => undefined);
    expect(notifyError).toHaveBeenCalledWith(COMMENT_UPDATE_FAILED_TITLE, "Request failed with status 500");
    expect(commentFailureDetail(new Error(""))).toBe("Check your connection and try again.");
    expect(commentFailureDetail("not an error")).toBe("Check your connection and try again.");
  });

  it("rolls a refused access switch back to the stored visibility with one notice", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment()]);
    stubClient(() => ({ status: 403, body: { error: "You don't have the required permissions." } }));
    const { result } = renderHook(() => useUpdateComment(SCOPE, COMMENT_UPDATE_FAILED_TITLE, COMMENT_UPDATED_TITLE), {
      wrapper,
    });
    await result.current.mutateAsync({ commentId: COMMENT, patch: accessPatch(comment()) }).catch(() => undefined);
    expect(notifyError).toHaveBeenCalledTimes(1);
    expect(notifyError).toHaveBeenCalledWith(COMMENT_UPDATE_FAILED_TITLE, "You don't have the required permissions.");
    expect(notifySuccess).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE))?.[0]?.access).toBe(
        "INTERNAL"
      )
    );
  });
});

describe("useDeleteComment", () => {
  it("removes the row, toasts and refreshes", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [
      comment(),
      comment({ id: "823e4567-e89b-12d3-a456-426614174008" }),
    ]);
    const { calls } = stubClient(() => ({ status: 204, body: "" }));
    const { result } = renderHook(() => useDeleteComment(SCOPE), { wrapper });
    await result.current.mutateAsync(COMMENT);
    expect(notifySuccess).toHaveBeenCalledTimes(1);
    expect(notifySuccess).toHaveBeenCalledWith(COMMENT_REMOVED_TITLE);
    expect(calls.some((call) => call.method === "DELETE" && call.url.includes(`/comments/${COMMENT}/`))).toBe(true);
    await waitFor(() =>
      expect(queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE))).toHaveLength(1)
    );
  });

  it("restores the row and notices when the delete is refused", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment()]);
    stubClient(() => ({ status: 403, body: { error: "Only the author can delete." } }));
    const { result } = renderHook(() => useDeleteComment(SCOPE), { wrapper });
    await result.current.mutateAsync(COMMENT).catch(() => undefined);
    expect(notifyError).toHaveBeenCalledWith(COMMENT_REMOVE_FAILED_TITLE, "Only the author can delete.");
    await waitFor(() =>
      expect(queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE))).toHaveLength(1)
    );
  });

  it("keeps a git-synced comment and carries the unbind instruction", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment({ is_synced: true })]);
    stubClient(() => ({
      status: 409,
      body: { error: "Unbind the project's repository to delete this comment." },
    }));
    const { result } = renderHook(() => useDeleteComment(SCOPE), { wrapper });
    await result.current.mutateAsync(COMMENT).catch(() => undefined);
    expect(notifyError).toHaveBeenCalledTimes(1);
    expect(notifyError).toHaveBeenCalledWith(
      COMMENT_SYNCED_REMOVE_FAILED_TITLE,
      "Unbind the project's repository to delete this comment."
    );
    const [title, message] = vi.mocked(notifyError).mock.calls[0] as [string, string];
    expect(`${title} ${message}`.toLowerCase()).toContain("fail");
    expect(message.toLowerCase()).toContain("unbind");
    await waitFor(() =>
      expect(queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE))).toHaveLength(1)
    );
  });
});

describe("useCreateComment", () => {
  it("posts, attaches parked images by comment id, and appends in order", async () => {
    const created = comment({ id: "823e4567-e89b-12d3-a456-426614174008", created_at: "2026-10-17T00:00:00Z" });
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment()]);
    const { calls } = stubClient((call) =>
      call.url.includes("/history/") ? { status: 200, body: [comment(), created] } : { status: 201, body: created }
    );
    const { result } = renderHook(() => useCreateComment(SCOPE), { wrapper });
    await result.current.mutateAsync({ html: "<p>Hi</p>", access: "EXTERNAL", assetIds: ["asset-1"] });
    expect(notifySuccess).toHaveBeenCalledTimes(1);
    expect(notifySuccess).toHaveBeenCalledWith(COMMENT_CREATED_TITLE);
    expect(notifyError).not.toHaveBeenCalled();
    const post = calls.find((call) => call.method === "POST" && call.url.endsWith("/comments/"));
    expect(JSON.parse(post?.body ?? "{}")).toEqual({ comment_html: "<p>Hi</p>", access: "EXTERNAL" });
    const bulk = calls.find((call) => call.url.includes("/bulk/"));
    expect(bulk?.url).toContain(`/projects/${PROJECT}/${created.id}/bulk/`);
    expect(JSON.parse(bulk?.body ?? "{}")).toEqual({ asset_ids: ["asset-1"] });
    await waitFor(() => {
      const rows = queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE)) ?? [];
      expect(rows.map((row) => row.id)).toEqual([COMMENT, created.id]);
    });
  });

  it("re-reads the row after the bulk link so the feed shows the pictures (CMT-009)", async () => {
    const created = comment({
      id: "823e4567-e89b-12d3-a456-426614174008",
      created_at: "2026-10-17T00:00:00Z",
      comment_html: "<p>Hi</p>",
    });
    const attached = comment({
      ...created,
      comment_html: '<p>Hi</p><image-component src="asset-1"></image-component>',
    });
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment()]);
    const { calls } = stubClient((call) => {
      if (call.url.includes("/history/")) return { status: 200, body: [comment(), attached] };
      if (call.method === "GET") return { status: 200, body: attached };
      if (call.url.includes("/bulk/")) return { status: 204, body: "" };
      return { status: 201, body: created };
    });
    const { result } = renderHook(() => useCreateComment(SCOPE), { wrapper });
    const row = await result.current.mutateAsync({ html: "<p>Hi</p>", access: "EXTERNAL", assetIds: ["asset-1"] });
    expect(row.comment_html).toContain("image-component");
    const reread = calls.find((call) => call.method === "GET" && call.url.includes(`/comments/${created.id}/`));
    expect(reread?.url).toContain(`/issues/${ISSUE}/comments/${created.id}/`);
    await waitFor(() => {
      const rows = queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE)) ?? [];
      expect(rows.map((entry) => entry.id)).toEqual([COMMENT, created.id]);
      expect(rows[1]?.comment_html).toContain("image-component");
    });
  });

  it("keeps the create response when the post-attach re-read is refused", async () => {
    const created = comment({ id: "823e4567-e89b-12d3-a456-426614174008", comment_html: "<p>Hi</p>" });
    const { wrapper } = harness();
    stubClient((call) => {
      if (call.url.includes("/bulk/")) return { status: 204, body: "" };
      if (call.method === "GET") return { status: 500, body: { error: "boom" } };
      return { status: 201, body: created };
    });
    const { result } = renderHook(() => useCreateComment(SCOPE), { wrapper });
    const row = await result.current.mutateAsync({ html: "<p>Hi</p>", access: "INTERNAL", assetIds: ["asset-1"] });
    expect(row).toEqual(created);
    expect(notifySuccess).toHaveBeenCalledWith(COMMENT_CREATED_TITLE);
  });

  it("skips the bulk write without images and keeps a row already in cache", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE), [comment()]);
    const { calls } = stubClient(() => ({ status: 201, body: comment() }));
    const { result } = renderHook(() => useCreateComment(SCOPE), { wrapper });
    await result.current.mutateAsync({ html: "<p>Hi</p>", access: "INTERNAL" });
    expect(calls.some((call) => call.url.includes("/bulk/"))).toBe(false);
    expect(queryClient.getQueryData<IssueComment[]>(issueKeys.comments(SLUG, PROJECT, ISSUE))).toHaveLength(1);
  });

  it("shows the attach failure but keeps the created comment", async () => {
    const created = comment({ id: "823e4567-e89b-12d3-a456-426614174008" });
    const { wrapper } = harness();
    stubClient((call) =>
      call.url.includes("/bulk/") ? { status: 500, body: { error: "Asset missing" } } : { status: 201, body: created }
    );
    const { result } = renderHook(() => useCreateComment(SCOPE), { wrapper });
    await expect(
      result.current.mutateAsync({ html: "<p>Hi</p>", access: "INTERNAL", assetIds: ["asset-1"] })
    ).resolves.toBeTruthy();
    expect(notifyError).toHaveBeenCalledWith("Couldn't finish the comment upload", "Asset missing");
    // The attach error rides beside the single create notice; the create
    // itself is never toasted twice for one post.
    expect(notifyError).toHaveBeenCalledTimes(1);
    expect(notifySuccess).toHaveBeenCalledTimes(1);
    expect(notifySuccess).toHaveBeenCalledWith(COMMENT_CREATED_TITLE);
  });

  it("notices when the create itself is refused", async () => {
    stubClient(() => ({ status: 400, body: { error: "Invalid HTML passed" } }));
    const { wrapper } = harness();
    const { result } = renderHook(() => useCreateComment(SCOPE), { wrapper });
    await result.current.mutateAsync({ html: "", access: "INTERNAL" }).catch(() => undefined);
    expect(notifyError).toHaveBeenCalledTimes(1);
    expect(notifyError).toHaveBeenCalledWith(COMMENT_CREATE_FAILED_TITLE, "Invalid HTML passed");
  });

  it("carries the server text and stores nothing on a guest refusal (CMT-002)", async () => {
    stubClient(() => ({ status: 400, body: { error: "Guests may only comment on work items they created." } }));
    const { queryClient, wrapper } = harness();
    const { result } = renderHook(() => useCreateComment(SCOPE), { wrapper });
    await result.current.mutateAsync({ html: "<p>Hi</p>", access: "INTERNAL" }).catch(() => undefined);
    expect(notifyError).toHaveBeenCalledWith(
      COMMENT_CREATE_FAILED_TITLE,
      "Guests may only comment on work items they created."
    );
    expect(queryClient.getQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE))).toBeUndefined();
  });
});

describe("duplicateKeptCommentImages", () => {
  const SCOPE_WITH_COMMENT = { workspaceSlug: SLUG, projectId: PROJECT, commentId: COMMENT };

  it("duplicates kept images and rebinds the new body (CMT-009)", async () => {
    const { client, calls } = stubClient(() => ({ status: 200, body: { asset_id: "asset-1b" } }));
    const original = '<p>First <image-component src="asset-1" status="uploaded"></image-component></p>';
    const next =
      '<p>First <image-component src="asset-1" status="uploaded"></image-component> ' +
      '<img src="/api/assets/v2/workspaces/acme/projects/p/asset-new/"></p>';
    const saved = await duplicateKeptCommentImages(client, SCOPE_WITH_COMMENT, original, next);
    expect(saved).toContain('src="asset-1b"');
    expect(saved).not.toContain('src="asset-1"');
    expect(saved).toContain("asset-new");
    const duplicate = calls.find((call) => call.url.includes("/duplicate-assets/"));
    expect(duplicate?.url).toBe(`https://api.test/api/assets/v2/workspaces/${SLUG}/duplicate-assets/asset-1/`);
    expect(JSON.parse(duplicate?.body ?? "{}")).toEqual({
      entity_id: COMMENT,
      entity_type: "COMMENT_DESCRIPTION",
      project_id: PROJECT,
    });
    expect(calls.filter((call) => call.url.includes("/duplicate-assets/"))).toHaveLength(1);
  });

  it("makes no call when nothing is kept and returns the body untouched", async () => {
    const { client, calls } = stubClient(() => ({ status: 200, body: { asset_id: "asset-x" } }));
    const added = '<p>New <image-component src="asset-new" status="uploaded"></image-component></p>';
    await expect(duplicateKeptCommentImages(client, SCOPE_WITH_COMMENT, "<p>Before</p>", added)).resolves.toBe(added);
    const dropped = "<p>Text only now</p>";
    const original = '<p>Text <image-component src="asset-1" status="uploaded"></image-component></p>';
    await expect(duplicateKeptCommentImages(client, SCOPE_WITH_COMMENT, original, dropped)).resolves.toBe(dropped);
    expect(calls).toHaveLength(0);
  });

  it("rejects when a duplicate is refused so the save fails instead", async () => {
    const { client } = stubClient(() => ({ status: 404, body: { error: "Asset missing" } }));
    const body = '<p>Keep <image-component src="asset-1" status="uploaded"></image-component></p>';
    await expect(duplicateKeptCommentImages(client, SCOPE_WITH_COMMENT, body, body)).rejects.toBeTruthy();
  });
});

describe("useCommentMentionSearch", () => {
  const PEOPLE = {
    user_mention: [
      { member__id: "u-1", member__display_name: "Ada L", member__avatar_url: "https://cdn.test/a.png" },
      { member__id: "u-2", member__display_name: "Grace H", member__avatar_url: null },
    ],
  };

  it("caps the lookup at five people per project (CMT-019)", async () => {
    expect(COMMENT_MENTION_LIMIT).toBe(5);
    const { client, calls } = stubClient(() => ({ status: 200, body: PEOPLE }));
    const people = await requestCommentMentions(client, SLUG, PROJECT, "a");
    expect(people).toEqual([
      { id: "u-1", name: "Ada L", avatarUrl: "https://cdn.test/a.png" },
      { id: "u-2", name: "Grace H", avatarUrl: null },
    ]);
    const call = calls.find((entry) => entry.url.includes("/entity-search/"));
    expect(call?.url).toContain("query=a");
    expect(call?.url).toContain("query_type=user_mention");
    expect(call?.url).toContain(`project_id=${PROJECT}`);
    expect(call?.url).toContain("count=5");
  });

  it("reads no one without a project and keys the query by its text", async () => {
    const { client, calls } = stubClient(() => ({ status: 200, body: PEOPLE }));
    await expect(requestCommentMentions(client, SLUG, "", "a")).resolves.toEqual([]);
    expect(calls).toHaveLength(0);
    expect(commentMentionSearchKey(SLUG, PROJECT, "ad")).toContain("ad");
    expect(commentMentionSearchKey(SLUG, PROJECT, "ad")).not.toEqual(commentMentionSearchKey(SLUG, PROJECT, "ada"));
  });

  it("exposes the hook against the app client and stays idle while disabled", async () => {
    stubClient(() => ({ status: 200, body: PEOPLE }));
    const { wrapper } = harness();
    const { result } = renderHook(() => useCommentMentionSearch(SLUG, PROJECT, "a", true), { wrapper });
    await waitFor(() => expect(result.current.data).toHaveLength(2));
    // A query text of its own: the shared cache would hand the idle
    // hook the rows above, hiding whether it fetched.
    const idle = renderHook(() => useCommentMentionSearch(SLUG, PROJECT, "ad", false), { wrapper });
    expect(idle.result.current.data).toBeUndefined();
    expect(idle.result.current.fetchStatus).toBe("idle");
  });
});

describe("useUploadCommentImage", () => {
  it("uploads, attaches for an edit and resolves the composer image", async () => {
    const { calls } = stubClient((call) => {
      if (call.method === "POST" && call.url.includes("/projects/") && !call.url.includes("/bulk/")) {
        return {
          status: 200,
          body: {
            asset_id: "asset-1",
            asset_url: "/api/assets/v2/workspaces/acme/projects/x/asset-1/",
            upload_data: { url: "https://s3.test/u", fields: {} },
          },
        };
      }
      return { status: 204, body: "" };
    });
    vi.stubGlobal("fetch", async () => ({ ok: true, status: 204 }));
    const { wrapper } = harness();
    const { result } = renderHook(() => useUploadCommentImage(SCOPE), { wrapper });
    const image = await result.current.mutateAsync({
      file: new File(["x"], "shot.png", { type: "image/png" }),
      commentId: COMMENT,
    });
    expect(image).toEqual({ assetId: "asset-1", src: "/api/assets/v2/workspaces/acme/projects/x/asset-1/" });
    expect(calls.some((call) => call.url.includes(`/projects/${PROJECT}/${COMMENT}/bulk/`))).toBe(true);
    vi.unstubAllGlobals();
  });

  it("leaves a post-composer upload parked and notices a refused upload", async () => {
    stubClient(() => ({ status: 400, body: { error: "Invalid file type." } }));
    const { wrapper } = harness();
    const { result } = renderHook(() => useUploadCommentImage(SCOPE), { wrapper });
    await result.current
      .mutateAsync({ file: new File(["x"], "shot.svg", { type: "image/svg+xml" }) })
      .catch(() => undefined);
    expect(notifyError).toHaveBeenCalledTimes(1);
    expect(notifyError).toHaveBeenCalledWith(COMMENT_IMAGE_UPLOAD_FAILED_TITLE, "Invalid file type.");
  });
});

describe("cache edges", () => {
  it("paints and removes do nothing when the list was never read", async () => {
    const { queryClient } = harness();
    expect(await paintComment(queryClient, SCOPE, COMMENT, { labels: ["fold"] })).toEqual([]);
    expect(await removeCommentRow(queryClient, SCOPE, COMMENT)).toEqual([]);
    expect(queryClient.getQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE))).toBeUndefined();
  });

  it("leaves an unread thread to its own first read after an update", async () => {
    const updated = comment({ labels: ["fold"] });
    stubClient((call) => ({ status: 200, body: call.method === "PATCH" ? updated : [] }));
    const { queryClient, wrapper } = harness();
    const { result } = renderHook(() => useUpdateComment(SCOPE, COMMENT_UPDATE_FAILED_TITLE, COMMENT_UPDATED_TITLE), {
      wrapper,
    });
    await result.current.mutateAsync({ commentId: COMMENT, patch: { labels: ["fold"] } });
    expect(notifySuccess).toHaveBeenCalledWith(COMMENT_UPDATED_TITLE);
    expect(queryClient.getQueryData(issueKeys.comments(SLUG, PROJECT, ISSUE))).toBeUndefined();
  });

  it("gives a bare Error the fallback copy and keeps an ApiError's own words", () => {
    expect(commentFailureDetail(new Error(""))).toBe(COMMENT_FAILURE_FALLBACK);
    expect(commentFailureDetail(new Error("Offline"))).toBe(COMMENT_FAILURE_FALLBACK);
  });
});

describe("patches", () => {
  it("builds fold and access writes from the row", () => {
    expect(foldPatch(comment({ labels: ["pinned"] }), true)).toEqual({ labels: ["pinned", "fold"] });
    expect(foldPatch(comment({ labels: ["fold"] }), false)).toEqual({ labels: [] });
    expect(accessPatch(comment())).toEqual({ access: "EXTERNAL" });
    expect(accessPatch(comment({ access: "EXTERNAL" }))).toEqual({ access: "INTERNAL" });
  });

  it("keeps the clipboard failure notice copy stable", () => {
    expect(COMMENT_LINK_COPY_FAILED_TITLE).toBe("Couldn't copy the comment link");
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Me, SpaceComment, SpaceCommentReaction, Transport, TransportInit } from "@pidash/api-client";
import { createClient } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/spaceNotices.js", () => ({
  notifySpaceAttachmentFailure: vi.fn(),
  notifySpaceCommentDeleteFailure: vi.fn(),
  notifySpaceCommentDeleteSuccess: vi.fn(),
  notifySpaceCommentEditFailure: vi.fn(),
  notifySpaceCommentEditSuccess: vi.fn(),
  notifySpaceCommentFailure: vi.fn(),
  notifySpaceReactionFailure: vi.fn(),
}));

import {
  notifySpaceAttachmentFailure,
  notifySpaceCommentDeleteFailure,
  notifySpaceCommentDeleteSuccess,
  notifySpaceCommentEditFailure,
  notifySpaceCommentEditSuccess,
  notifySpaceCommentFailure,
  notifySpaceReactionFailure,
} from "../lib/spaceNotices.js";
import { spaceKeys } from "../api/keys.js";
import { spaceViewerQueryOptions } from "../api/queries.js";
import {
  invalidateSpaceComments,
  useCreateSpaceComment,
  useDeleteSpaceComment,
  useSpaceCommentReactions,
  useSpaceMentionPeople,
  useUpdateSpaceComment,
  useUploadSpaceCommentImage,
} from "./api.js";

const ANCHOR = "anchor-1";
const ISSUE_ID = "11111111-1111-4111-8111-111111111111";
const COMMENT_ID = "123e4567-e89b-12d3-a456-426614174000";
const ADA = "523e4567-e89b-12d3-a456-426614174004";
const GUEST = "623e4567-e89b-12d3-a456-426614174005";

function comment(overrides: Partial<SpaceComment> = {}): SpaceComment {
  return {
    id: COMMENT_ID,
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
    issue: ISSUE_ID,
    project: "323e4567-e89b-12d3-a456-426614174002",
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    comment_html: "<p>Hello</p>",
    comment_stripped: "Hello",
    comment_json: {},
    attachments: [],
    labels: [],
    access: "EXTERNAL",
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

function reaction(overrides: Partial<SpaceCommentReaction> = {}): SpaceCommentReaction {
  return {
    id: "723e4567-e89b-12d3-a456-426614174006",
    actor: GUEST,
    comment: COMMENT_ID,
    reaction: "127881",
    display_name: "Guest",
    deleted_at: null,
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    project: "323e4567-e89b-12d3-a456-426614174002",
    created_at: "2026-10-16T22:00:00Z",
    updated_at: "2026-10-16T22:00:00Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

function viewer(): Me {
  return {
    id: ADA,
    email: "ada@test.dev",
    username: "ada",
    first_name: "Ada",
    last_name: "L",
    display_name: "Ada L",
    avatar: "",
    avatar_url: null,
    cover_image: null,
    cover_image_url: null,
    date_joined: "2026-01-01T00:00:00Z",
    user_timezone: "UTC",
    is_active: true,
    is_bot: false,
    is_email_verified: true,
    is_password_autoset: false,
    last_login_medium: "password",
    last_login_time: null,
  };
}

interface Recorded {
  method: string;
  url: string;
  body: string;
}

function stubClient(handler: (call: Recorded) => { status: number; body: unknown }) {
  const calls: Recorded[] = [];
  const transport: Transport = async (url: string, init?: TransportInit) => {
    const call = { method: init?.method ?? "GET", url, body: init?.body ?? "" };
    calls.push(call);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "x" }),
      };
    }
    const answer = handler(call);
    return {
      status: answer.status,
      url,
      headers: { get: () => "application/json" },
      text: async () => (answer.body === "" ? "" : JSON.stringify(answer.body)),
    };
  };
  const client = createClient({ baseUrl: "https://api.test", transport });
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
  vi.mocked(notifySpaceAttachmentFailure).mockClear();
  vi.mocked(notifySpaceCommentDeleteFailure).mockClear();
  vi.mocked(notifySpaceCommentDeleteSuccess).mockClear();
  vi.mocked(notifySpaceCommentEditFailure).mockClear();
  vi.mocked(notifySpaceCommentEditSuccess).mockClear();
  vi.mocked(notifySpaceCommentFailure).mockClear();
  vi.mocked(notifySpaceReactionFailure).mockClear();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("invalidateSpaceComments", () => {
  it("re-reads the stored thread", async () => {
    const { queryClient } = harness();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    await invalidateSpaceComments(queryClient, ANCHOR, ISSUE_ID);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: spaceKeys.comments(ANCHOR, ISSUE_ID) });
  });
});

describe("useSpaceMentionPeople", () => {
  const members = [
    { id: "723e4567-e89b-12d3-a456-426614174006", member: ADA, member__display_name: "Ada L", member__avatar: "" },
    { id: "823e4567-e89b-12d3-a456-426614174008", member: GUEST, member__display_name: "Guest G", member__avatar: "" },
  ];

  it("filters the board members by the live query (SPC-054)", async () => {
    const { client } = stubClient(() => ({ status: 200, body: members }));
    const { wrapper } = harness();
    const { result } = renderHook(() => useSpaceMentionPeople(client, ANCHOR, "ada", true), { wrapper });
    await waitFor(() => expect(result.current).toHaveLength(1));
    expect(result.current).toEqual([{ id: ADA, name: "Ada L", avatarUrl: null }]);
  });

  it("offers nobody without a query (SPC-054)", async () => {
    const { client } = stubClient(() => ({ status: 200, body: members }));
    const { wrapper } = harness();
    const { result } = renderHook(() => useSpaceMentionPeople(client, ANCHOR, "", true), { wrapper });
    await waitFor(() => expect(result.current).toEqual([]));
  });

  it("reads nothing while disabled or on a malformed anchor (SPC-054)", () => {
    const { client, calls } = stubClient(() => ({ status: 200, body: members }));
    const { wrapper } = harness();
    const disabled = renderHook(() => useSpaceMentionPeople(client, ANCHOR, "ada", false), { wrapper });
    const malformed = renderHook(() => useSpaceMentionPeople(client, "bad anchor", "ada", true), { wrapper });
    expect(disabled.result.current).toEqual([]);
    expect(malformed.result.current).toEqual([]);
    expect(calls).toEqual([]);
  });
});

describe("useUploadSpaceCommentImage", () => {
  const grant = {
    asset_id: "asset-1",
    asset_url: null,
    upload_data: { url: "https://storage.test/upload", fields: { key: "k1", token: "t1" } },
  };

  function file(): File {
    return new File(["bytes"], "pic.png", { type: "image/png" });
  }

  it("walks the three legs and confirms before resolving (SPC-053)", async () => {
    const seen: Array<{ url: string; form: FormData }> = [];
    const { client, calls } = stubClient((call) => {
      if (call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/")) {
        return { status: 201, body: grant };
      }
      return { status: 204, body: "" };
    });
    const { wrapper } = harness();
    const { result } = renderHook(
      () =>
        useUploadSpaceCommentImage(client, ANCHOR, async (url, form) => {
          seen.push({ url, form });
          return { ok: true, status: 204 };
        }),
      { wrapper }
    );
    const image = await result.current.mutateAsync({ file: file(), commentId: COMMENT_ID });
    expect(image).toEqual({
      assetId: "asset-1",
      src: "https://api.test/api/public/assets/v2/anchor/anchor-1/asset-1/",
    });
    const grantCall = calls.find((call) => call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/"));
    expect(JSON.parse(grantCall?.body ?? "{}")).toMatchObject({
      name: "pic.png",
      type: "image/png",
      size: 5,
      entity_type: "COMMENT_DESCRIPTION",
      entity_identifier: COMMENT_ID,
    });
    expect(seen).toHaveLength(1);
    expect(seen[0]?.url).toBe("https://storage.test/upload");
    expect(seen[0]?.form.get("key")).toBe("k1");
    expect(seen[0]?.form.get("token")).toBe("t1");
    expect(seen[0]?.form.get("file")).toBeInstanceOf(File);
    expect(calls[calls.length - 1]?.url).toBe("https://api.test/api/public/assets/v2/anchor/anchor-1/asset-1/");
    expect(calls[calls.length - 1]?.method).toBe("PATCH");
    expect(notifySpaceAttachmentFailure).not.toHaveBeenCalled();
  });

  it("parks a blank entity identifier for a new comment (SPC-053)", async () => {
    const { client, calls } = stubClient((call) =>
      call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/")
        ? { status: 201, body: grant }
        : { status: 204, body: "" }
    );
    const { wrapper } = harness();
    const { result } = renderHook(
      () => useUploadSpaceCommentImage(client, ANCHOR, async () => ({ ok: true, status: 204 })),
      {
        wrapper,
      }
    );
    await result.current.mutateAsync({ file: file() });
    const grantCall = calls.find((call) => call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/"));
    expect(JSON.parse(grantCall?.body ?? "{}")).toMatchObject({ entity_identifier: "" });
  });

  it("posts bytes through the global fetch when no transport is injected (SPC-053)", async () => {
    const { client } = stubClient((call) =>
      call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/")
        ? { status: 201, body: grant }
        : { status: 204, body: "" }
    );
    const fetchMock = vi.fn(async () => ({ ok: true, status: 204 }));
    vi.stubGlobal("fetch", fetchMock);
    const { wrapper } = harness();
    const { result } = renderHook(() => useUploadSpaceCommentImage(client, ANCHOR), { wrapper });
    const image = await result.current.mutateAsync({ file: file() });
    expect(image.assetId).toBe("asset-1");
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("reports a refused byte leg without confirming (SPC-053)", async () => {
    const { client, calls } = stubClient(() => ({ status: 201, body: grant }));
    const { wrapper } = harness();
    const { result } = renderHook(
      () => useUploadSpaceCommentImage(client, ANCHOR, async () => ({ ok: false, status: 500 })),
      {
        wrapper,
      }
    );
    await expect(result.current.mutateAsync({ file: file() })).rejects.toThrow("status 500");
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
    expect(notifySpaceAttachmentFailure).toHaveBeenCalledTimes(1);
  });

  it("reports a refused grant (SPC-053)", async () => {
    const { client } = stubClient(() => ({ status: 500, body: { error: "boom" } }));
    const { wrapper } = harness();
    const { result } = renderHook(
      () => useUploadSpaceCommentImage(client, ANCHOR, async () => ({ ok: true, status: 204 })),
      {
        wrapper,
      }
    );
    await expect(result.current.mutateAsync({ file: file() })).rejects.toBeTruthy();
    expect(notifySpaceAttachmentFailure).toHaveBeenCalledTimes(1);
  });
});

describe("useCreateSpaceComment", () => {
  const created = () =>
    comment({
      id: "823e4567-e89b-12d3-a456-426614174008",
      comment_html: "<p>Hi</p>",
      created_at: "2026-10-17T00:00:00Z",
    });

  it("posts, links parked images, and appends in creation order (SPC-053)", async () => {
    const fresh = created();
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID), [comment()]);
    const { client, calls } = stubClient((call) => {
      if (call.method === "GET") return { status: 200, body: [comment(), fresh] };
      if (call.url.includes("/bulk/")) return { status: 204, body: "" };
      return { status: 201, body: fresh };
    });
    const { result } = renderHook(() => useCreateSpaceComment(client, ANCHOR, ISSUE_ID), { wrapper });
    await result.current.mutateAsync({ html: "<p>Hi</p>", assetIds: ["asset-1"] });
    const post = calls.find((call) => call.method === "POST" && call.url.endsWith("/comments/"));
    expect(JSON.parse(post?.body ?? "{}")).toEqual({ comment_html: "<p>Hi</p>", access: "EXTERNAL" });
    const bulk = calls.find((call) => call.url.includes("/bulk/"));
    expect(bulk?.url).toContain(`/assets/v2/anchor/anchor-1/${fresh.id}/bulk/`);
    expect(JSON.parse(bulk?.body ?? "{}")).toEqual({ asset_ids: ["asset-1"] });
    expect(notifySpaceCommentFailure).not.toHaveBeenCalled();
    await waitFor(() => {
      const rows = queryClient.getQueryData<SpaceComment[]>(spaceKeys.comments(ANCHOR, ISSUE_ID)) ?? [];
      expect(rows.map((row) => row.id)).toEqual([COMMENT_ID, fresh.id]);
    });
  });

  it("skips the bulk link without images and keeps a row already cached (SPC-053)", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID), [comment()]);
    const { client, calls } = stubClient((call) =>
      call.method === "GET" ? { status: 200, body: [comment()] } : { status: 201, body: comment() }
    );
    const { result } = renderHook(() => useCreateSpaceComment(client, ANCHOR, ISSUE_ID), { wrapper });
    await result.current.mutateAsync({ html: "<p>Hi</p>" });
    expect(calls.some((call) => call.url.includes("/bulk/"))).toBe(false);
    expect(queryClient.getQueryData<SpaceComment[]>(spaceKeys.comments(ANCHOR, ISSUE_ID))).toHaveLength(1);
  });

  it("keeps an unread thread untouched until its first read (SPC-053)", async () => {
    const fresh = created();
    const { queryClient, wrapper } = harness();
    const { client } = stubClient((call) =>
      call.method === "GET" ? { status: 500, body: { error: "boom" } } : { status: 201, body: fresh }
    );
    const { result } = renderHook(() => useCreateSpaceComment(client, ANCHOR, ISSUE_ID), { wrapper });
    await result.current.mutateAsync({ html: "<p>Hi</p>" });
    await waitFor(() => expect(queryClient.isFetching()).toBe(0));
    expect(queryClient.getQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID))).toBeUndefined();
  });

  it("reports a failed link but keeps the posted comment (SPC-053)", async () => {
    const fresh = created();
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID), [comment()]);
    const { client } = stubClient((call) => {
      if (call.method === "GET") return { status: 200, body: [comment(), fresh] };
      if (call.url.includes("/bulk/")) return { status: 500, body: { error: "Asset missing" } };
      return { status: 201, body: fresh };
    });
    const { result } = renderHook(() => useCreateSpaceComment(client, ANCHOR, ISSUE_ID), { wrapper });
    await result.current.mutateAsync({ html: "<p>Hi</p>", assetIds: ["asset-1"] });
    expect(notifySpaceAttachmentFailure).toHaveBeenCalledTimes(1);
    expect(notifySpaceCommentFailure).not.toHaveBeenCalled();
    await waitFor(() => {
      const rows = queryClient.getQueryData<SpaceComment[]>(spaceKeys.comments(ANCHOR, ISSUE_ID)) ?? [];
      expect(rows.map((row) => row.id)).toContain(fresh.id);
    });
  });

  it("reports a refused post and leaves the cached thread untouched (SPC-053)", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID), [comment()]);
    const { client } = stubClient(() => ({ status: 400, body: { error: "Invalid HTML passed" } }));
    const { result } = renderHook(() => useCreateSpaceComment(client, ANCHOR, ISSUE_ID), { wrapper });
    await result.current.mutateAsync({ html: "<p>Hi</p>" }).catch(() => undefined);
    expect(notifySpaceCommentFailure).toHaveBeenCalledTimes(1);
    expect(queryClient.getQueryData<SpaceComment[]>(spaceKeys.comments(ANCHOR, ISSUE_ID))).toEqual([comment()]);
  });
});

describe("useUpdateSpaceComment", () => {
  it("swaps the body at once and confirms the stored row (SPC-056)", async () => {
    const stored = comment({ comment_html: "<p>Edited</p>", updated_at: "2026-10-17T00:00:00Z" });
    const other = comment({ id: "823e4567-e89b-12d3-a456-426614174008", comment_html: "<p>Other</p>" });
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID), [comment(), other]);
    const { client, calls } = stubClient((call) => {
      if (call.method === "GET") return { status: 200, body: [stored, other] };
      return { status: 200, body: stored };
    });
    const { result } = renderHook(() => useUpdateSpaceComment(client, ANCHOR, ISSUE_ID), { wrapper });
    const flight = result.current.mutateAsync({ commentId: COMMENT_ID, html: "<p>Edited</p>" });
    await waitFor(() => {
      const rows = queryClient.getQueryData<SpaceComment[]>(spaceKeys.comments(ANCHOR, ISSUE_ID)) ?? [];
      expect(rows[0]?.comment_html).toBe("<p>Edited</p>");
    });
    await flight;
    const patch = calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toBe(`https://api.test/api/public/anchor/anchor-1/issues/${ISSUE_ID}/comments/${COMMENT_ID}/`);
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ comment_html: "<p>Edited</p>" });
    expect(notifySpaceCommentEditSuccess).toHaveBeenCalledTimes(1);
    expect(notifySpaceCommentEditFailure).not.toHaveBeenCalled();
    await waitFor(() => {
      const rows = queryClient.getQueryData<SpaceComment[]>(spaceKeys.comments(ANCHOR, ISSUE_ID)) ?? [];
      expect(rows[0]?.updated_at).toBe("2026-10-17T00:00:00Z");
      expect(rows[1]?.comment_html).toBe("<p>Other</p>");
    });
  });

  it("reconciles a refused save to the stored thread (SPC-056)", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID), [comment()]);
    const { client } = stubClient((call) => {
      if (call.method === "GET") return { status: 200, body: [comment()] };
      return { status: 500, body: { error: "boom" } };
    });
    const { result } = renderHook(() => useUpdateSpaceComment(client, ANCHOR, ISSUE_ID), { wrapper });
    await result.current.mutateAsync({ commentId: COMMENT_ID, html: "<p>Phantom</p>" }).catch(() => undefined);
    expect(notifySpaceCommentEditFailure).toHaveBeenCalledTimes(1);
    expect(notifySpaceCommentEditSuccess).not.toHaveBeenCalled();
    const rows = queryClient.getQueryData<SpaceComment[]>(spaceKeys.comments(ANCHOR, ISSUE_ID)) ?? [];
    expect(rows[0]?.comment_html).toBe("<p>Hello</p>");
  });

  it("leaves an unread cache alone on both ends of the write (SPC-056)", async () => {
    const stored = comment({ comment_html: "<p>Edited</p>" });
    const { queryClient, wrapper } = harness();
    const { client } = stubClient((call) => {
      if (call.method === "GET") return { status: 500, body: { error: "boom" } };
      return { status: 200, body: stored };
    });
    const { result } = renderHook(() => useUpdateSpaceComment(client, ANCHOR, ISSUE_ID), { wrapper });
    await result.current.mutateAsync({ commentId: COMMENT_ID, html: "<p>Edited</p>" });
    await waitFor(() => expect(queryClient.isFetching()).toBe(0));
    expect(queryClient.getQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID))).toBeUndefined();
    expect(notifySpaceCommentEditSuccess).toHaveBeenCalledTimes(1);
  });

  it("still reports when the optimistic paint itself throws (SPC-056)", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID), [comment()]);
    const { client } = stubClient(() => ({ status: 200, body: comment() }));
    vi.spyOn(queryClient, "cancelQueries").mockRejectedValueOnce(new Error("cancelled"));
    const { result } = renderHook(() => useUpdateSpaceComment(client, ANCHOR, ISSUE_ID), { wrapper });
    await result.current.mutateAsync({ commentId: COMMENT_ID, html: "<p>Edited</p>" }).catch(() => undefined);
    expect(notifySpaceCommentEditFailure).toHaveBeenCalledTimes(1);
  });
});

describe("useDeleteSpaceComment", () => {
  it("removes the row at once and confirms with a notice (SPC-055)", async () => {
    const other = comment({ id: "823e4567-e89b-12d3-a456-426614174008" });
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID), [comment(), other]);
    const { client, calls } = stubClient((call) => {
      if (call.method === "GET") return { status: 200, body: [other] };
      return { status: 204, body: "" };
    });
    const { result } = renderHook(() => useDeleteSpaceComment(client, ANCHOR, ISSUE_ID), { wrapper });
    const flight = result.current.mutateAsync(COMMENT_ID);
    await waitFor(() => {
      const rows = queryClient.getQueryData<SpaceComment[]>(spaceKeys.comments(ANCHOR, ISSUE_ID)) ?? [];
      expect(rows.map((row) => row.id)).toEqual([other.id]);
    });
    await flight;
    expect(calls.find((call) => call.method === "DELETE")?.url).toBe(
      `https://api.test/api/public/anchor/anchor-1/issues/${ISSUE_ID}/comments/${COMMENT_ID}/`
    );
    expect(notifySpaceCommentDeleteSuccess).toHaveBeenCalledTimes(1);
    expect(notifySpaceCommentDeleteFailure).not.toHaveBeenCalled();
  });

  it("restores a refused delete and reports (SPC-055)", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID), [comment()]);
    const { client } = stubClient((call) => {
      if (call.method === "GET") return { status: 200, body: [comment()] };
      return { status: 500, body: { error: "boom" } };
    });
    const { result } = renderHook(() => useDeleteSpaceComment(client, ANCHOR, ISSUE_ID), { wrapper });
    await result.current.mutateAsync(COMMENT_ID).catch(() => undefined);
    expect(notifySpaceCommentDeleteFailure).toHaveBeenCalledTimes(1);
    expect(notifySpaceCommentDeleteSuccess).not.toHaveBeenCalled();
    expect(queryClient.getQueryData<SpaceComment[]>(spaceKeys.comments(ANCHOR, ISSUE_ID))).toEqual([comment()]);
  });

  it("leaves an unread cache alone (SPC-055)", async () => {
    const { queryClient, wrapper } = harness();
    const { client } = stubClient((call) => {
      if (call.method === "GET") return { status: 500, body: { error: "boom" } };
      return { status: 204, body: "" };
    });
    const { result } = renderHook(() => useDeleteSpaceComment(client, ANCHOR, ISSUE_ID), { wrapper });
    await result.current.mutateAsync(COMMENT_ID);
    await waitFor(() => expect(queryClient.isFetching()).toBe(0));
    expect(queryClient.getQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID))).toBeUndefined();
    expect(notifySpaceCommentDeleteSuccess).toHaveBeenCalledTimes(1);
  });

  it("still reports when the optimistic removal itself throws (SPC-055)", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID), [comment()]);
    const { client } = stubClient(() => ({ status: 204, body: "" }));
    vi.spyOn(queryClient, "cancelQueries").mockRejectedValueOnce(new Error("cancelled"));
    const { result } = renderHook(() => useDeleteSpaceComment(client, ANCHOR, ISSUE_ID), { wrapper });
    await result.current.mutateAsync(COMMENT_ID).catch(() => undefined);
    expect(notifySpaceCommentDeleteFailure).toHaveBeenCalledTimes(1);
  });
});

describe("useSpaceCommentReactions", () => {
  const guestRow = comment({ comment_reactions: [reaction()] });

  function signedClient(handler: (call: Recorded) => { status: number; body: unknown }) {
    return stubClient((call) =>
      call.url.endsWith("/api/users/me/") ? { status: 200, body: viewer() } : handler(call)
    );
  }

  it("groups the server rows and marks the visitor's own (SPC-057)", async () => {
    const mine = comment({ comment_reactions: [reaction(), reaction({ actor: ADA, display_name: "Ada L" })] });
    const { client } = signedClient(() => ({ status: 200, body: [] }));
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceViewerQueryOptions(client).queryKey, viewer());
    const { result } = renderHook(() => useSpaceCommentReactions(client, ANCHOR, ISSUE_ID, mine, () => {}), {
      wrapper,
    });
    await waitFor(() => expect(result.current.groups[0]).toMatchObject({ reacted: true }));
    expect(result.current.groups[0]).toMatchObject({ code: "127881", emoji: "🎉", count: 2, reacted: true });
  });

  it("sends anonymous activations to sign-in instead of writing (SPC-057)", async () => {
    const { client, calls } = stubClient(() => ({ status: 401, body: { error: "anonymous" } }));
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceViewerQueryOptions(client).queryKey, null);
    const onRequireSignIn = vi.fn();
    const { result } = renderHook(() => useSpaceCommentReactions(client, ANCHOR, ISSUE_ID, guestRow, onRequireSignIn), {
      wrapper,
    });
    await result.current.react("127881");
    expect(onRequireSignIn).toHaveBeenCalledTimes(1);
    expect(calls.some((call) => call.url.includes("/reactions/"))).toBe(false);
    expect(notifySpaceReactionFailure).not.toHaveBeenCalled();
  });

  it("treats a still-loading visitor as anonymous (SPC-057)", async () => {
    const transport: Transport = async (url: string) => {
      if (url.endsWith("/api/users/me/")) await new Promise(() => {});
      return { status: 200, url, headers: { get: () => "application/json" }, text: async () => "[]" };
    };
    const client = createClient({ baseUrl: "https://api.test", transport });
    const { wrapper } = harness();
    const onRequireSignIn = vi.fn();
    const { result } = renderHook(() => useSpaceCommentReactions(client, ANCHOR, ISSUE_ID, guestRow, onRequireSignIn), {
      wrapper,
    });
    await result.current.react("127881");
    expect(onRequireSignIn).toHaveBeenCalledTimes(1);
  });

  it("overlays a pending add until the write settles (SPC-057)", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { queryClient, wrapper } = harness();
    const posted = reaction({ actor: ADA, display_name: "Ada L" });
    const transport: Transport = async (url: string, init?: TransportInit) => {
      let body: unknown = [];
      let status = 200;
      if (url.endsWith("/api/users/me/")) {
        body = viewer();
      } else if ((init?.method ?? "GET") === "POST" && url.includes("/reactions/")) {
        await gate;
        status = 201;
        body = posted;
      }
      return { status, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(body) };
    };
    const client = createClient({ baseUrl: "https://api.test", transport });
    queryClient.setQueryData(spaceViewerQueryOptions(client).queryKey, viewer());
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useSpaceCommentReactions(client, ANCHOR, ISSUE_ID, guestRow, () => {}), {
      wrapper,
    });
    await waitFor(() => expect(result.current.groups[0]).toMatchObject({ count: 1, reacted: false }));
    const flight = result.current.react("127881");
    await waitFor(() => expect(result.current.groups[0]).toMatchObject({ count: 2, reacted: true }));
    expect(result.current.groups[0]?.names).toContainEqual({ name: "Ada L", mine: true });
    release();
    await flight;
    expect(invalidate).toHaveBeenCalledWith({ queryKey: spaceKeys.comments(ANCHOR, ISSUE_ID) });
    expect(notifySpaceReactionFailure).not.toHaveBeenCalled();
  });

  it("removes the visitor's row on re-click (SPC-057)", async () => {
    const mine = comment({ comment_reactions: [reaction({ actor: ADA, display_name: "Ada L" })] });
    const { client, calls } = signedClient(() => ({ status: 204, body: "" }));
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceViewerQueryOptions(client).queryKey, viewer());
    const { result } = renderHook(() => useSpaceCommentReactions(client, ANCHOR, ISSUE_ID, mine, () => {}), {
      wrapper,
    });
    await waitFor(() => expect(result.current.groups[0]).toMatchObject({ reacted: true }));
    await result.current.react("127881");
    const remove = calls.find((call) => call.method === "DELETE");
    expect(remove?.url).toBe(`https://api.test/api/public/anchor/anchor-1/comments/${COMMENT_ID}/reactions/127881/`);
    expect(notifySpaceReactionFailure).not.toHaveBeenCalled();
  });

  it("clears the overlay and reports a refused toggle (SPC-057)", async () => {
    const { client } = signedClient((call) => {
      if (call.url.endsWith("/api/users/me/")) return { status: 200, body: viewer() };
      return { status: 500, body: { error: "boom" } };
    });
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceViewerQueryOptions(client).queryKey, viewer());
    const { result } = renderHook(() => useSpaceCommentReactions(client, ANCHOR, ISSUE_ID, guestRow, () => {}), {
      wrapper,
    });
    await result.current.react("127881");
    expect(result.current.groups[0]).toMatchObject({ count: 1, reacted: false });
    expect(notifySpaceReactionFailure).toHaveBeenCalledTimes(1);
  });

  it("runs one write at a time (SPC-057)", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const posted = reaction({ actor: ADA, display_name: "Ada L" });
    const transport: Transport = async (url: string, init?: TransportInit) => {
      let body: unknown = [];
      let status = 200;
      if (url.endsWith("/api/users/me/")) {
        body = viewer();
      } else if ((init?.method ?? "GET") === "POST" && url.includes("/reactions/")) {
        await gate;
        status = 201;
        body = posted;
      }
      return { status, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(body) };
    };
    const writes: string[] = [];
    const counting: Transport = async (url, init) => {
      if ((init?.method ?? "GET") === "POST" && url.includes("/reactions/")) writes.push(url);
      return transport(url, init);
    };
    const counted = createClient({ baseUrl: "https://api.test", transport: counting });
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(spaceViewerQueryOptions(counted).queryKey, viewer());
    const { result } = renderHook(() => useSpaceCommentReactions(counted, ANCHOR, ISSUE_ID, guestRow, () => {}), {
      wrapper,
    });
    const first = result.current.react("127881");
    await waitFor(() => expect(result.current.groups[0]).toMatchObject({ reacted: true }));
    await result.current.react("127881");
    release();
    await first;
    expect(writes).toHaveLength(1);
  });
});

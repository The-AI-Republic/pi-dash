// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type SpaceBoardIssueDetail, type Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const embed = vi.hoisted(() => ({ framed: false }));
const notices = vi.hoisted(() => ({ voteFailure: vi.fn(), reactionFailure: vi.fn() }));

vi.mock("../lib/spaceEmbed.js", () => ({ useSpaceEmbedded: () => embed.framed }));
vi.mock("../lib/spaceNotices.js", () => ({
  notifySpaceVoteFailure: notices.voteFailure,
  notifySpaceReactionFailure: notices.reactionFailure,
}));

import { spaceKeys } from "../api/keys.js";
import { useSpaceEngagement } from "./useSpaceEngagement.js";

const ANCHOR = "anchor-1";
const ISSUE_ID = "11111111-1111-4111-8111-111111111111";
const VIEWER_ID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const GUEST_ID = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

const ME = {
  id: VIEWER_ID,
  email: "ada@example.test",
  username: "ada",
  first_name: "Ada",
  last_name: "Lovelace",
  display_name: "Ada Lovelace",
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

function actor(id: string, displayName: string): SpaceBoardIssueDetail["vote_items"][number]["actor_details"] {
  return { id, display_name: displayName, first_name: displayName, last_name: "", avatar: "", avatar_url: null };
}

function issue(): SpaceBoardIssueDetail {
  return {
    id: ISSUE_ID,
    name: "Public work item",
    state_id: null,
    sort_order: 1,
    estimate_point: null,
    priority: null,
    start_date: null,
    target_date: null,
    sequence_id: 7,
    project_id: "44444444-4444-4444-8444-444444444444",
    parent_id: null,
    created_by: null,
    state__group: "unstarted",
    cycle_id: null,
    assignee_ids: [],
    label_ids: [],
    module_ids: [],
    vote_items: [{ vote: 1, actor_details: actor(GUEST_ID, "Guest") }],
    reaction_items: [{ reaction: "127881", actor_details: actor(GUEST_ID, "Guest") }],
    description_json: null,
    description_html: "",
    description_stripped: null,
    description_binary: null,
  };
}

function reply(status: number, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status,
    url: "",
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

interface SetupOptions {
  viewer?: () => Awaited<ReturnType<Transport>> | Promise<never>;
  votes?: (method: string, body: string | undefined) => Awaited<ReturnType<Transport>>;
  reactions?: (method: string, body: string | undefined) => Awaited<ReturnType<Transport>>;
}

function setup(options: SetupOptions = {}) {
  const requests: Array<{ method: string; url: string; body?: string | undefined }> = [];
  const transport: Transport = (async (url, init) => {
    requests.push({ method: init?.method ?? "GET", url, body: init?.body as string | undefined });
    if (url.includes("/api/users/me/")) {
      return options.viewer === undefined ? reply(401, { detail: "anonymous" }) : options.viewer();
    }
    if (url.includes("/reactions/")) {
      if (options.reactions !== undefined) return options.reactions(init?.method ?? "GET", init?.body as string);
      return reply(200, {
        id: "55555555-5555-4555-8555-555555555555",
        actor_detail: { ...actor(VIEWER_ID, "Ada Lovelace"), is_bot: false },
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
        deleted_at: null,
        reaction: "127881",
        created_by: null,
        updated_by: null,
        project: "44444444-4444-4444-8444-444444444444",
        workspace: "44444444-4444-4444-8444-444444444444",
        actor: VIEWER_ID,
        issue: ISSUE_ID,
      });
    }
    if (url.includes("/votes/")) {
      if (options.votes !== undefined) return options.votes(init?.method ?? "GET", init?.body as string);
      return reply(200, {
        issue: ISSUE_ID,
        vote: 1,
        workspace: "44444444-4444-4444-8444-444444444444",
        project: "44444444-4444-4444-8444-444444444444",
        actor: VIEWER_ID,
        actor_detail: { ...actor(VIEWER_ID, "Ada Lovelace"), is_bot: false },
      });
    }
    throw new Error(`unexpected request ${url}`);
  }) as Transport;
  const client = createClient({ baseUrl: "https://api.test", transport });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(spaceKeys.detail(ANCHOR, ISSUE_ID), issue());
  const onRequireSignIn = vi.fn();
  const rendered = renderHook(() => useSpaceEngagement(client, ANCHOR, onRequireSignIn), {
    wrapper: ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    ),
  });
  return { rendered, queryClient, requests, onRequireSignIn };
}

afterEach(() => {
  embed.framed = false;
  vi.restoreAllMocks();
  notices.voteFailure.mockClear();
  notices.reactionFailure.mockClear();
});

describe("useSpaceEngagement viewer", () => {
  it("exposes the signed-in visitor's id", async () => {
    const { rendered } = setup({ viewer: () => reply(200, ME) });
    await waitFor(() => expect(rendered.result.current.viewerId).toBe(VIEWER_ID));
  });

  it("reads anonymous visitors as null", async () => {
    const { rendered } = setup();
    await waitFor(() => expect(rendered.result.current.viewerId).toBeNull());
    // The query settled (no longer loading) on the anonymous answer.
    expect(rendered.result.current.viewerId).toBeNull();
  });
});

describe("useSpaceEngagement vote", () => {
  it("sends anonymous voters to sign-in with no write", async () => {
    const { rendered, requests, onRequireSignIn } = setup();
    await waitFor(() => expect(rendered.result.current.viewerId).toBeNull());
    await rendered.result.current.vote(issue(), 1);
    expect(onRequireSignIn).toHaveBeenCalledTimes(1);
    expect(requests.filter((request) => request.url.includes("/votes/"))).toHaveLength(0);
  });

  it("sends voters to sign-in while identity still loads", async () => {
    const { rendered, requests, onRequireSignIn } = setup({ viewer: () => new Promise<never>(() => undefined) });
    expect(rendered.result.current.viewerId).toBeNull();
    await rendered.result.current.vote(issue(), 1);
    expect(onRequireSignIn).toHaveBeenCalledTimes(1);
    expect(requests.filter((request) => request.url.includes("/votes/"))).toHaveLength(0);
  });

  it("stays inert inside a frame without redirecting", async () => {
    embed.framed = true;
    const { rendered, requests, onRequireSignIn } = setup();
    await waitFor(() => expect(rendered.result.current.viewerId).toBeNull());
    await rendered.result.current.vote(issue(), 1);
    expect(onRequireSignIn).not.toHaveBeenCalled();
    expect(requests.filter((request) => request.url.includes("/votes/"))).toHaveLength(0);
  });

  it("posts a fresh vote and shows it optimistically", async () => {
    const { rendered, requests, queryClient } = setup({ viewer: () => reply(200, ME) });
    await waitFor(() => expect(rendered.result.current.viewerId).toBe(VIEWER_ID));
    await rendered.result.current.vote(issue(), 1);
    const writes = requests.filter((request) => request.url.includes("/votes/"));
    expect(writes).toHaveLength(1);
    expect(writes[0]?.method).toBe("POST");
    expect(writes[0]?.body).toContain('"vote":1');
    const detail = queryClient.getQueryData<SpaceBoardIssueDetail>(spaceKeys.detail(ANCHOR, ISSUE_ID));
    expect(detail?.vote_items).toHaveLength(2);
    expect(detail?.vote_items[1]).toMatchObject({ vote: 1, actor_details: { id: VIEWER_ID } });
    expect(queryClient.getQueryState(spaceKeys.detail(ANCHOR, ISSUE_ID))?.isInvalidated).toBe(true);
  });

  it("removes the vote when the same side is clicked again", async () => {
    const { rendered, requests, queryClient } = setup({ viewer: () => reply(200, ME) });
    await waitFor(() => expect(rendered.result.current.viewerId).toBe(VIEWER_ID));
    const mine = issue();
    mine.vote_items = [{ vote: 1, actor_details: actor(VIEWER_ID, "Ada Lovelace") }];
    await rendered.result.current.vote(mine, 1);
    const writes = requests.filter((request) => request.url.includes("/votes/"));
    expect(writes).toHaveLength(1);
    expect(writes[0]?.method).toBe("DELETE");
    const detail = queryClient.getQueryData<SpaceBoardIssueDetail>(spaceKeys.detail(ANCHOR, ISSUE_ID));
    expect(detail?.vote_items).toEqual([{ vote: 1, actor_details: actor(GUEST_ID, "Guest") }]);
  });

  it("moves the vote when the other side is clicked", async () => {
    const { rendered, requests } = setup({ viewer: () => reply(200, ME) });
    await waitFor(() => expect(rendered.result.current.viewerId).toBe(VIEWER_ID));
    const mine = issue();
    mine.vote_items = [{ vote: -1, actor_details: actor(VIEWER_ID, "Ada Lovelace") }];
    await rendered.result.current.vote(mine, 1);
    const writes = requests.filter((request) => request.url.includes("/votes/"));
    expect(writes).toHaveLength(1);
    expect(writes[0]?.method).toBe("POST");
    expect(writes[0]?.body).toContain('"vote":1');
  });

  it("restores the stored votes and reports a failed write", async () => {
    const { rendered, queryClient } = setup({
      viewer: () => reply(200, ME),
      votes: () => reply(500, { detail: "boom" }),
    });
    await waitFor(() => expect(rendered.result.current.viewerId).toBe(VIEWER_ID));
    await rendered.result.current.vote(issue(), 1);
    const detail = queryClient.getQueryData<SpaceBoardIssueDetail>(spaceKeys.detail(ANCHOR, ISSUE_ID));
    expect(detail?.vote_items).toEqual([{ vote: 1, actor_details: actor(GUEST_ID, "Guest") }]);
    expect(notices.voteFailure).toHaveBeenCalledTimes(1);
    expect(queryClient.getQueryState(spaceKeys.detail(ANCHOR, ISSUE_ID))?.isInvalidated).toBe(true);
  });
});

describe("useSpaceEngagement react", () => {
  it("sends anonymous reactors to sign-in with no write", async () => {
    const { rendered, requests, onRequireSignIn } = setup();
    await waitFor(() => expect(rendered.result.current.viewerId).toBeNull());
    await rendered.result.current.react(issue(), "127881");
    expect(onRequireSignIn).toHaveBeenCalledTimes(1);
    expect(requests.filter((request) => request.url.includes("/reactions/"))).toHaveLength(0);
  });

  it("sends reactors to sign-in while identity still loads, even inside a frame", async () => {
    embed.framed = true;
    const { rendered, requests, onRequireSignIn } = setup({ viewer: () => new Promise<never>(() => undefined) });
    await rendered.result.current.react(issue(), "127881");
    expect(onRequireSignIn).toHaveBeenCalledTimes(1);
    expect(requests.filter((request) => request.url.includes("/reactions/"))).toHaveLength(0);
  });

  it("posts a fresh reaction and shows it optimistically", async () => {
    const { rendered, requests, queryClient } = setup({ viewer: () => reply(200, ME) });
    await waitFor(() => expect(rendered.result.current.viewerId).toBe(VIEWER_ID));
    await rendered.result.current.react(issue(), "128077");
    const writes = requests.filter((request) => request.url.includes("/reactions/"));
    expect(writes).toHaveLength(1);
    expect(writes[0]?.method).toBe("POST");
    expect(writes[0]?.body).toContain('"reaction":"128077"');
    const detail = queryClient.getQueryData<SpaceBoardIssueDetail>(spaceKeys.detail(ANCHOR, ISSUE_ID));
    expect(detail?.reaction_items).toHaveLength(2);
    expect(detail?.reaction_items[1]).toMatchObject({ reaction: "128077", actor_details: { id: VIEWER_ID } });
  });

  it("removes the reaction when the visitor's own group is clicked", async () => {
    const { rendered, requests } = setup({ viewer: () => reply(200, ME) });
    await waitFor(() => expect(rendered.result.current.viewerId).toBe(VIEWER_ID));
    const mine = issue();
    mine.reaction_items = [{ reaction: "127881", actor_details: actor(VIEWER_ID, "Ada Lovelace") }];
    await rendered.result.current.react(mine, "127881");
    const writes = requests.filter((request) => request.url.includes("/reactions/"));
    expect(writes).toHaveLength(1);
    expect(writes[0]?.method).toBe("DELETE");
    expect(writes[0]?.url).toContain("/reactions/127881/");
  });

  it("restores the stored reactions and reports a failed write", async () => {
    const { rendered, queryClient } = setup({
      viewer: () => reply(200, ME),
      reactions: () => reply(400, { detail: "disabled" }),
    });
    await waitFor(() => expect(rendered.result.current.viewerId).toBe(VIEWER_ID));
    await rendered.result.current.react(issue(), "128077");
    const detail = queryClient.getQueryData<SpaceBoardIssueDetail>(spaceKeys.detail(ANCHOR, ISSUE_ID));
    expect(detail?.reaction_items).toEqual([{ reaction: "127881", actor_details: actor(GUEST_ID, "Guest") }]);
    expect(notices.reactionFailure).toHaveBeenCalledTimes(1);
  });
});

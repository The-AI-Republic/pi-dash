// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type SpaceBoardIssueDetail, type Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ client: null as unknown, queryClient: null as unknown }));
const embed = vi.hoisted(() => ({ framed: false }));
const notices = vi.hoisted(() => ({ voteFailure: vi.fn(), reactionFailure: vi.fn() }));

vi.mock("../api/client.js", () => ({ spaceRuntime: runtime }));
vi.mock("../lib/spaceEmbed.js", () => ({ useSpaceEmbedded: () => embed.framed }));
vi.mock("../lib/spaceNotices.js", () => ({
  notifySpaceVoteFailure: notices.voteFailure,
  notifySpaceReactionFailure: notices.reactionFailure,
}));

import type { SpaceBoardSearch } from "../lib/address.js";
import { defaultSpaceSignInRedirect, SpaceIssueEngagement } from "./SpaceIssueEngagement.js";

const ANCHOR = "anchor-1";
const ISSUE_ID = "11111111-1111-4111-8111-111111111111";
const VIEWER_ID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

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
    vote_items: [],
    reaction_items: [
      {
        reaction: "127881",
        actor_details: {
          id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
          display_name: "Guest",
          first_name: "Guest",
          last_name: "",
          avatar: "",
          avatar_url: null,
        },
      },
    ],
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
  viewer?: () => Awaited<ReturnType<Transport>>;
  votesEnabled?: boolean;
  reactionsEnabled?: boolean;
  embedded?: boolean;
  search?: SpaceBoardSearch;
  onRequireSignIn?: () => void;
}

function setup(options: SetupOptions = {}) {
  const requests: string[] = [];
  const transport: Transport = (async (url) => {
    requests.push(url);
    if (url.includes("/api/users/me/")) {
      return options.viewer === undefined ? reply(401, { detail: "anonymous" }) : options.viewer();
    }
    if (url.includes("/votes/")) {
      return reply(200, {
        issue: ISSUE_ID,
        vote: 1,
        workspace: "44444444-4444-4444-8444-444444444444",
        project: "44444444-4444-4444-8444-444444444444",
        actor: VIEWER_ID,
        actor_detail: {
          id: VIEWER_ID,
          first_name: "Ada",
          last_name: "Lovelace",
          avatar: "",
          avatar_url: null,
          display_name: "Ada Lovelace",
          is_bot: false,
        },
      });
    }
    if (url.includes("/reactions/")) {
      return reply(200, {
        id: "55555555-5555-4555-8555-555555555555",
        actor_detail: {
          id: VIEWER_ID,
          first_name: "Ada",
          last_name: "Lovelace",
          avatar: "",
          avatar_url: null,
          display_name: "Ada Lovelace",
          is_bot: false,
        },
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
    throw new Error(`unexpected request ${url}`);
  }) as Transport;
  runtime.client = createClient({ baseUrl: "https://api.test", transport });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onRequireSignIn = options.onRequireSignIn ?? vi.fn();
  render(
    <QueryClientProvider client={queryClient}>
      <SpaceIssueEngagement
        anchor={ANCHOR}
        search={options.search ?? { board: "kanban" }}
        issue={issue()}
        votesEnabled={options.votesEnabled ?? true}
        reactionsEnabled={options.reactionsEnabled ?? true}
        embedded={options.embedded ?? false}
        onRequireSignIn={onRequireSignIn}
      />
    </QueryClientProvider>
  );
  return { requests, onRequireSignIn: onRequireSignIn as ReturnType<typeof vi.fn> };
}

afterEach(() => {
  embed.framed = false;
  vi.restoreAllMocks();
  notices.voteFailure.mockClear();
  notices.reactionFailure.mockClear();
});

describe("SpaceIssueEngagement", () => {
  it("renders nothing when both flags are off", () => {
    setup({ votesEnabled: false, reactionsEnabled: false });
    expect(screen.queryByTestId("space-peek-engagement")).not.toBeInTheDocument();
  });

  it("renders each half only while its flag is on", () => {
    const { unmount } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <SpaceIssueEngagement
          anchor={ANCHOR}
          search={{}}
          issue={issue()}
          votesEnabled={true}
          reactionsEnabled={false}
          embedded={false}
          onRequireSignIn={vi.fn()}
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("space-peek-votes")).toBeInTheDocument();
    expect(screen.queryByTestId("space-peek-reactions")).not.toBeInTheDocument();
    unmount();
  });

  it("votes and reacts from the overlay as a signed-in visitor", async () => {
    const { requests, onRequireSignIn } = setup({ viewer: () => reply(200, ME) });
    await userEvent.click(screen.getByTestId("space-peek-vote-up"));
    await waitFor(() => expect(requests.some((url) => url.includes("/votes/"))).toBe(true));
    expect(onRequireSignIn).not.toHaveBeenCalled();
    await userEvent.click(screen.getByTestId("space-peek-reaction"));
    await userEvent.click(screen.getByTestId("space-peek-reaction-add"));
    await userEvent.click(screen.getByRole("button", { name: "🎉" }));
    await waitFor(() => expect(requests.filter((url) => url.includes("/reactions/")).length).toBeGreaterThan(0));
    expect(onRequireSignIn).not.toHaveBeenCalled();
  });

  it("renders reactions alone when only that flag is on", () => {
    setup({ votesEnabled: false, reactionsEnabled: true });
    expect(screen.queryByTestId("space-peek-votes")).not.toBeInTheDocument();
    expect(screen.getByTestId("space-peek-reactions")).toBeInTheDocument();
    expect(screen.getByTestId("space-peek-reaction")).toHaveAttribute("data-reaction", "127881");
  });

  it("sends anonymous activations to sign-in with no write", async () => {
    const { requests, onRequireSignIn } = setup();
    await userEvent.click(screen.getByTestId("space-peek-vote-up"));
    expect(onRequireSignIn).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByTestId("space-peek-reaction-add"));
    await userEvent.click(screen.getByRole("button", { name: "🎉" }));
    expect(onRequireSignIn).toHaveBeenCalledTimes(2);
    expect(requests.filter((url) => url.includes("/votes/") || url.includes("/reactions/"))).toHaveLength(0);
  });

  it("keeps inert votes and hides reactions inside a frame", async () => {
    const { requests, onRequireSignIn } = setup({ viewer: () => reply(200, ME), embedded: true });
    expect(screen.queryByTestId("space-peek-reactions")).not.toBeInTheDocument();
    const up = screen.getByTestId("space-peek-vote-up");
    expect(up).toBeDisabled();
    await userEvent.click(up);
    expect(onRequireSignIn).not.toHaveBeenCalled();
    expect(requests.filter((url) => url.includes("/votes/"))).toHaveLength(0);
  });

  it("renders nothing framed when only reactions are enabled", () => {
    setup({ votesEnabled: false, reactionsEnabled: true, embedded: true });
    expect(screen.queryByTestId("space-peek-engagement")).not.toBeInTheDocument();
  });
});

describe("defaultSpaceSignInRedirect", () => {
  it("navigates to sign-in carrying the whole board position", () => {
    const assign = vi.fn();
    const location = window.location;
    Object.defineProperty(window, "location", { configurable: true, writable: true, value: { assign } });
    try {
      defaultSpaceSignInRedirect(ANCHOR, { board: "kanban", peekId: ISSUE_ID });
      expect(assign).toHaveBeenCalledWith(`/spaces/?next_path=/issues/anchor-1/?board=kanban&peekId=${ISSUE_ID}`);
    } finally {
      Object.defineProperty(window, "location", { configurable: true, writable: true, value: location });
    }
  });

  it("has nowhere to go outside a browser window", () => {
    vi.stubGlobal("window", undefined as unknown as Window & typeof globalThis);
    try {
      expect(() => defaultSpaceSignInRedirect(ANCHOR, {})).not.toThrow();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("is the overlay's fallback when the board passes no handler", async () => {
    const assign = vi.fn();
    const location = window.location;
    Object.defineProperty(window, "location", { configurable: true, writable: true, value: { assign } });
    try {
      const transport: Transport = (async () => reply(401, { detail: "anonymous" })) as Transport;
      runtime.client = createClient({ baseUrl: "https://api.test", transport });
      render(
        <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
          <SpaceIssueEngagement
            anchor={ANCHOR}
            search={{ board: "kanban" }}
            issue={issue()}
            votesEnabled={true}
            reactionsEnabled={false}
            embedded={false}
          />
        </QueryClientProvider>
      );
      await userEvent.click(screen.getByTestId("space-peek-vote-up"));
      expect(assign).toHaveBeenCalledWith("/spaces/?next_path=/issues/anchor-1/?board=kanban");
    } finally {
      Object.defineProperty(window, "location", { configurable: true, writable: true, value: location });
    }
  });
});

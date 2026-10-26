// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceComment, Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ client: null as unknown }));

vi.mock("../api/client.js", () => ({ spaceRuntime: runtime }));

import { createClient } from "@pidash/api-client";

import { SpaceCommentsSection, spaceCommentReturnPath, spaceCommentsSignInHref } from "./SpaceCommentsSection.js";

const ISSUE_ID = "11111111-1111-4111-8111-111111111111";
const ANCHOR = "anchor-1";
const ADA = "523e4567-e89b-12d3-a456-426614174004";

function comment(overrides: Partial<SpaceComment> = {}): SpaceComment {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
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

function reply(status: number, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status,
    url: "",
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

interface SetupOptions {
  comments?: () => Promise<Awaited<ReturnType<Transport>>> | Awaited<ReturnType<Transport>>;
  enabled?: boolean;
  embedded?: boolean;
  viewerId?: string | null;
  composer?: React.ReactNode;
}

function setup(options: SetupOptions = {}) {
  const requests: string[] = [];
  const transport: Transport = async (url) => {
    requests.push(url);
    return await (options.comments?.() ?? reply(200, []));
  };
  const client = createClient({ baseUrl: "https://api.test", transport });
  runtime.client = client;
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <SpaceCommentsSection
        anchor={ANCHOR}
        issueId={ISSUE_ID}
        search={{ board: "kanban", peekId: ISSUE_ID, state: [ISSUE_ID] }}
        enabled={options.enabled ?? true}
        viewerId={options.viewerId === undefined ? ADA : options.viewerId}
        embedded={options.embedded}
        resolveAsset={(src) => src}
        composer={options.composer}
      />
    </QueryClientProvider>
  );
  return { requests, ...view };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("SpaceCommentsSection gating", () => {
  it("renders nothing and makes no request while comments are disabled (SPC-049)", () => {
    const { requests } = setup({ enabled: false });
    expect(screen.queryByTestId("space-comments")).not.toBeInTheDocument();
    expect(requests).toEqual([]);
  });

  it("shows the empty thread to everyone while enabled", async () => {
    setup();
    expect(await screen.findByTestId("space-comments-empty")).toHaveTextContent("No comments yet.");
    expect(screen.getByRole("heading", { name: "Comments" })).toBeInTheDocument();
  });

  it("shows the composer slot to a signed-in visitor", async () => {
    setup({ composer: <button type="button">Write a comment</button> });
    expect(await screen.findByRole("button", { name: "Write a comment" })).toBeInTheDocument();
    expect(screen.queryByTestId("space-comments-signin")).not.toBeInTheDocument();
  });

  it("invites an anonymous visitor to sign in with the full position", async () => {
    setup({ viewerId: null });
    const link = await screen.findByTestId("space-comments-signin");
    expect(link).toHaveAttribute(
      "href",
      "/spaces/?next_path=/issues/anchor-1/%3Fboard%3Dkanban%26peekId%3D11111111-1111-4111-8111-111111111111%26state%3D11111111-1111-4111-8111-111111111111"
    );
    expect(link).toHaveTextContent("Sign in to add your comment");
  });

  it("suppresses the anonymous invitation inside a frame", async () => {
    setup({ viewerId: null, embedded: true });
    await screen.findByTestId("space-comments-empty");
    expect(screen.queryByTestId("space-comments-signin")).not.toBeInTheDocument();
  });
});

describe("SpaceCommentsSection thread states", () => {
  it("renders one row per comment", async () => {
    setup({
      comments: () =>
        reply(200, [
          comment(),
          comment({
            id: "223e4567-e89b-12d3-a456-426614174001",
            comment_html: "<p>Second</p>",
            comment_stripped: "Second",
          }),
        ]),
    });
    expect(await screen.findByText("Hello")).toBeInTheDocument();
    expect(screen.getByText("Second")).toBeInTheDocument();
  });

  it("shows a loading placeholder until the read settles", () => {
    setup({ comments: () => new Promise(() => undefined) });
    expect(screen.getByTestId("space-comments-loading")).toBeInTheDocument();
  });

  it("shows the failure message and retries the read", async () => {
    let attempts = 0;
    const user = userEvent.setup();
    setup({
      comments: () => {
        attempts += 1;
        return attempts === 1 ? reply(500, { detail: "down" }) : reply(200, [comment()]);
      },
    });
    expect(await screen.findByTestId("space-comments-error")).toHaveTextContent("Comments could not be loaded.");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(screen.getByText("Hello")).toBeInTheDocument());
    expect(attempts).toBe(2);
  });
});

describe("comment invitation addresses", () => {
  it("keeps board, view and open issue in both helpers", () => {
    const search = { board: "list" as const, peekId: ISSUE_ID, priority: ["high" as const] };
    expect(spaceCommentReturnPath(ANCHOR, search)).toBe(
      `/issues/anchor-1/?board=list&peekId=${ISSUE_ID}&priority=high`
    );
    expect(spaceCommentsSignInHref(ANCHOR, search)).toBe(
      `/spaces/?next_path=/issues/anchor-1/%3Fboard%3Dlist%26peekId%3D${ISSUE_ID}%26priority%3Dhigh`
    );
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue reaction row (ISS-204): server chips, the viewer highlight, the
// add picker and withdrawal, and the archived surface that drops it.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IssueReaction, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

vi.mock("../../../core/session/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/session/index.js")>();
  return { ...original, useMe: () => ({ data: { id: "523e4567-e89b-12d3-a456-426614174004" } }) };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifySuccess } from "../../../shared/shell/index.js";
import { REACTION_CREATED_TITLE, REACTION_REMOVED_TITLE } from "./api.js";
import { EMOJI_PICKER_LABEL } from "./EmojiPicker.js";
import { ISSUE_REACTIONS_LABEL, IssueReactions } from "./IssueReactions.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const ADA = "523e4567-e89b-12d3-a456-426614174004";
const BOB = "623e4567-e89b-12d3-a456-426614174005";

const BOB_DETAIL = {
  id: BOB,
  first_name: "Bob",
  last_name: "B",
  avatar: "",
  avatar_url: null,
  display_name: "Bob B",
  is_bot: false,
};

function reaction(overrides: Partial<IssueReaction> = {}): IssueReaction {
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
    reaction: "1f44d",
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

function stubTransport(handler: (call: Recorded) => { status: number; body: unknown }) {
  const calls: Recorded[] = [];
  createAppClient("https://api.test", async (url: string, init: TransportInit) => {
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
  return calls;
}

function renderRow(props: Partial<React.ComponentProps<typeof IssueReactions>> = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <IssueReactions workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} {...props} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.mocked(notifySuccess).mockClear();
});

afterEach(() => {
  resetAppClient();
});

describe("IssueReactions", () => {
  it("renders nothing on an archived issue and reads no reactions", () => {
    const calls = stubTransport(() => ({ status: 200, body: [] }));
    renderRow({ disabled: true });
    expect(screen.queryByRole("region", { name: ISSUE_REACTIONS_LABEL })).not.toBeInTheDocument();
    expect(calls.filter((call) => call.url.includes("/reactions/"))).toEqual([]);
  });

  it("shows the server chips with the viewer's own highlighted", async () => {
    stubTransport(() => ({
      status: 200,
      body: [
        reaction(),
        reaction({ id: "623e4567-e89b-12d3-a456-426614174007", actor: BOB, actor_detail: BOB_DETAIL }),
      ],
    }));
    renderRow();
    const chip = await screen.findByRole("button", { name: "👍 reaction, 2" });
    expect(chip).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("region", { name: ISSUE_REACTIONS_LABEL })).toBeInTheDocument();
  });

  it("leaves the add button on an issue nobody reacted to", () => {
    stubTransport(() => ({ status: 200, body: [] }));
    renderRow();
    expect(screen.getByRole("button", { name: EMOJI_PICKER_LABEL })).toBeInTheDocument();
  });

  it("posts the picked emoji against the issue and notices it", async () => {
    const user = userEvent.setup();
    const calls = stubTransport((call) =>
      call.method === "POST" ? { status: 201, body: reaction({ reaction: "1f440" }) } : { status: 200, body: [] }
    );
    renderRow();
    await user.click(screen.getByRole("button", { name: EMOJI_PICKER_LABEL }));
    await user.click(await screen.findByRole("option", { name: "👀" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(REACTION_CREATED_TITLE));
    const post = calls.find((call) => call.method === "POST");
    expect(post?.url).toContain(`/issues/${ISSUE}/reactions/`);
    expect(post?.body).toBe(JSON.stringify({ reaction: "1f440" }));
  });

  it("withdraws the viewer's reaction from a highlighted chip", async () => {
    const user = userEvent.setup();
    const calls = stubTransport((call) =>
      call.method === "DELETE" ? { status: 204, body: "" } : { status: 200, body: [reaction()] }
    );
    renderRow();
    await user.click(await screen.findByRole("button", { name: "👍 reaction, 1" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(REACTION_REMOVED_TITLE));
    const del = calls.find((call) => call.method === "DELETE");
    expect(del?.url).toContain(`/issues/${ISSUE}/reactions/1f44d/`);
  });

  it("adds from a chip someone else picked instead of withdrawing", async () => {
    const user = userEvent.setup();
    const calls = stubTransport((call) =>
      call.method === "POST"
        ? { status: 201, body: reaction() }
        : { status: 200, body: [reaction({ actor: BOB, actor_detail: BOB_DETAIL })] }
    );
    renderRow();
    const chip = await screen.findByRole("button", { name: "👍 reaction, 1" });
    expect(chip).toHaveAttribute("aria-pressed", "false");
    await user.click(chip);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(REACTION_CREATED_TITLE));
    expect(calls.some((call) => call.method === "POST")).toBe(true);
  });
});

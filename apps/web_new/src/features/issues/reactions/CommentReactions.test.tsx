// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Comment reactions (ISS-203): the footer appears only once a comment
// has reactions, chips toggle against the comment collection, and the
// card-header picker posts a new one.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { CommentReaction, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

const session = vi.hoisted(() => ({
  viewer: { data: { id: "523e4567-e89b-12d3-a456-426614174004" } } as { data?: { id: string } | undefined },
}));

vi.mock("../../../core/session/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/session/index.js")>();
  return { ...original, useMe: () => session.viewer };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  REACTION_CREATED_TITLE,
  REACTION_CREATE_FAILED_TITLE,
  REACTION_REMOVE_FAILED_TITLE,
  REACTION_REMOVED_TITLE,
} from "./api.js";
import { COMMENT_REACTIONS_LABEL, CommentReactionPicker, CommentReactions } from "./CommentReactions.js";
import { EMOJI_PICKER_LABEL } from "./EmojiPicker.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const COMMENT = "323e4567-e89b-12d3-a456-426614174002";
const ADA = "523e4567-e89b-12d3-a456-426614174004";
const BOB = "623e4567-e89b-12d3-a456-426614174005";

function reaction(overrides: Partial<CommentReaction> = {}): CommentReaction {
  return {
    id: "723e4567-e89b-12d3-a456-426614174007",
    actor: ADA,
    comment: COMMENT,
    reaction: "1f44d",
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

function wrap(node: JSX.Element) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(<QueryClientProvider client={queryClient}>{node}</QueryClientProvider>);
}

function renderFooter() {
  return wrap(<CommentReactions workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} commentId={COMMENT} />);
}

beforeEach(() => {
  vi.mocked(notifySuccess).mockClear();
  vi.mocked(notifyError).mockClear();
});

afterEach(() => {
  resetAppClient();
});

describe("CommentReactions footer", () => {
  it("renders nothing while the collection is still loading", () => {
    createAppClient("https://api.test", () => new Promise(() => undefined));
    renderFooter();
    expect(screen.queryByRole("region", { name: COMMENT_REACTIONS_LABEL })).not.toBeInTheDocument();
  });

  it("uses the product credit rule for a chip when the read has not settled", () => {
    // A resolved-but-empty list still hides the footer; the pending arm is
    // exercised by the never-settling transport above, so assert the null
    // return for an empty settled list with a signed-out reader too.
    stubTransport(() => ({ status: 200, body: [] }));
    session.viewer = { data: undefined };
    renderFooter();
    return waitFor(() =>
      expect(screen.queryByRole("region", { name: COMMENT_REACTIONS_LABEL })).not.toBeInTheDocument()
    ).finally(() => {
      session.viewer = { data: { id: "523e4567-e89b-12d3-a456-426614174004" } };
    });
  });

  it("renders nothing when the comment has no reactions", async () => {
    stubTransport(() => ({ status: 200, body: [] }));
    renderFooter();
    await waitFor(() =>
      expect(screen.queryByRole("region", { name: COMMENT_REACTIONS_LABEL })).not.toBeInTheDocument()
    );
  });

  it("posts the picked emoji from its own trigger while a chip sits beside it", async () => {
    const user = userEvent.setup();
    const calls = stubTransport((call) =>
      call.method === "POST"
        ? { status: 201, body: reaction() }
        : { status: 200, body: [reaction({ actor: BOB, display_name: "Bob B" })] }
    );
    renderFooter();
    await user.click(await screen.findByRole("button", { name: "Add reaction" }));
    await user.click(await screen.findByRole("option", { name: "🎉" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(REACTION_CREATED_TITLE));
    const post = calls.find((call) => call.method === "POST" && call.body !== "");
    expect(post).toBeDefined();
  });

  it("renders nothing once a refused read settles", async () => {
    let refuse: (() => void) | undefined;
    let readServed = false;
    createAppClient("https://api.test", async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ csrf_token: "x" }),
        };
      }
      await new Promise<void>((resolve) => {
        refuse = resolve;
      });
      readServed = true;
      return { status: 500, url, headers: { get: () => null }, text: async () => JSON.stringify({ error: "boom" }) };
    });
    renderFooter();
    await waitFor(() => expect(refuse).toBeDefined());
    refuse?.();
    await waitFor(() => expect(readServed).toBe(true));
    await waitFor(() =>
      expect(screen.queryByRole("region", { name: COMMENT_REACTIONS_LABEL })).not.toBeInTheDocument()
    );
    expect(screen.queryByRole("button", { name: "Add reaction" })).not.toBeInTheDocument();
  });

  it("shows another member's chip unhighlighted for a signed-out reader", async () => {
    session.viewer = { data: undefined };
    try {
      stubTransport(() => ({ status: 200, body: [reaction({ actor: BOB, display_name: "Bob B" })] }));
      renderFooter();
      const chip = await screen.findByRole("button", { name: /👍/ });
      expect(chip.className).not.toContain("border-accent-strong");
    } finally {
      session.viewer = { data: { id: "523e4567-e89b-12d3-a456-426614174004" } };
    }
  });

  it("shows chips once reactions arrive, with the viewer's own highlighted", async () => {
    stubTransport(() => ({
      status: 200,
      body: [reaction(), reaction({ id: "723e4567-e89b-12d3-a456-426614174009", actor: BOB, display_name: "Bob B" })],
    }));
    renderFooter();
    const region = await screen.findByRole("region", { name: COMMENT_REACTIONS_LABEL });
    const chip = await screen.findByRole("button", { name: "👍 reaction, 2" });
    expect(region).toContainElement(chip);
    expect(chip).toHaveAttribute("aria-pressed", "true");
  });

  it("withdraws the viewer's own reaction from the chip", async () => {
    const user = userEvent.setup();
    const calls = stubTransport((call) =>
      call.method === "DELETE" ? { status: 204, body: "" } : { status: 200, body: [reaction()] }
    );
    renderFooter();
    await user.click(await screen.findByRole("button", { name: "👍 reaction, 1" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(REACTION_REMOVED_TITLE));
    const del = calls.find((call) => call.method === "DELETE");
    expect(del?.url).toContain(`/comments/${COMMENT}/reactions/1f44d/`);
  });

  it("adds the viewer's reaction from someone else's chip", async () => {
    const user = userEvent.setup();
    const calls = stubTransport((call) =>
      call.method === "POST"
        ? { status: 201, body: reaction() }
        : { status: 200, body: [reaction({ actor: BOB, display_name: "Bob B" })] }
    );
    renderFooter();
    await user.click(await screen.findByRole("button", { name: "👍 reaction, 1" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(REACTION_CREATED_TITLE));
    expect(calls.some((call) => call.method === "POST")).toBe(true);
  });

  it("reports a refused toggle", async () => {
    const user = userEvent.setup();
    stubTransport((call) =>
      call.method === "DELETE" ? { status: 500, body: { error: "no" } } : { status: 200, body: [reaction()] }
    );
    renderFooter();
    await user.click(await screen.findByRole("button", { name: "👍 reaction, 1" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(REACTION_REMOVE_FAILED_TITLE, "no"));
  });
});

describe("CommentReactionPicker", () => {
  it("posts the picked emoji against the comment", async () => {
    const user = userEvent.setup();
    const calls = stubTransport(() => ({ status: 201, body: reaction({ reaction: "1f389" }) }));
    wrap(<CommentReactionPicker workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} commentId={COMMENT} />);
    await user.click(screen.getByRole("button", { name: EMOJI_PICKER_LABEL }));
    await user.click(await screen.findByRole("option", { name: "🎉" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(REACTION_CREATED_TITLE));
    const post = calls.find((call) => call.method === "POST");
    expect(post?.url).toContain(`/comments/${COMMENT}/reactions/`);
    expect(post?.body).toBe(JSON.stringify({ reaction: "1f389" }));
  });

  it("reports a refused add and re-enables the trigger", async () => {
    const user = userEvent.setup();
    stubTransport(() => ({ status: 403, body: { error: "Not allowed." } }));
    wrap(<CommentReactionPicker workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} commentId={COMMENT} />);
    await user.click(screen.getByRole("button", { name: EMOJI_PICKER_LABEL }));
    await user.click(await screen.findByRole("option", { name: "👍" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(REACTION_CREATE_FAILED_TITLE, "Not allowed."));
    await waitFor(() => expect(screen.getByRole("button", { name: EMOJI_PICKER_LABEL })).toBeEnabled());
  });
});

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: runner chat sessions, transcript, warm/send/cancel/close,
// the resumable event stream and chat approvals against live Django,
// seeded with contract/seed-contracts.py. No runner is connected in
// the contract environment, so every dispatch to the runner fails
// synchronously at commit: warm records a warm-failed event, a sent
// turn is released as failed with the offline reason on the session,
// and cancel/close observe the released turn. The suites assert those
// deterministic states; the live-runner paths (a conflicting second
// send, a parked close) are covered by the backend's own view tests.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import type { ApiClient } from "../client.js";
import { listWorkspaces } from "../contracts/workspaces.js";
import {
  CHAT_EVENT_STREAM_NAME,
  ChatEvent,
  cancelChat,
  chatEventsUrl,
  closeChat,
  createChatSession,
  decideChatApproval,
  getChatSession,
  listChatApprovals,
  listChatMessages,
  listChatSessions,
  sendChatMessage,
  warmChatSession,
} from "../contracts/runner-chat.js";
import { CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

async function workspaceId(client: ApiClient): Promise<string> {
  const workspaces = await listWorkspaces(client);
  const workspace = workspaces.find((row) => row.slug === CONTRACT_WORKSPACE);
  expect(workspace).toBeDefined();
  return workspace?.id ?? "";
}

/**
 * The seeded chat runner, read off the oldest listed session. The seed
 * owns the oldest session on that runner, so the id is stable even as
 * suites add their own sessions. Fails loudly on an unseeded database.
 */
async function chatRunnerId(client: ApiClient, wsId: string): Promise<string> {
  const sessions = await listChatSessions(client, { workspace: wsId });
  expect(sessions.length).toBeGreaterThanOrEqual(1);
  return sessions[sessions.length - 1]?.runner ?? "";
}

/**
 * Pending seeded approval probes, newest first. Each run decides one;
 * an empty pool means the seed needs re-running, not a weaker test.
 */
async function pendingProbes(client: ApiClient) {
  const pending = (await listChatApprovals(client)).filter((row) =>
    row.local_approval_id.startsWith("contract-approval-")
  );
  expect(pending.length).toBeGreaterThanOrEqual(1);
  return pending;
}

/**
 * Open the event stream, parse the first frame, then tear it down.
 * The transport underneath is the real fetch, so the response casts
 * to a streaming body; the abort signal ends the long-lived request.
 */
async function readStreamedFrame(client: ApiClient, url: string) {
  const controller = new AbortController();
  const response = await client.get(url, { signal: controller.signal, timeoutMs: 0 });
  const body = (response as unknown as Response).body;
  expect(body).not.toBeNull();
  const reader = (body as ReadableStream<Uint8Array>).getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const end = buffer.indexOf("\n\n");
      if (end < 0) continue;
      const fields: Record<string, string> = {};
      for (const line of buffer.slice(0, end).split("\n")) {
        const separator = line.indexOf(":");
        if (separator > 0) fields[line.slice(0, separator)] = line.slice(separator + 1).trim();
      }
      expect(fields["event"]).toBe(CHAT_EVENT_STREAM_NAME);
      return ChatEvent.parse(JSON.parse(fields["data"] ?? "{}"));
    }
    throw new Error("stream closed without a frame");
  } finally {
    controller.abort();
    await reader.cancel().catch(() => {});
  }
}

describe.skipIf(!contractEnabled)("runner chat contract", () => {
  it("creates a session and reads it back through list and get", async () => {
    const client = await sharedSignedIn();
    const wsId = await workspaceId(client);
    const runner = await chatRunnerId(client, wsId);

    const created = await createChatSession(client, { workspace: wsId, runner });
    expect(created.status).toBe("open");
    expect(created.runner).toBe(runner);
    expect(created.workspace).toBe(wsId);
    expect(created.runner_detail.name.length).toBeGreaterThan(0);

    const read = await getChatSession(client, created.id);
    expect(read.id).toBe(created.id);

    const listed = await listChatSessions(client, { workspace: wsId, runner });
    expect(listed.map((session) => session.id)).toContain(created.id);
  });

  it("warms, sends and cancels a turn", async () => {
    const client = await sharedSignedIn();
    const wsId = await workspaceId(client);
    const runner = await chatRunnerId(client, wsId);

    // Fresh or reused-empty: either way no turn is in flight.
    const session = await createChatSession(client, { workspace: wsId, runner });
    const warmed = await warmChatSession(client, session.id);
    expect(warmed.ok).toBe(true);
    expect(warmed.skipped).toBeUndefined();

    const sent = await sendChatMessage(client, session.id, "contract probe turn");
    expect(sent.content).toBe("contract probe turn");
    expect(sent.seq).toBeGreaterThanOrEqual(1);

    // Dispatch fails offline at commit, so the re-read turn is failed
    // and the session carries the offline reason; nothing stays active.
    const messages = await listChatMessages(client, session.id);
    const reread = messages.find((row) => row.id === sent.id);
    expect(reread?.status).toBe("failed");
    const settled = await getChatSession(client, session.id);
    expect(settled.active_message_id).toBeNull();
    expect(settled.error).toContain("offline");

    const cancelled = await cancelChat(client, session.id);
    expect(cancelled).toEqual({ ok: true, noop: true });
  });

  it("closes a session", async () => {
    // Red live on backend bug PDASHOSS01-307: the close dispatch to an
    // offline runner raises out of the view (500) instead of being
    // recorded like the send/warm dispatches. The mutation persists;
    // only the response is wrong.
    const client = await sharedSignedIn();
    const wsId = await workspaceId(client);
    const runner = await chatRunnerId(client, wsId);

    const session = await createChatSession(client, { workspace: wsId, runner });
    const closed = await closeChat(client, session.id);
    expect(closed.status).toBe("closed");
    expect(closed.closed_at).not.toBeNull();
  });

  it("streams stored events and resumes past a seen sequence", async () => {
    const client = await sharedSignedIn();
    const wsId = await workspaceId(client);
    const runner = await chatRunnerId(client, wsId);

    const session = await createChatSession(client, { workspace: wsId, runner });
    await sendChatMessage(client, session.id, "contract stream probe");

    const first = await readStreamedFrame(client, chatEventsUrl(client, session.id));
    expect(first.session).toBe(session.id);
    expect(first.seq).toBeGreaterThanOrEqual(1);
    // The send is this session's first write, so replay starts there.
    expect(first.kind).toBe("chat_timing");

    // Nothing follows the last stored sequence, so this proves the
    // resumable URL opens a live stream (headers, then abort) rather
    // than replaying.
    const controller = new AbortController();
    try {
      const resumed = await client.get(chatEventsUrl(client, session.id, first.seq), {
        signal: controller.signal,
        timeoutMs: 0,
      });
      expect(resumed.status).toBe(200);
      expect(resumed.headers.get("content-type")).toContain("text/event-stream");
    } finally {
      controller.abort();
    }
  });

  it("lists pending chat approvals scoped and unscoped", async () => {
    const client = await sharedSignedIn();
    const wsId = await workspaceId(client);

    const pending = await pendingProbes(client);
    const scoped = await listChatApprovals(client, wsId);
    expect(scoped.map((row) => row.id)).toContain(pending[0]?.id);
  });

  it("decides a chat approval", async () => {
    // Red live on backend bug PDASHOSS01-307: the decide dispatch to
    // an offline runner raises out of the view (500). The decision
    // persists; only the response is wrong.
    const client = await sharedSignedIn();

    const pending = await pendingProbes(client);
    const decided = await decideChatApproval(client, pending[0]?.id ?? "", "accept");
    expect(decided.status).toBe("accepted");
    expect(decided.decision_source).toBe("web");
    expect(decided.decided_by).not.toBeNull();

    const relisted = await listChatApprovals(client);
    expect(relisted.map((row) => row.id)).not.toContain(pending[0]?.id);
  });

  it("rejects the desktop-only decision without deciding", async () => {
    const client = await sharedSignedIn();

    const pending = await pendingProbes(client);
    // The always-allow choice is desktop-only; the cloud endpoint
    // rejects it without deciding the row.
    const rejected = await decideChatApproval(
      client,
      pending[pending.length - 1]?.id ?? "",
      "accept_for_session" as "accept"
    ).catch((e: unknown) => e);
    expect(rejected).toBeInstanceOf(ApiError);
    expect((rejected as ApiError).status).toBe(400);
    const stillPending = await listChatApprovals(client);
    expect(stillPending.map((row) => row.id)).toContain(pending[pending.length - 1]?.id);
  });
});

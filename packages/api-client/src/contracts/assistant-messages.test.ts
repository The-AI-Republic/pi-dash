// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Thread/message/cancel/title contracts against a stub client (no backend).
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  AssistantMessage,
  cancelAssistantTurn,
  deleteAssistantThread,
  generateAssistantTitle,
  listAssistantMessages,
  sendAssistantMessage,
  updateAssistantThread,
} from "./assistant-messages.js";

const THREAD = "123e4567-e89b-12d3-a456-426614174000";
const MESSAGE = "223e4567-e89b-12d3-a456-426614174001";
const TURN = "323e4567-e89b-12d3-a456-426614174002";
const AT = "2026-10-16T15:32:05.479661+09:00";

function stubClient(impl: {
  getJson?: ApiClient["getJson"];
  sendJson?: ApiClient["sendJson"];
  post?: ApiClient["post"];
  delete?: ApiClient["delete"];
}): ApiClient {
  return {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    ...impl,
  } as ApiClient;
}

const NO_CONTENT = {
  status: 204,
  url: "https://api.test/x/",
  headers: { get: () => null },
  text: async () => "",
};

const THREAD_ROW = {
  id: THREAD,
  title: "Renamed",
  is_archived: false,
  has_active_turn: false,
  created_at: AT,
  updated_at: AT,
};

const MESSAGE_ROW = {
  id: MESSAGE,
  role: "user",
  content: "hello",
  status: "completed",
  seq: 1,
  turn_id: TURN,
  payload: {},
  created_at: AT,
  completed_at: AT,
};

describe("thread writes", () => {
  it("patches the title and archived flag through the detail route", async () => {
    const calls: { url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { json?: unknown }) => {
        calls.push({ url, json: options.json });
        return { data: THREAD_ROW, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    const thread = await updateAssistantThread(client, "acme", THREAD, { title: "Renamed", is_archived: false });
    expect(calls[0]?.url).toBe(`/api/workspaces/acme/ai-assistant/threads/${THREAD}/`);
    expect(calls[0]?.json).toEqual({ title: "Renamed", is_archived: false });
    expect(thread.title).toBe("Renamed");
  });

  it("deletes through the detail route", async () => {
    const calls: string[] = [];
    const client = stubClient({
      delete: (async (url: string) => {
        calls.push(url);
        return NO_CONTENT;
      }) as ApiClient["delete"],
    });
    await expect(deleteAssistantThread(client, "acme", THREAD)).resolves.toBeUndefined();
    expect(calls).toEqual([`/api/workspaces/acme/ai-assistant/threads/${THREAD}/`]);
  });
});

describe("messages", () => {
  it("pages the transcript with the after cursor and limit", async () => {
    const calls: { url?: string; query?: unknown }[] = [];
    const client = stubClient({
      getJson: (async (url: string, options: { query?: unknown }) => {
        calls.push({ url, query: options.query });
        return [MESSAGE_ROW];
      }) as ApiClient["getJson"],
    });
    const rows = await listAssistantMessages(client, "acme", THREAD, { after: 4, limit: 25 });
    expect(calls[0]?.url).toBe(`/api/workspaces/acme/ai-assistant/threads/${THREAD}/messages/`);
    expect(calls[0]?.query).toEqual({ after: 4, limit: 25 });
    expect(rows).toHaveLength(1);
    expect(rows[0]?.content).toBe("hello");
  });

  it("omits paging params by default", async () => {
    const calls: { query?: unknown }[] = [];
    const client = stubClient({
      getJson: (async (_url: string, options: { query?: unknown }) => {
        calls.push({ query: options.query });
        return [];
      }) as ApiClient["getJson"],
    });
    await expect(listAssistantMessages(client, "acme", THREAD)).resolves.toEqual([]);
    expect(calls[0]?.query).toEqual({ after: undefined, limit: undefined });
  });

  it("sends content and parses the queued turn plus user row", async () => {
    const calls: { url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { json?: unknown }) => {
        calls.push({ url, json: options.json });
        return { data: { turn: { id: TURN, status: "queued" }, message: MESSAGE_ROW }, status: 202, url };
      }) as ApiClient["sendJson"],
    });
    const answer = await sendAssistantMessage(client, "acme", THREAD, "hello");
    expect(calls[0]?.url).toBe(`/api/workspaces/acme/ai-assistant/threads/${THREAD}/messages/`);
    expect(calls[0]?.json).toEqual({ content: "hello" });
    expect(answer.turn.status).toBe("queued");
    expect(answer.message.id).toBe(MESSAGE);
  });

  it("parses tool rows with links and strips unknown payload keys", () => {
    const row = AssistantMessage.parse({
      ...MESSAGE_ROW,
      role: "tool_result",
      payload: {
        links: [
          {
            type: "issue",
            workspace_slug: "acme",
            project_id: "pid",
            issue_id: "iid",
            url_path: "/acme/projects/pid/issues/iid",
          },
        ],
        future: "stripped",
      } satisfies Record<string, unknown>,
    });
    expect(row.payload.links).toHaveLength(1);
    expect(row.payload).not.toHaveProperty("future");
  });

  it("parses a minimal link with only its address and label", () => {
    const row = AssistantMessage.parse({
      ...MESSAGE_ROW,
      role: "tool_result",
      payload: { links: [{ url_path: "/acme/assistant/t1", label: "Open thread" }] },
    });
    expect(row.payload.links).toEqual([{ url_path: "/acme/assistant/t1", label: "Open thread" }]);
  });
});

describe("cancel and title", () => {
  it("stops the turn through the cancel route", async () => {
    const calls: string[] = [];
    const client = stubClient({
      post: (async (url: string) => {
        calls.push(url);
        return NO_CONTENT;
      }) as ApiClient["post"],
    });
    await expect(cancelAssistantTurn(client, "acme", THREAD)).resolves.toBeUndefined();
    expect(calls).toEqual([`/api/workspaces/acme/ai-assistant/threads/${THREAD}/cancel/`]);
  });

  it("asks the title helper with the description", async () => {
    const calls: { url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { json?: unknown }) => {
        calls.push({ url, json: options.json });
        return { data: { title: "Drafted title" }, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    const answer = await generateAssistantTitle(client, "acme", "a long description");
    expect(calls[0]?.url).toBe("/api/workspaces/acme/ai-assistant/generate-title/");
    expect(calls[0]?.json).toEqual({ description: "a long description" });
    expect(answer.title).toBe("Drafted title");
  });
});

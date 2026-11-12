// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: assistant threads, writes, transcript, sends, cancel,
// title helper and the event stream against live Django.
import { describe, expect, it } from "vitest";
import { createClient } from "../client.js";
import { createAssistantThread, getLlmConfig, listAssistantThreads } from "../contracts/assistant.js";
import { assistantErrorFrom } from "../contracts/assistant-errors.js";
import { deleteLlmConfig, putLlmConfig } from "../contracts/assistant-config.js";
import { assistantEventsUrl } from "../contracts/assistant-events.js";
import { signIn } from "../contracts/auth-entry.js";
import {
  cancelAssistantTurn,
  deleteAssistantThread,
  generateAssistantTitle,
  listAssistantMessages,
  sendAssistantMessage,
  updateAssistantThread,
} from "../contracts/assistant-messages.js";
import { isApiError } from "../errors.js";
import {
  CONTRACT_BASE_URL,
  CONTRACT_EMAIL,
  CONTRACT_PASSWORD,
  CONTRACT_WORKSPACE,
  contractEnabled,
  contractTransport,
  sharedSignedIn,
} from "./setup.js";

describe.skipIf(!contractEnabled)("assistant contract", () => {
  it("creates a thread and lists it back newest-first", async () => {
    const client = await sharedSignedIn();
    const title = `Contract thread ${Date.now()}`;
    const created = await createAssistantThread(client, CONTRACT_WORKSPACE, title);
    expect(created.title).toBe(title);
    expect(created.has_active_turn).toBe(false);
    const threads = await listAssistantThreads(client, CONTRACT_WORKSPACE);
    expect(threads.map((thread) => thread.id)).toContain(created.id);
  });

  it("reads the model config, blank until configured", async () => {
    const client = await sharedSignedIn();
    const config = await getLlmConfig(client);
    expect(typeof config.provider_kind).toBe("string");
    expect(typeof config.model_name).toBe("string");
    expect(typeof config.has_api_key).toBe("boolean");
  });

  it("renames, archives and deletes a thread; thread GET stays 405", async () => {
    const client = await sharedSignedIn();
    const created = await createAssistantThread(client, CONTRACT_WORKSPACE, `Contract writes ${Date.now()}`);
    const renamed = await updateAssistantThread(client, CONTRACT_WORKSPACE, created.id, {
      title: `${created.title} v2`,
      is_archived: true,
    });
    expect(renamed.title).toBe(`${created.title} v2`);
    expect(renamed.is_archived).toBe(true);
    // No thread GET exists server-side: the detail route only answers
    // PATCH and DELETE, and the view reads the thread back via the list.
    const detailPath = `/api/workspaces/${CONTRACT_WORKSPACE}/ai-assistant/threads/${created.id}/`;
    const refused: unknown = await client.getJson<unknown>(detailPath).then(
      () => null,
      (error: unknown) => error
    );
    expect(isApiError(refused) && refused.status).toBe(405);
    await deleteAssistantThread(client, CONTRACT_WORKSPACE, created.id);
    const gone: unknown = await deleteAssistantThread(client, CONTRACT_WORKSPACE, created.id).then(
      () => null,
      (error: unknown) => error
    );
    expect(assistantErrorFrom(gone)).toEqual({ code: "not_found", detail: "", status: 404 });
  });

  it("reads the transcript cursor page, empty on a fresh thread", async () => {
    const client = await sharedSignedIn();
    const created = await createAssistantThread(client, CONTRACT_WORKSPACE, `Contract page ${Date.now()}`);
    try {
      const rows = await listAssistantMessages(client, CONTRACT_WORKSPACE, created.id, { after: 0, limit: 10 });
      expect(rows).toEqual([]);
    } finally {
      await deleteAssistantThread(client, CONTRACT_WORKSPACE, created.id);
    }
  });

  it("refuses empty, oversize and keyless sends with machine codes", async () => {
    const client = await sharedSignedIn();
    const created = await createAssistantThread(client, CONTRACT_WORKSPACE, `Contract gates ${Date.now()}`);
    try {
      const empty: unknown = await sendAssistantMessage(client, CONTRACT_WORKSPACE, created.id, "   ").then(
        () => null,
        (error: unknown) => error
      );
      expect(assistantErrorFrom(empty)).toEqual({ code: "empty_message", detail: "", status: 400 });
      const long: unknown = await sendAssistantMessage(client, CONTRACT_WORKSPACE, created.id, "x".repeat(32_001)).then(
        () => null,
        (error: unknown) => error
      );
      expect(assistantErrorFrom(long)).toEqual({ code: "message_too_long", detail: "", status: 400 });
      const keyless: unknown = await sendAssistantMessage(client, CONTRACT_WORKSPACE, created.id, "hello").then(
        () => null,
        (error: unknown) => error
      );
      expect(assistantErrorFrom(keyless)).toMatchObject({ code: "llm_config_missing", status: 422 });
    } finally {
      await deleteAssistantThread(client, CONTRACT_WORKSPACE, created.id);
    }
  });

  it("queues a turn against a stubbed key, refuses the second send, then cancels", async () => {
    const client = await sharedSignedIn();
    await putLlmConfig(client, {
      provider_kind: "openai_compatible",
      base_url: "http://127.0.0.1:9",
      model_name: "contract-stub",
      api_key: "contract-dummy-key",
    });
    try {
      const created = await createAssistantThread(client, CONTRACT_WORKSPACE, "");
      try {
        const answer = await sendAssistantMessage(client, CONTRACT_WORKSPACE, created.id, "hello stub");
        expect(answer.turn.status).toBe("queued");
        expect(answer.message.role).toBe("user");
        expect(answer.message.content).toBe("hello stub");
        const busy: unknown = await sendAssistantMessage(client, CONTRACT_WORKSPACE, created.id, "again").then(
          () => null,
          (error: unknown) => error
        );
        expect(assistantErrorFrom(busy)).toEqual({ code: "turn_active", detail: "", status: 409 });
        const threads = await listAssistantThreads(client, CONTRACT_WORKSPACE);
        const mine = threads.find((thread) => thread.id === created.id);
        expect(mine?.has_active_turn).toBe(true);
        expect(mine?.title).toBe("hello stub");
        await cancelAssistantTurn(client, CONTRACT_WORKSPACE, created.id);
      } finally {
        await deleteAssistantThread(client, CONTRACT_WORKSPACE, created.id);
      }
    } finally {
      await deleteLlmConfig(client);
    }
  });

  it("refuses to cancel an idle thread", async () => {
    const client = await sharedSignedIn();
    const created = await createAssistantThread(client, CONTRACT_WORKSPACE, `Contract idle ${Date.now()}`);
    try {
      const idle: unknown = await cancelAssistantTurn(client, CONTRACT_WORKSPACE, created.id).then(
        () => null,
        (error: unknown) => error
      );
      expect(assistantErrorFrom(idle)).toEqual({ code: "no_active_turn", detail: "", status: 409 });
    } finally {
      await deleteAssistantThread(client, CONTRACT_WORKSPACE, created.id);
    }
  });

  it("gates the title helper on a description and a key", async () => {
    const client = await sharedSignedIn();
    const blank: unknown = await generateAssistantTitle(client, CONTRACT_WORKSPACE, "  ").then(
      () => null,
      (error: unknown) => error
    );
    expect(assistantErrorFrom(blank)).toMatchObject({ code: "description_required", status: 400 });
    const keyless: unknown = await generateAssistantTitle(client, CONTRACT_WORKSPACE, "a draft description").then(
      () => null,
      (error: unknown) => error
    );
    expect(assistantErrorFrom(keyless)).toMatchObject({ code: "llm_config_missing", status: 422 });
  });

  it("answers the event stream and refuses unknown threads and strangers", async (ctx) => {
    const client = await sharedSignedIn();
    const created = await createAssistantThread(client, CONTRACT_WORKSPACE, `Contract stream ${Date.now()}`);
    try {
      // The refusals answer before the stream opens, so they prove
      // the route and the gate on any server.
      const jar = new Map<string, string>();
      const transport = contractTransport(jar);
      const streamClient = createClient({ baseUrl: CONTRACT_BASE_URL, transport, validate: true });
      const signedIn = await signIn(streamClient, { email: CONTRACT_EMAIL, password: CONTRACT_PASSWORD });
      expect(signedIn.ok).toBe(true);
      const missing = await transport(
        assistantEventsUrl(streamClient, CONTRACT_WORKSPACE, "00000000-0000-0000-0000-000000000000"),
        { method: "GET", headers: {}, credentials: "include" }
      );
      expect(missing.status).toBe(404);
      await missing.text();
      const stranger = await contractTransport(new Map<string, string>())(
        assistantEventsUrl(streamClient, CONTRACT_WORKSPACE, created.id),
        { method: "GET", headers: {}, credentials: "include" }
      );
      expect(stranger.status).toBe(401);
      await stranger.text();
      // The open stream needs an ASGI server: the dev runserver
      // consumes the async iterator synchronously and never answers.
      // Abort quickly and skip there instead of hanging the suite —
      // the client waits for a full response, which a stream never
      // sends, so headers are the assertion either way.
      const controller = new AbortController();
      const guard = setTimeout(() => controller.abort(), 5000);
      let response;
      try {
        response = await transport(assistantEventsUrl(streamClient, CONTRACT_WORKSPACE, created.id), {
          method: "GET",
          headers: {},
          signal: controller.signal,
          credentials: "include",
        });
      } catch (error) {
        if (controller.signal.aborted) {
          ctx.skip();
          return;
        }
        throw error;
      } finally {
        clearTimeout(guard);
      }
      expect(response.status).toBe(200);
      expect(response.headers.get("content-type")).toContain("text/event-stream");
      controller.abort();
      await response.text().catch(() => "");
    } finally {
      await deleteAssistantThread(client, CONTRACT_WORKSPACE, created.id);
    }
  });
});

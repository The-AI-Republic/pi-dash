// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-surface model-selection contracts against a stub client (no
// backend): the selection read and write, the surface delete, the
// per-ID smoke-test results, and the 400 results reader.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import {
  deleteModelSelection,
  getModelSelection,
  modelTestResultsFrom,
  putModelSelection,
  testAssistantModel,
} from "./assistant-model-selection.js";

function stubClient(impl: {
  getJson?: ApiClient["getJson"];
  sendJson?: ApiClient["sendJson"];
  delete?: ApiClient["delete"];
}): ApiClient {
  return {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    ...impl,
  } as ApiClient;
}

const FULL_SELECTION = {
  web_assistant: {
    saved: { model: "modelC", connection_id: "a93e4567-e89b-12d3-a456-426614174001" },
    effective: { model: "openhub_free_llm", connection_id: null },
    fallback_reason: "model_removed",
  },
  desktop_assistant: { saved: null, effective: { model: "openhub_free_llm" }, fallback_reason: "none_saved" },
  agent: { saved: null, effective: null, fallback_reason: "lane_unavailable" },
};

describe("getModelSelection", () => {
  it("parses the saved, effective and fallback state of every surface", async () => {
    const calls: string[] = [];
    const client = stubClient({
      getJson: (async (url: string) => {
        calls.push(url);
        return FULL_SELECTION;
      }) as ApiClient["getJson"],
    });
    const selection = await getModelSelection(client);
    expect(calls).toEqual(["/api/users/me/ai-assistant/model-selection/"]);
    expect(selection.web_assistant.saved).toEqual({
      model: "modelC",
      connection_id: "a93e4567-e89b-12d3-a456-426614174001",
    });
    expect(selection.web_assistant.fallback_reason).toBe("model_removed");
    expect(selection.agent.effective).toBeNull();
  });

  it("tolerates a missing fallback reason and connection id", async () => {
    const client = stubClient({
      getJson: (async () => ({
        web_assistant: { saved: { model: "openhub_free_llm" }, effective: { model: "openhub_free_llm" } },
        desktop_assistant: { saved: null, effective: null },
        agent: { saved: null, effective: null, fallback_reason: null },
      })) as ApiClient["getJson"],
    });
    const selection = await getModelSelection(client);
    expect(selection.web_assistant.fallback_reason).toBeUndefined();
    expect(selection.web_assistant.saved).toEqual({ model: "openhub_free_llm" });
  });
});

describe("putModelSelection", () => {
  it("writes only its own surface and parses the answered state", async () => {
    const calls: { method?: string; url?: string; json?: unknown }[] = [];
    const answered = FULL_SELECTION.desktop_assistant;
    const client = stubClient({
      sendJson: (async (method: string, url: string, options: { json?: unknown }) => {
        calls.push({ method, url, json: options.json });
        return { data: answered, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    const state = await putModelSelection(client, "desktop_assistant", {
      model: "openhub_free_llm",
    });
    expect(calls).toEqual([
      {
        method: "PUT",
        url: "/api/users/me/ai-assistant/model-selection/desktop_assistant/",
        json: { model: "openhub_free_llm", connection_id: null },
      },
    ]);
    expect(state).toEqual(answered);
  });

  it("sends the BYOK connection with a BYOK choice", async () => {
    const calls: { json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { json?: unknown }) => {
        calls.push({ json: options.json });
        return {
          data: FULL_SELECTION.web_assistant,
          status: 200,
          url,
        };
      }) as ApiClient["sendJson"],
    });
    await putModelSelection(client, "web_assistant", {
      model: "gpt-6",
      connectionId: "a93e4567-e89b-12d3-a456-426614174001",
    });
    expect(calls[0]?.json).toEqual({
      model: "gpt-6",
      connection_id: "a93e4567-e89b-12d3-a456-426614174001",
    });
  });
});

describe("deleteModelSelection", () => {
  it("clears only its own surface", async () => {
    const calls: string[] = [];
    const client = stubClient({
      delete: (async (url: string) => {
        calls.push(url);
        return { status: 204, url, headers: { get: () => null }, text: async () => "" };
      }) as ApiClient["delete"],
    });
    await expect(deleteModelSelection(client, "agent")).resolves.toBeUndefined();
    expect(calls).toEqual(["/api/users/me/ai-assistant/model-selection/agent/"]);
  });
});

describe("testAssistantModel", () => {
  it("posts the model and parses the one result object", async () => {
    const calls: { url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { json?: unknown }) => {
        calls.push({ url, json: options.json });
        return { data: { model: "gpt-6", status: "verified" }, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    const result = await testAssistantModel(client, "/api/users/me/ai-assistant/config/test/", "gpt-6");
    expect(calls).toEqual([{ url: "/api/users/me/ai-assistant/config/test/", json: { model: "gpt-6" } }]);
    expect(result).toEqual({ model: "gpt-6", status: "verified" });
  });
});

describe("modelTestResultsFrom", () => {
  const results = {
    models: [
      { model: "gpt-6", status: "verified" },
      { model: "gpt-6.3", status: "failed", code: "model_invalid", detail: "Model not accepted." },
      { model: "gpt-6.2", status: "unverified", code: "provider_unreachable" },
    ],
  };

  it("reads the merged refusal envelope", () => {
    const found = modelTestResultsFrom(
      new ApiError({
        status: 400,
        code: "http",
        message: "rejected",
        body: { error: "model_test_failed", detail: "One model failed.", ...results },
      })
    );
    expect(found).toEqual(results);
  });

  it("reads the bare per-ID envelope", () => {
    const found = modelTestResultsFrom(new ApiError({ status: 400, code: "http", message: "rejected", body: results }));
    expect(found).toEqual(results);
  });

  it("returns null for other failures", () => {
    expect(modelTestResultsFrom(new Error("plain"))).toBeNull();
    expect(modelTestResultsFrom(new ApiError({ status: 500, code: "http", message: "boom", body: null }))).toBeNull();
    expect(
      modelTestResultsFrom(
        new ApiError({
          status: 400,
          code: "http",
          message: "rejected",
          body: { error: "models_required", detail: "Empty." },
        })
      )
    ).toBeNull();
    expect(
      modelTestResultsFrom(
        new ApiError({ status: 400, code: "http", message: "rejected", body: { error: "x", models: [1] } })
      )
    ).toBeNull();
  });
});

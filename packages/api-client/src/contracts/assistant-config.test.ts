// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider-key lifecycle contracts against a stub client (no backend).
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { deleteLlmConfig, putLlmConfig, testLlmConfig } from "./assistant-config.js";

const CONFIG = {
  provider_kind: "openai_compatible",
  base_url: "https://llm.example.com",
  model_name: "scribe",
  has_api_key: true,
  last_verified_at: null,
};

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

describe("provider-key lifecycle", () => {
  it("saves the config and parses the echo", async () => {
    const calls: { url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { json?: unknown }) => {
        calls.push({ url, json: options.json });
        return { data: CONFIG, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    const saved = await putLlmConfig(client, {
      provider_kind: "openai_compatible",
      base_url: "https://llm.example.com/",
      model_name: "scribe",
      api_key: "secret-key",
    });
    expect(calls[0]?.url).toBe("/api/users/me/ai-assistant/config/");
    expect(calls[0]?.json).toEqual({
      provider_kind: "openai_compatible",
      base_url: "https://llm.example.com/",
      model_name: "scribe",
      api_key: "secret-key",
    });
    expect(saved.has_api_key).toBe(true);
  });

  it("removes the config", async () => {
    const calls: string[] = [];
    const client = stubClient({
      delete: (async (url: string) => {
        calls.push(url);
        return NO_CONTENT;
      }) as ApiClient["delete"],
    });
    await expect(deleteLlmConfig(client)).resolves.toBeUndefined();
    expect(calls).toEqual(["/api/users/me/ai-assistant/config/"]);
  });

  it("parses the connectivity verdict, flag and failure alike", async () => {
    const bodies = [{ ok: true }, { ok: false, error_code: "provider_auth_failed", detail: "rejected" }];
    const calls: string[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string) => {
        calls.push(url);
        return { data: bodies[calls.length - 1], status: 200, url };
      }) as ApiClient["sendJson"],
    });
    await expect(testLlmConfig(client)).resolves.toEqual({ ok: true });
    await expect(testLlmConfig(client)).resolves.toEqual({
      ok: false,
      error_code: "provider_auth_failed",
      detail: "rejected",
    });
    expect(calls).toEqual(["/api/users/me/ai-assistant/config/test/", "/api/users/me/ai-assistant/config/test/"]);
  });
});

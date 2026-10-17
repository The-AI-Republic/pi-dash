// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Speech-key lifecycle, transcription and the kill-switch reader
// against a stub client (no backend).
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import {
  SttConfig,
  deleteSttConfig,
  dictationDisabledFrom,
  getSttConfig,
  putSttConfig,
  testSttConfig,
  transcribeAudio,
} from "./assistant-voice.js";

const CONFIG = {
  base_url: "https://stt.example.com",
  model_name: "whisper",
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

describe("speech-key lifecycle", () => {
  it("reads the config with and without the kill-switch flag", async () => {
    const bodies = [CONFIG, { ...CONFIG, enabled: false }];
    const calls: string[] = [];
    const client = stubClient({
      getJson: (async (url: string) => {
        calls.push(url);
        return bodies[calls.length - 1];
      }) as ApiClient["getJson"],
    });
    await expect(getSttConfig(client)).resolves.toEqual(CONFIG);
    const flagged = await getSttConfig(client);
    expect(flagged.enabled).toBe(false);
    expect(calls).toEqual(["/api/users/me/ai-assistant/stt-config/", "/api/users/me/ai-assistant/stt-config/"]);
  });

  it("saves and removes the config", async () => {
    const puts: { url?: string; json?: unknown }[] = [];
    const deletes: string[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { json?: unknown }) => {
        puts.push({ url, json: options.json });
        return { data: CONFIG, status: 200, url };
      }) as ApiClient["sendJson"],
      delete: (async (url: string) => {
        deletes.push(url);
        return NO_CONTENT;
      }) as ApiClient["delete"],
    });
    const saved = await putSttConfig(client, {
      base_url: "https://stt.example.com",
      model_name: "whisper",
      api_key: "secret-key",
    });
    expect(puts[0]?.url).toBe("/api/users/me/ai-assistant/stt-config/");
    expect(puts[0]?.json).toEqual({
      base_url: "https://stt.example.com",
      model_name: "whisper",
      api_key: "secret-key",
    });
    expect(saved.has_api_key).toBe(true);
    await expect(deleteSttConfig(client)).resolves.toBeUndefined();
    expect(deletes).toEqual(["/api/users/me/ai-assistant/stt-config/"]);
  });

  it("parses the probe verdict", async () => {
    const client = stubClient({
      sendJson: (async (_method: string, url: string) => {
        return { data: { ok: false, error_code: "stt_config_missing" }, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    await expect(testSttConfig(client)).resolves.toEqual({ ok: false, error_code: "stt_config_missing" });
  });

  it("accepts the blank pre-switch read", () => {
    expect(() =>
      SttConfig.parse({ base_url: "", model_name: "", has_api_key: false, last_verified_at: null })
    ).not.toThrow();
  });
});

describe("transcribeAudio", () => {
  it("uploads the audio as multipart under the file field", async () => {
    const calls: { url?: string; multipart?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { multipart?: unknown }) => {
        calls.push({ url, multipart: options.multipart });
        return { data: { text: "hello world" }, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    const answer = await transcribeAudio(client, new Blob(["audio"]), {
      filename: "note.webm",
      language: "en",
      response_format: "json",
    });
    expect(calls[0]?.url).toBe("/api/users/me/ai-assistant/transcribe/");
    const form = calls[0]?.multipart as FormData;
    expect(form).toBeInstanceOf(FormData);
    expect((form.get("file") as File).name).toBe("note.webm");
    expect(form.get("language")).toBe("en");
    expect(form.get("response_format")).toBe("json");
    expect(answer.text).toBe("hello world");
  });

  it("defaults the filename and omits unset passthroughs", async () => {
    const calls: { multipart?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, _url: string, options: { multipart?: unknown }) => {
        calls.push({ multipart: options.multipart });
        return { data: { text: "" }, status: 200, url: _url };
      }) as ApiClient["sendJson"],
    });
    await transcribeAudio(client, new Blob(["audio"]));
    const form = calls[0]?.multipart as FormData;
    expect((form.get("file") as File).name).toBe("dictation.webm");
    expect(form.has("language")).toBe(false);
    expect(form.has("response_format")).toBe(false);
  });
});

describe("dictationDisabledFrom", () => {
  const refusal = (status: number, body: unknown) => new ApiError({ status, code: "http", message: "x", body });

  it("reads the kill-switch 404", () => {
    expect(dictationDisabledFrom(refusal(404, { error: "dictation_disabled", detail: "off" }))).toEqual({
      error: "dictation_disabled",
      detail: "off",
    });
    expect(dictationDisabledFrom(refusal(404, { error: "dictation_disabled" }))).toEqual({
      error: "dictation_disabled",
    });
  });

  it("returns null for anything else", () => {
    expect(dictationDisabledFrom(refusal(404, { error: "not_found" }))).toBeNull();
    expect(dictationDisabledFrom(refusal(404, null))).toBeNull();
    expect(dictationDisabledFrom(refusal(422, { error: "dictation_disabled" }))).toBeNull();
    expect(dictationDisabledFrom(new Error("plain"))).toBeNull();
  });
});

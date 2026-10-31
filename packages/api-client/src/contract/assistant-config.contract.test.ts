// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: model-key and speech-key lifecycles, their connectivity
// probes and transcription against live Django. Success paths run
// against a stub provider on loopback (the SSRF guard stays off in
// the contract stack); every test restores the blank config it found.
import http from "node:http";
import type { AddressInfo } from "node:net";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { ApiClient } from "../client.js";
import { assistantErrorFrom } from "../contracts/assistant-errors.js";
import { deleteLlmConfig, putLlmConfig, testLlmConfig } from "../contracts/assistant-config.js";
import { generateAssistantTitle } from "../contracts/assistant-messages.js";
import {
  deleteSttConfig,
  getSttConfig,
  putSttConfig,
  testSttConfig,
  transcribeAudio,
} from "../contracts/assistant-voice.js";
import { getLlmConfig } from "../contracts/assistant.js";
import { isApiError } from "../errors.js";
import { CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

/** Last transcription upload the stub saw, for framing assertions. */
let lastTranscribe: { contentType: string; hadFileField: boolean } | null = null;

let stubBaseUrl = "";
let stubServer: http.Server | null = null;

function readBody(request: http.IncomingMessage): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    request.on("data", (chunk: Buffer) => chunks.push(chunk));
    request.on("end", () => resolve(Buffer.concat(chunks)));
    request.on("error", reject);
  });
}

beforeAll(async () => {
  if (!contractEnabled) return;
  stubServer = http.createServer(async (request, response) => {
    if (request.method === "POST" && request.url === "/audio/transcriptions") {
      const body = await readBody(request);
      lastTranscribe = {
        contentType: request.headers["content-type"] ?? "",
        hadFileField: body.includes('name="file"'),
      };
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ text: "hello world" }));
      return;
    }
    if (request.method === "POST" && request.url === "/chat/completions") {
      await readBody(request);
      response.writeHead(200, { "content-type": "application/json" });
      response.end(
        JSON.stringify({
          id: "stub-1",
          object: "chat.completion",
          created: 1_700_000_000,
          model: "stub-model",
          choices: [
            {
              index: 0,
              message: { role: "assistant", content: "Stubbed title" },
              finish_reason: "stop",
            },
          ],
          usage: { prompt_tokens: 4, completion_tokens: 2, total_tokens: 6 },
        })
      );
      return;
    }
    response.writeHead(404, { "content-type": "application/json" });
    response.end(JSON.stringify({ error: "stub has no such route" }));
  });
  await new Promise<void>((resolve) => stubServer?.listen(0, "127.0.0.1", resolve));
  const address = stubServer?.address() as AddressInfo | null;
  if (!address || typeof address === "string") throw new Error("stub provider did not bind");
  stubBaseUrl = `http://127.0.0.1:${address.port}`;
});

afterAll(async () => {
  await new Promise<void>((resolve, reject) =>
    stubServer ? stubServer.close((error) => (error ? reject(error) : resolve())) : resolve()
  );
  stubServer = null;
});

async function blankLlm(client: ApiClient): Promise<void> {
  await deleteLlmConfig(client);
  const config = await getLlmConfig(client);
  expect(config.model_name).toBe("");
  expect(config.has_api_key).toBe(false);
}

async function blankStt(client: ApiClient): Promise<void> {
  await deleteSttConfig(client);
  const config = await getSttConfig(client);
  expect(config.model_name).toBe("");
  expect(config.has_api_key).toBe(false);
}

describe.skipIf(!contractEnabled)("assistant config contract", () => {
  it("proves the model-key lifecycle against a stub provider", async () => {
    const client = await sharedSignedIn();
    await blankLlm(client);
    try {
      const missing = await testLlmConfig(client);
      expect(missing).toEqual({ ok: false, error_code: "llm_config_missing" });
      const saved = await putLlmConfig(client, {
        provider_kind: "openai_compatible",
        base_url: `${stubBaseUrl}/`,
        model_name: "stub-model",
        api_key: "contract-dummy-key",
      });
      expect(saved.base_url).toBe(stubBaseUrl);
      expect(saved.model_name).toBe("stub-model");
      expect(saved.has_api_key).toBe(true);
      expect(saved.last_verified_at).toBeNull();
      const raw = await client.getJson<Record<string, unknown>>("/api/users/me/ai-assistant/config/");
      expect("api_key" in raw).toBe(false);
      const bad: unknown = await putLlmConfig(client, { base_url: "notaurl" }).then(
        () => null,
        (error: unknown) => error
      );
      expect(isApiError(bad) && bad.status).toBe(400);
      expect(isApiError(bad) && bad.fields?.["base_url"]?.length).toBeGreaterThan(0);
      const verdict = await testLlmConfig(client);
      expect(verdict).toEqual({ ok: true });
      const verified = await getLlmConfig(client);
      expect(verified.last_verified_at).not.toBeNull();
    } finally {
      await blankLlm(client);
    }
  });

  it("generates a title through the stub provider", async () => {
    const client = await sharedSignedIn();
    await blankLlm(client);
    try {
      await putLlmConfig(client, {
        provider_kind: "openai_compatible",
        base_url: stubBaseUrl,
        model_name: "stub-model",
        api_key: "contract-dummy-key",
      });
      const answer = await generateAssistantTitle(client, CONTRACT_WORKSPACE, "a stubbed draft description");
      expect(answer.title).toBe("Stubbed title");
    } finally {
      await blankLlm(client);
    }
  });

  it("proves the speech-key lifecycle and transcription against the stub", async () => {
    const client = await sharedSignedIn();
    await blankStt(client);
    try {
      const missing = await testSttConfig(client);
      expect(missing).toEqual({ ok: false, error_code: "stt_config_missing" });
      const gated: unknown = await transcribeAudio(client, new Blob(["audio"])).then(
        () => null,
        (error: unknown) => error
      );
      expect(assistantErrorFrom(gated)).toMatchObject({ code: "stt_config_missing", status: 422 });
      const saved = await putSttConfig(client, {
        base_url: stubBaseUrl,
        model_name: "stub-whisper",
        api_key: "contract-dummy-key",
      });
      expect(saved.has_api_key).toBe(true);
      const verdict = await testSttConfig(client);
      expect(verdict).toEqual({ ok: true });
      const verified = await getSttConfig(client);
      expect(verified.last_verified_at).not.toBeNull();
      lastTranscribe = null;
      const answer = await transcribeAudio(client, new Blob(["fake-audio-bytes"]), { filename: "probe.wav" });
      expect(answer).toEqual({ text: "hello world" });
      const seen = lastTranscribe as { contentType: string; hadFileField: boolean } | null;
      expect(seen?.contentType).toContain("multipart/form-data");
      expect(seen?.hadFileField).toBe(true);
      const emptyForm = new FormData();
      const noAudio: unknown = await client
        .sendJson("POST", "/api/users/me/ai-assistant/transcribe/", { multipart: emptyForm })
        .then(
          () => null,
          (error: unknown) => error
        );
      expect(assistantErrorFrom(noAudio)).toMatchObject({ code: "no_audio", status: 400 });
      // Past the 5 MB platform body cap the platform guard refuses
      // first — the view's own 25 MB audio_too_large only fires with
      // a raised FILE_SIZE_LIMIT, so the platform code is the live one.
      const oversize: unknown = await transcribeAudio(client, new Blob([new Uint8Array(6 * 1024 * 1024)])).then(
        () => null,
        (error: unknown) => error
      );
      expect(assistantErrorFrom(oversize)).toMatchObject({ code: "REQUEST_BODY_TOO_LARGE", status: 413 });
    } finally {
      await blankStt(client);
    }
  });
});

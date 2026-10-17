// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Voice dictation: the per-user speech-key lifecycle, the connectivity
// check, and the transcription upload. The speech config mirrors the
// model config minus the provider selector — dictation has the one
// OpenAI-compatible provider, so the base address is always required.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { isApiError } from "../errors.js";
import { nullableDateTime } from "./fields.js";

/**
 * The stored speech config. `enabled` travels on the read once the
 * dictation kill switch lands (main#586) and stays absent until then —
 * absent means a pre-switch backend, not a disabled one.
 */
export const SttConfig = z.object({
  base_url: z.string(),
  model_name: z.string(),
  has_api_key: z.boolean(),
  last_verified_at: nullableDateTime(),
  enabled: z.optional(z.boolean()),
});
export type SttConfig = z.infer<typeof SttConfig>;

/**
 * Read the speech config; absent configuration reads back blank
 * rather than 404, exactly like the model config read.
 */
export async function getSttConfig(client: ApiClient): Promise<SttConfig> {
  const body = await client.getJson<unknown>("/api/users/me/ai-assistant/stt-config/");
  return client.parse(SttConfig, body, "GET /api/users/me/ai-assistant/stt-config/");
}

export interface SttConfigInput {
  base_url?: string;
  model_name?: string;
  /** Write-only: omit to keep the saved key. */
  api_key?: string;
}

/** Save the speech key (create or partial update); the key never echoes. */
export async function putSttConfig(client: ApiClient, input: SttConfigInput): Promise<SttConfig> {
  const path = "/api/users/me/ai-assistant/stt-config/";
  const envelope = await client.sendJson<unknown>("PUT", path, {
    json: {
      base_url: input.base_url,
      model_name: input.model_name,
      api_key: input.api_key,
    },
  });
  return client.parse(SttConfig, envelope.data, `PUT ${path}`);
}

/** Remove the speech key; resolves on the 204. Stays open when killed. */
export async function deleteSttConfig(client: ApiClient): Promise<void> {
  await client.delete("/api/users/me/ai-assistant/stt-config/");
}

/**
 * The speech connectivity verdict. Always 200: success carries only
 * the flag (the probe answers, the key is accepted), failure names
 * a machine code.
 */
export const SttConfigTestResult = z.object({
  ok: z.boolean(),
  error_code: z.optional(z.string()),
  detail: z.optional(z.string()),
});
export type SttConfigTestResult = z.infer<typeof SttConfigTestResult>;

/** Probe the saved speech endpoint without transcribing audio. */
export async function testSttConfig(client: ApiClient): Promise<SttConfigTestResult> {
  const path = "/api/users/me/ai-assistant/stt-config/test/";
  const envelope = await client.sendJson<unknown>("POST", path);
  return client.parse(SttConfigTestResult, envelope.data, `POST ${path}`);
}

/** The transcription answer: the recognized text, nothing else. */
export const TranscribeResponse = z.object({
  text: z.string(),
});
export type TranscribeResponse = z.infer<typeof TranscribeResponse>;

export interface TranscribeOptions {
  filename?: string;
  language?: string;
  response_format?: string;
}

/**
 * Transcribe a recording through the saved speech endpoint. The audio
 * uploads as multipart under the `file` field; the server injects the
 * model, forwards to the provider, and drops the bytes — dictation is
 * never persisted. Refusals (no key, no audio, too large, provider
 * failures) reject with the assistant failure shape.
 */
export async function transcribeAudio(
  client: ApiClient,
  audio: Blob,
  options: TranscribeOptions = {}
): Promise<TranscribeResponse> {
  const path = "/api/users/me/ai-assistant/transcribe/";
  const form = new FormData();
  form.append("file", audio, options.filename ?? "dictation.webm");
  if (options.language !== undefined) form.append("language", options.language);
  if (options.response_format !== undefined) form.append("response_format", options.response_format);
  const envelope = await client.sendJson<unknown>("POST", path, { multipart: form });
  return client.parse(TranscribeResponse, envelope.data, `POST ${path}`);
}

/**
 * The kill-switch refusal: with dictation switched off instance-wide
 * the speech save, test and transcription endpoints answer 404 naming
 * this code, while the read and the remove stay open.
 */
export const DictationDisabledPayload = z.object({
  error: z.literal("dictation_disabled"),
  detail: z.optional(z.string()),
});
export type DictationDisabledPayload = z.infer<typeof DictationDisabledPayload>;

/**
 * Read the kill-switch refusal off a rejected speech call: the
 * payload when the failure is the dictation-disabled 404, else null.
 */
export function dictationDisabledFrom(error: unknown): DictationDisabledPayload | null {
  if (!isApiError(error) || error.status !== 404) return null;
  const parsed = DictationDisabledPayload.safeParse(error.body);
  return parsed.success ? parsed.data : null;
}

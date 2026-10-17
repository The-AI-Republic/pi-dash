// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider-key lifecycle: the per-user model config write and remove
// plus the connectivity check. The read lives in assistant.ts; the
// key itself is write-only — the API only ever reports its presence.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { LlmConfig } from "./assistant.js";

export interface LlmConfigInput {
  provider_kind?: string;
  base_url?: string;
  model_name?: string;
  /** Write-only: omit to keep the saved key. */
  api_key?: string;
}

/**
 * Save the provider key (create or partial update). The server trims
 * the model name, strips the base address's trailing slash, and
 * answers the stored config — the key never echoes back.
 */
export async function putLlmConfig(client: ApiClient, input: LlmConfigInput): Promise<LlmConfig> {
  const path = "/api/users/me/ai-assistant/config/";
  const envelope = await client.sendJson<unknown>("PUT", path, {
    json: {
      provider_kind: input.provider_kind,
      base_url: input.base_url,
      model_name: input.model_name,
      api_key: input.api_key,
    },
  });
  return client.parse(LlmConfig, envelope.data, `PUT ${path}`);
}

/** Remove the provider key; resolves on the 204. */
export async function deleteLlmConfig(client: ApiClient): Promise<void> {
  await client.delete("/api/users/me/ai-assistant/config/");
}

/**
 * The connectivity verdict. Always 200: success carries only the flag
 * (and stamps the verified time), failure names a machine code.
 */
export const LlmConfigTestResult = z.object({
  ok: z.boolean(),
  error_code: z.optional(z.string()),
  detail: z.optional(z.string()),
});
export type LlmConfigTestResult = z.infer<typeof LlmConfigTestResult>;

/** Probe the saved provider with a one-word completion. */
export async function testLlmConfig(client: ApiClient): Promise<LlmConfigTestResult> {
  const path = "/api/users/me/ai-assistant/config/test/";
  const envelope = await client.sendJson<unknown>("POST", path);
  return client.parse(LlmConfigTestResult, envelope.data, `POST ${path}`);
}

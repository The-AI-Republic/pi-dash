// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant failure reader: the synchronous {error, detail} shape the
// assistant endpoints refuse with, and the helper that lifts it off a
// rejected call so surfaces can branch on the machine code.
import * as z from "zod/mini";
import { isApiError } from "../errors.js";

/**
 * The assistant's synchronous refusal shape: a stable machine code
 * (`empty_message`, `llm_config_missing`, `turn_active`, ...) plus a
 * human sentence. Thrown as the ApiError body, never the code.
 */
export const AssistantApiError = z.object({
  error: z.string(),
  detail: z.optional(z.string()),
});
export type AssistantApiError = z.infer<typeof AssistantApiError>;

export interface AssistantFailure {
  code: string;
  detail: string;
  status: number;
}

/**
 * Read the assistant failure off a rejected call: the machine code, the
 * human detail (empty when the server sent none) and the HTTP status.
 * Null when the failure is not an assistant refusal (transport errors,
 * contract violations, or an error body of another shape).
 */
export function assistantErrorFrom(error: unknown): AssistantFailure | null {
  if (!isApiError(error)) return null;
  const parsed = AssistantApiError.safeParse(error.body);
  if (!parsed.success) return null;
  return { code: parsed.data.error, detail: parsed.data.detail ?? "", status: error.status };
}

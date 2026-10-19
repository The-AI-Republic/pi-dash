// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Setup gating (parity/specs/agents.md §9, AGT-039/AGT-040). A model is
// usable only when it has both a name and a stored key; the blank default
// the config read answers with is "not set up", and an unresolved config
// is not a verdict either way.

import type { LlmConfig } from "@pidash/api-client";

import { resolveLaneState } from "./lane.js";

/** True when the user has a model name and a key to run it with. */
export function isAssistantConfigured(
  config: Pick<LlmConfig, "model_name" | "has_api_key"> | null | undefined
): boolean {
  if (config === null || config === undefined) return false;
  return config.model_name.trim() !== "" && config.has_api_key;
}

/**
 * Whether the landing must swap its chat entry for the setup card. Only a
 * known configuration can lock the assistant: while the read is pending
 * the caller renders neither branch.
 */
export function needsSetup(config: Pick<LlmConfig, "model_name" | "has_api_key"> | undefined): boolean {
  if (config === undefined) return false;
  return !isAssistantConfigured(config);
}

/**
 * Setup gating across credential lanes (CLOUD-037, from
 * parity/specs/cloud.md §7). The assistant entries ask this instead of the
 * self-supplied check alone, so a hosted or provider-account lane that
 * carries its credential is ready even with no self-supplied key. Older
 * backends without lane fields read as the self-supplied lane, and an
 * unresolved config never reports setup.
 */
export function needsLaneSetup(config: LlmConfig | null | undefined): boolean {
  return resolveLaneState(config).setupNeeded;
}

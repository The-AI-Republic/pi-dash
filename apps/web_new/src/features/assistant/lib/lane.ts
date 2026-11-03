// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Credential-lane state (parity/specs/cloud.md §7, CLOUD-037). Assistant
// entry points ask one question — is the active lane ready? — and the
// answer depends on which lane the config names, not on the self-supplied
// key alone. An unloaded config is never a verdict: a read in flight must
// not flash a setup card over a working assistant.

import { resolveCredentialLane, type CredentialLane, type LlmConfig } from "@pidash/api-client";

export interface LaneState {
  /** Active lane; self-supplied when the backend predates the field. */
  lane: CredentialLane;
  /** Whether the active lane carries a credential right now. */
  ready: boolean;
  /** True only when the config loaded and its lane is still unconfigured. */
  setupNeeded: boolean;
}

/** The credential flag that decides readiness for one lane. */
export function laneHasCredential(config: LlmConfig, lane: CredentialLane): boolean {
  switch (lane) {
    case "codex":
      return config.has_codex_token === true;
    case "openhub":
      return config.has_openhub_access === true;
    case "byok":
      return config.has_byok_api_key === true || config.has_api_key;
  }
}

/**
 * Lane readiness for a loaded or pending config. `undefined` means the
 * read has not answered yet, so nothing is known to need setup.
 */
export function resolveLaneState(config: LlmConfig | null | undefined): LaneState {
  if (config === null || config === undefined) {
    return { lane: "byok", ready: false, setupNeeded: false };
  }
  const lane = resolveCredentialLane(config);
  const ready = laneHasCredential(config, lane);
  return { lane, ready, setupNeeded: !ready };
}

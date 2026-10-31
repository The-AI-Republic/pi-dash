// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Credential-lane state (CLOUD-037): the lane the backend reports, whether
// that lane carries a credential, the self-supplied default on older
// backends, and the rule that an unloaded config never needs setup.
import { describe, expect, it } from "vitest";

import type { LlmConfig } from "@pidash/api-client";

import { laneHasCredential, resolveLaneState } from "./lane.js";

function config(overrides: Partial<LlmConfig> = {}): LlmConfig {
  return {
    provider_kind: "openai_compatible",
    base_url: "",
    model_name: "",
    has_api_key: false,
    last_verified_at: null,
    ...overrides,
  };
}

describe("resolveLaneState", () => {
  it("never reports setup for a config that has not loaded", () => {
    expect(resolveLaneState(undefined)).toEqual({ lane: "byok", ready: false, setupNeeded: false });
    expect(resolveLaneState(null)).toEqual({ lane: "byok", ready: false, setupNeeded: false });
  });

  it("defaults to the self-supplied lane on a backend without lane fields", () => {
    expect(resolveLaneState(config({ has_api_key: false }))).toEqual({
      lane: "byok",
      ready: false,
      setupNeeded: true,
    });
    expect(resolveLaneState(config({ has_api_key: true }))).toEqual({
      lane: "byok",
      ready: true,
      setupNeeded: false,
    });
  });

  it("reads the self-supplied lane's own flag before the legacy key flag", () => {
    expect(resolveLaneState(config({ credential_mode: "byok", has_byok_api_key: true })).ready).toBe(true);
    expect(resolveLaneState(config({ credential_mode: "byok", has_byok_api_key: false })).setupNeeded).toBe(true);
  });

  it("is ready on the provider-account lane only with its token", () => {
    expect(resolveLaneState(config({ credential_mode: "codex", has_codex_token: true })).setupNeeded).toBe(false);
    expect(resolveLaneState(config({ credential_mode: "codex", has_codex_token: false, has_api_key: true }))).toEqual({
      lane: "codex",
      ready: false,
      setupNeeded: true,
    });
  });

  it("is ready on the hosted lane only with its access flag", () => {
    expect(resolveLaneState(config({ credential_mode: "openhub", has_openhub_access: true })).setupNeeded).toBe(false);
    expect(resolveLaneState(config({ credential_mode: "openhub", has_openhub_access: false })).setupNeeded).toBe(true);
  });
});

describe("laneHasCredential", () => {
  it("requires the flag for the lane it is asked about", () => {
    const keyed = config({ has_api_key: true });
    expect(laneHasCredential(keyed, "byok")).toBe(true);
    expect(laneHasCredential(keyed, "codex")).toBe(false);
    expect(laneHasCredential(keyed, "openhub")).toBe(false);
  });
});

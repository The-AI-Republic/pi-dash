// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  AGENT_REASON_CODES,
  AGENT_REASON_MESSAGES,
  agentReasonMessage,
  agentReasonNeedsSignIn,
  agentRefusalCode,
  isKnownAgentReason,
} from "./reasons.js";

describe("desktop agent reasons", () => {
  it("explains every cloud-model situation distinctly", () => {
    expect(AGENT_REASON_CODES).toEqual(
      expect.arrayContaining([
        "managed_runner_disabled",
        "byok_not_supported_on_desktop",
        "llm_config_missing",
        "gateway_session_revoked",
        "desktop_session_required",
        "desktop_not_connected",
        "gateway_scopes_missing",
        "gateway_unavailable",
      ])
    );
    expect(AGENT_REASON_CODES).toHaveLength(8);
    expect(new Set(Object.values(AGENT_REASON_MESSAGES)).size).toBe(8);
  });

  it("names the fix in the self-supplied lane message and keeps cloud runs working", () => {
    const message = agentReasonMessage("byok_not_supported_on_desktop");
    expect(message).toMatch(/hosted model lane/i);
    expect(message).toMatch(/cloud runs/i);
  });

  it("keeps the session situations apart", () => {
    expect(agentReasonMessage("gateway_session_revoked")).not.toBe(agentReasonMessage("desktop_session_required"));
    expect(agentReasonMessage("desktop_not_connected")).not.toBe(agentReasonMessage("desktop_session_required"));
    expect(agentReasonMessage("gateway_scopes_missing")).not.toBe(agentReasonMessage("gateway_session_revoked"));
  });

  it("says the gateway outage retries on its own", () => {
    expect(agentReasonMessage("gateway_unavailable")).toMatch(/retries/i);
  });

  it("surfaces an unknown code verbatim", () => {
    expect(agentReasonMessage("some_new_code")).toBe("some_new_code");
  });

  it("knows which refusals a new sign-in fixes", () => {
    expect(agentReasonNeedsSignIn("gateway_session_revoked")).toBe(true);
    expect(agentReasonNeedsSignIn("gateway_scopes_missing")).toBe(true);
    expect(agentReasonNeedsSignIn("gateway_unavailable")).toBe(false);
    expect(agentReasonNeedsSignIn("managed_runner_disabled")).toBe(false);
  });
});

describe("agent refusal reader", () => {
  it("reads the error code off a refusal body", () => {
    expect(agentRefusalCode({ error: "byok_not_supported_on_desktop", detail: "x" })).toBe(
      "byok_not_supported_on_desktop"
    );
    expect(agentRefusalCode({ error: "gateway_session_revoked" })).toBe("gateway_session_revoked");
  });

  it("returns null for shapes without a usable code", () => {
    expect(agentRefusalCode(null)).toBeNull();
    expect(agentRefusalCode("byok")).toBeNull();
    expect(agentRefusalCode({ detail: "no code" })).toBeNull();
    expect(agentRefusalCode({ error: "" })).toBeNull();
    expect(agentRefusalCode({ error: 42 })).toBeNull();
  });

  it("knows which codes this build explains", () => {
    for (const code of AGENT_REASON_CODES) expect(isKnownAgentReason(code)).toBe(true);
    expect(isKnownAgentReason("feature_off")).toBe(false);
    expect(isKnownAgentReason("toString")).toBe(false);
  });
});

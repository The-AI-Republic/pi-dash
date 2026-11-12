// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { AGENT_REASON_MESSAGES, agentReasonMessage } from "../../../shared/lib/agentReasons.js";
import { agentRefusalCode, isKnownAgentReason } from "./reasons.js";

/** The cloud-model situations CLOUD-040 names. */
const CLOUD_CODES = [
  "managed_runner_disabled",
  "byok_not_supported_on_desktop",
  "llm_config_missing",
  "gateway_session_revoked",
  "desktop_session_required",
  "desktop_not_connected",
  "gateway_scopes_missing",
  "gateway_unavailable",
];

describe("cloud-model refusals", () => {
  it("explains every situation with its own sentence", () => {
    for (const code of CLOUD_CODES) expect(isKnownAgentReason(code), code).toBe(true);
    const sentences = CLOUD_CODES.map((code) => AGENT_REASON_MESSAGES[code]);
    expect(new Set(sentences).size).toBe(CLOUD_CODES.length);
  });

  it("names the hosted lane in the self-supplied-key sentence and keeps cloud runs working", () => {
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
    // "will retry shortly": the retry is automatic, not a viewer action.
    expect(agentReasonMessage("gateway_unavailable")).toMatch(/retr(y|ies)/i);
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
    for (const code of CLOUD_CODES) expect(isKnownAgentReason(code)).toBe(true);
    expect(isKnownAgentReason("feature_off")).toBe(false);
    expect(isKnownAgentReason("toString")).toBe(false);
  });
});

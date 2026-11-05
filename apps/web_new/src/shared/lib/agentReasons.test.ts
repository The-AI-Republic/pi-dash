// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { ApiError } from "@pidash/api-client";

import { AGENT_REASON_MESSAGES, agentFailureMessage, agentReasonMessage } from "./agentReasons.js";

const CODES = [
  "managed_runner_disabled",
  "desktop_not_connected",
  "llm_config_missing",
  "gateway_scopes_missing",
  "byok_not_supported_on_desktop",
  "no_managed_runner_for_project",
  "desktop_session_required",
  "gateway_session_revoked",
  "gateway_unavailable",
  "agent_profile_unreachable",
  "cloud_agent_unavailable",
  "no_local_runner",
];

describe("AGENT_REASON_MESSAGES", () => {
  it("covers every server and client agent reason with its own sentence", () => {
    for (const code of CODES) {
      const sentence = AGENT_REASON_MESSAGES[code];
      expect(sentence, code).toBeTruthy();
      expect(sentence, code).not.toBe(code);
      expect(sentence, code).not.toContain(code);
    }
    expect(new Set(Object.values(AGENT_REASON_MESSAGES)).size).toBe(CODES.length);
  });

  it("does not promise a raw protocol identifier as the explanation", () => {
    for (const sentence of Object.values(AGENT_REASON_MESSAGES)) {
      expect(sentence).not.toMatch(/_/);
    }
  });

  it("points the self-supplied-key refusal at the hosted lane", () => {
    expect(agentReasonMessage("byok_not_supported_on_desktop")).toMatch(/hosted model lane/i);
    expect(agentReasonMessage("byok_not_supported_on_desktop")).toMatch(/cloud runs/i);
  });
});

describe("agentReasonMessage", () => {
  it("returns the mapped sentence for a known code", () => {
    expect(agentReasonMessage("gateway_session_revoked")).toContain("revoked");
    expect(agentReasonMessage("byok_not_supported_on_desktop")).toContain("provider key");
  });

  it("surfaces an unknown code unchanged", () => {
    expect(agentReasonMessage("tomorrow_problem")).toBe("tomorrow_problem");
  });

  it("handles the empty code without inventing a refusal", () => {
    expect(agentReasonMessage("")).toBe("");
  });
});

describe("agentFailureMessage", () => {
  it("maps a server refusal body onto its sentence", () => {
    const failure = new ApiError({
      status: 409,
      code: "http",
      message: "Conflict",
      body: { error: "llm_config_missing", detail: "no model" },
    });
    expect(agentFailureMessage(failure)).toBe(AGENT_REASON_MESSAGES.llm_config_missing);
  });

  it("reads error_code and code bodies too", () => {
    const errorCode = new ApiError({
      status: 400,
      code: "http",
      message: "Bad",
      body: { error_code: "gateway_unavailable" },
    });
    expect(agentFailureMessage(errorCode)).toContain("gateway");
    const plainCode = new ApiError({
      status: 400,
      code: "http",
      message: "Bad",
      body: { code: "desktop_session_required" },
    });
    expect(agentFailureMessage(plainCode)).toContain("desktop app");
  });

  it("surfaces an unknown body code as-is", () => {
    const failure = new ApiError({ status: 400, code: "http", message: "Bad", body: { error: "new_code" } });
    expect(agentFailureMessage(failure)).toBe("new_code");
  });

  it("keeps the API message when the body names no reason", () => {
    const failure = new ApiError({ status: 502, code: "http", message: "Bad gateway" });
    expect(agentFailureMessage(failure)).toBe("Bad gateway");
    const textBody = new ApiError({ status: 502, code: "http", message: "Bad gateway", body: "nope" });
    expect(agentFailureMessage(textBody)).toBe("Bad gateway");
  });

  it("ignores non-string and empty reason values", () => {
    for (const body of [{ error: 7 }, { error: "" }, { error_code: null }, { code: {} }]) {
      const failure = new ApiError({ status: 400, code: "http", message: "Bad request", body });
      expect(agentFailureMessage(failure)).toBe("Bad request");
    }
  });

  it("keeps transport and programming errors intact", () => {
    expect(agentFailureMessage(new TypeError("network down"))).toBe("network down");
    expect(agentFailureMessage("plain")).toBe("plain");
  });
});

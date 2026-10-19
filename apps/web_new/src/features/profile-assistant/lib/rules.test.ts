// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ApiError } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  LLM_KEY_PLACEHOLDER_EMPTY,
  SAVE_FAILED_FALLBACK,
  SAVED_KEY_PLACEHOLDER,
  SERVER_PREFIX_PREFIX,
  UNKNOWN_ERROR_FALLBACK,
} from "./copy.js";
import {
  canAddServer,
  canTest,
  endpointForSave,
  endpointVisible,
  keyPlaceholder,
  keyForSave,
  lastVerifiedLine,
  llmCanSave,
  providerOf,
  saveFailureMessage,
  serverFailureMessage,
  serverPrefix,
  serverPrefixLine,
  sttCanSave,
  testFailureCode,
  voiceSection,
} from "./rules.js";

describe("provider form rules", () => {
  it("defaults a blank read to the URL-based provider", () => {
    expect(providerOf(undefined)).toBe("openai_compatible");
    expect(providerOf({ provider_kind: "" })).toBe("openai_compatible");
    expect(providerOf({ provider_kind: "anthropic" })).toBe("anthropic");
  });

  it("hides the endpoint only for the named provider", () => {
    expect(endpointVisible("openai_compatible")).toBe(true);
    expect(endpointVisible("anthropic")).toBe(false);
    expect(endpointForSave("openai_compatible", "  http://x  ")).toBe("http://x");
    expect(endpointForSave("anthropic", "http://x")).toBeUndefined();
  });

  it("gates the save on a model and a typed or stored key", () => {
    expect(llmCanSave({ model: " m ", key: " k ", hasKey: false })).toBe(true);
    expect(llmCanSave({ model: "m", key: "  ", hasKey: false })).toBe(false);
    expect(llmCanSave({ model: "", key: "k", hasKey: true })).toBe(false);
  });

  it("saves over a stored key with the field blank, keeping the secret", () => {
    expect(llmCanSave({ model: "m", key: "", hasKey: true })).toBe(true);
    expect(llmCanSave({ model: "m", key: "  ", hasKey: true })).toBe(true);
    expect(sttCanSave({ baseUrl: "u", model: "m", key: "", hasKey: true })).toBe(true);
  });

  it("gates the voice save on endpoint, model and a typed or stored key", () => {
    expect(sttCanSave({ baseUrl: "u", model: "m", key: "k", hasKey: false })).toBe(true);
    expect(sttCanSave({ baseUrl: "", model: "m", key: "k", hasKey: true })).toBe(false);
    expect(sttCanSave({ baseUrl: "u", model: " ", key: "k", hasKey: true })).toBe(false);
    expect(sttCanSave({ baseUrl: "u", model: "m", key: "", hasKey: false })).toBe(false);
  });

  it("posts the typed key only, so a blank field keeps the saved one", () => {
    expect(keyForSave("  secret  ")).toBe("secret");
    expect(keyForSave("")).toBeUndefined();
    expect(keyForSave("   ")).toBeUndefined();
  });

  it("tests only once a key is stored, and adds only with name and URL", () => {
    expect(canTest(undefined)).toBe(false);
    expect(canTest({ has_api_key: false })).toBe(false);
    expect(canTest({ has_api_key: true })).toBe(true);
    expect(canAddServer({ name: "a", url: "b" })).toBe(true);
    expect(canAddServer({ name: " ", url: "b" })).toBe(false);
    expect(canAddServer({ name: "a", url: "" })).toBe(false);
  });

  it("swaps the key placeholder once a key is stored", () => {
    expect(keyPlaceholder(false, LLM_KEY_PLACEHOLDER_EMPTY)).toBe(LLM_KEY_PLACEHOLDER_EMPTY);
    expect(keyPlaceholder(true, LLM_KEY_PLACEHOLDER_EMPTY)).toBe(SAVED_KEY_PLACEHOLDER);
  });

  it("formats the verification stamp and hides it when absent", () => {
    expect(lastVerifiedLine(null)).toBeNull();
    expect(lastVerifiedLine(undefined)).toBeNull();
    expect(lastVerifiedLine("")).toBeNull();
    expect(lastVerifiedLine("not-a-date")).toBe("Last verified: Never");
    expect(lastVerifiedLine("2026-10-19T12:00:00Z")).toMatch(/^Last verified: ./);
  });
});

describe("failure wording", () => {
  it("prefers the detail, then the error code, then the fallback", () => {
    expect(saveFailureMessage({ body: { detail: "detail line", error: "code" } })).toBe("detail line");
    expect(saveFailureMessage({ body: { error: "probe_boom" } })).toBe("probe_boom");
    expect(saveFailureMessage({ body: { detail: "  ", error: "" } })).toBe(SAVE_FAILED_FALLBACK);
    expect(saveFailureMessage({ body: null })).toBe(SAVE_FAILED_FALLBACK);
    expect(saveFailureMessage(new Error("network"))).toBe(SAVE_FAILED_FALLBACK);
    expect(saveFailureMessage(null)).toBe(SAVE_FAILED_FALLBACK);
  });

  it("keeps a caller-specific fallback when the body carries no message", () => {
    expect(serverFailureMessage({ body: null }, "custom")).toBe("custom");
    expect(serverFailureMessage({ body: { error: "duplicate_name" } }, "custom")).toBe("duplicate_name");
  });

  it("reads an ApiError body too", () => {
    const error = new ApiError({ status: 400, code: "http", message: "x", body: { detail: "blocked" } });
    expect(saveFailureMessage(error)).toBe("blocked");
  });

  it("names the test failure code, falling back when blank", () => {
    expect(testFailureCode({ error_code: "provider_unreachable" })).toBe("provider_unreachable");
    expect(testFailureCode({ error_code: " " })).toBe(UNKNOWN_ERROR_FALLBACK);
    expect(testFailureCode({})).toBe(UNKNOWN_ERROR_FALLBACK);
  });
});

describe("voice switch", () => {
  it("edits while the switch is on or absent", () => {
    expect(voiceSection(undefined)).toEqual({ kind: "editable" });
    expect(voiceSection({ has_api_key: false, enabled: true })).toEqual({ kind: "editable" });
  });

  it("hides when off with no saved key, and keeps removal with one", () => {
    expect(voiceSection({ has_api_key: false, enabled: false })).toEqual({ kind: "hidden" });
    expect(voiceSection({ has_api_key: true, enabled: false })).toEqual({ kind: "removal-only" });
  });
});

describe("tool-server prefixes", () => {
  it("prefers the resolved prefix and falls back to the slug", () => {
    expect(serverPrefix({ effective_tool_prefix: "abc", tool_prefix: "x" })).toBe("abc_*");
    expect(serverPrefix({ effective_tool_prefix: null, tool_prefix: "x" })).toBe("x_*");
    expect(serverPrefix({ effective_tool_prefix: "", tool_prefix: "" })).toBeNull();
    expect(serverPrefixLine({ effective_tool_prefix: null, tool_prefix: "x" })).toBe(`${SERVER_PREFIX_PREFIX} x_*`);
    expect(serverPrefixLine({ effective_tool_prefix: "", tool_prefix: "" })).toBeNull();
  });
});

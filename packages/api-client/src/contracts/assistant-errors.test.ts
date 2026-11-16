// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant failure reader against fabricated errors (no backend).
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { AssistantApiError, assistantErrorFrom } from "./assistant-errors.js";

describe("assistantErrorFrom", () => {
  it("reads the code, detail and status off a refusal", () => {
    const failure = assistantErrorFrom(
      new ApiError({
        status: 422,
        code: "http",
        message: "Configure your AI provider in Settings.",
        body: { error: "llm_config_missing", detail: "Configure your AI provider in Settings." },
      })
    );
    expect(failure).toEqual({
      code: "llm_config_missing",
      detail: "Configure your AI provider in Settings.",
      status: 422,
    });
  });

  it("defaults a missing detail to empty", () => {
    const failure = assistantErrorFrom(
      new ApiError({ status: 404, code: "http", message: "not_found", body: { error: "not_found" } })
    );
    expect(failure).toEqual({ code: "not_found", detail: "", status: 404 });
  });

  it("returns null for non-assistant failures", () => {
    expect(assistantErrorFrom(new Error("plain"))).toBeNull();
    expect(assistantErrorFrom(new ApiError({ status: 500, code: "http", message: "boom", body: null }))).toBeNull();
    expect(
      assistantErrorFrom(new ApiError({ status: 400, code: "http", message: "x", body: { other: 1 } }))
    ).toBeNull();
  });

  it("accepts the bare refusal shape", () => {
    expect(() => AssistantApiError.parse({ error: "turn_active" })).not.toThrow();
  });
});

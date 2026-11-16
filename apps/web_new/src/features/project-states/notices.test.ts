// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ApiError } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { isDuplicateStateError, isOccupiedStateError, stateFailureDetail, STATE_FAILURE_FALLBACK } from "./notices.js";

function apiError(status: number, message = "", fields?: Record<string, string[]>): ApiError {
  return new ApiError({ status, code: "http", message, ...(fields === undefined ? {} : { fields }) });
}

/**
 * The error the fetch client builds from a raw refusal body: `error`
 * becomes the message, any other shape stays "Request failed", and the
 * parsed body is kept.
 */
function refusal(status: number, body: Record<string, unknown>): ApiError {
  const message = typeof body["error"] === "string" ? body["error"] : "Request failed";
  return new ApiError({ status, code: "http", message, body });
}

describe("isDuplicateStateError", () => {
  it("recognises a 400 whose field errors name `name`", () => {
    expect(isDuplicateStateError(apiError(400, "", { name: ["The state name is already taken"] }))).toBe(true);
  });

  it("recognises the string `name` body the state views answer an integrity clash with", () => {
    expect(isDuplicateStateError(refusal(400, { name: "The state name is already taken" }))).toBe(true);
  });

  it("rejects other statuses, other fields and non-API errors", () => {
    expect(isDuplicateStateError(apiError(500, "", { name: ["x"] }))).toBe(false);
    expect(isDuplicateStateError(apiError(400, "", { color: ["x"] }))).toBe(false);
    expect(isDuplicateStateError(apiError(400, "boom"))).toBe(false);
    expect(isDuplicateStateError(new Error("boom"))).toBe(false);
    expect(isDuplicateStateError(refusal(400, { color: "bad" }))).toBe(false);
    expect(isDuplicateStateError(refusal(400, { name: "taken", group: "bad" }))).toBe(false);
    expect(isDuplicateStateError(refusal(400, {}))).toBe(false);
  });
});

describe("isOccupiedStateError", () => {
  it("recognises a refusal that talks about work items", () => {
    expect(isOccupiedStateError(apiError(400, "This state still has issues; move them"))).toBe(true);
    expect(isOccupiedStateError(apiError(409, "State is in use"))).toBe(true);
  });

  it("recognises the exact body the state view answers a non-empty delete with", () => {
    expect(
      isOccupiedStateError(refusal(400, { error: "The state is not empty, only empty states can be deleted" }))
    ).toBe(true);
  });

  it("rejects unrelated failures, other statuses and non-API errors", () => {
    expect(isOccupiedStateError(apiError(400, "Already exists"))).toBe(false);
    expect(isOccupiedStateError(apiError(400, "Default state cannot be deleted"))).toBe(false);
    expect(isOccupiedStateError(new Error("move"))).toBe(false);
  });
});

describe("stateFailureDetail", () => {
  it("prefers the server message", () => {
    expect(stateFailureDetail(apiError(500, "Server exploded"))).toBe("Server exploded");
  });

  it("falls back when the message is empty or the error is not an API error", () => {
    expect(stateFailureDetail(apiError(500))).toBe(STATE_FAILURE_FALLBACK);
    expect(stateFailureDetail(new Error("x"))).toBe(STATE_FAILURE_FALLBACK);
  });
});

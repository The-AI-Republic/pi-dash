// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook detail read states (WSET-036, spec §36): loading resolves to
// the page, to a missing notice on 404, or to a retryable error — never
// to an endless spinner for an unknown id (NEWFRONT-417).
import { ApiError } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { isMissingWebhookError, resolveWebhookDetailStatus } from "./webhookRoute.js";

describe("isMissingWebhookError", () => {
  it("recognises a 404 and nothing else", () => {
    expect(isMissingWebhookError(new ApiError({ status: 404, code: "http", message: "gone", body: null }))).toBe(true);
    expect(isMissingWebhookError({ status: 404 })).toBe(true);
    expect(isMissingWebhookError({ status: 500 })).toBe(false);
    expect(isMissingWebhookError(null)).toBe(false);
    expect(isMissingWebhookError("404")).toBe(false);
  });
});

describe("resolveWebhookDetailStatus", () => {
  it("is loading while the first read runs", () => {
    expect(resolveWebhookDetailStatus({ status: "pending" })).toBe("loading");
  });

  it("is ready once data arrives, even during a refetch", () => {
    expect(resolveWebhookDetailStatus({ status: "success", data: { id: "x" }, fetchStatus: "fetching" })).toBe("ready");
  });

  it("explains a 404 as missing and other failures as errors", () => {
    const missing = new ApiError({ status: 404, code: "http", message: "gone", body: null });
    expect(resolveWebhookDetailStatus({ status: "error", error: missing })).toBe("missing");
    expect(resolveWebhookDetailStatus({ status: "error", error: { status: 500 } })).toBe("error");
  });

  it("falls back to loading for a success without data", () => {
    expect(resolveWebhookDetailStatus({ status: "success" })).toBe("loading");
  });
});

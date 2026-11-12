// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration channel frames (parity/specs/pages.md §8.6, PAGE-069):
// addressed error events decode to phase, code, message and the
// disconnect flag; force-close notices decode ahead of the dedicated
// close; anything else is ignored.

import { describe, expect, it } from "vitest";

import {
  PAGE_LIVE_CLOSE_TOO_LARGE,
  PAGE_LIVE_REASON_TOO_LARGE,
  pageLiveCloseKind,
  parsePageLivePayload,
} from "./pageLiveFrames.js";

const PAGE_ID = "323e4567-e89b-12d3-a456-426614174002";

function errorEvent(overrides: Record<string, unknown> = {}): string {
  return JSON.stringify({
    action: "error",
    affectedPages: { currentPage: PAGE_ID },
    data: {
      error_message: "The save was refused.",
      error_type: "store",
      error_code: "content_too_large",
      should_disconnect: true,
    },
    ...overrides,
  });
}

describe("pageLiveCloseKind", () => {
  it("pins the oversized close and reason", () => {
    expect(PAGE_LIVE_CLOSE_TOO_LARGE).toBe(4001);
    expect(PAGE_LIVE_REASON_TOO_LARGE).toBe("document_too_large");
  });

  it("ends the session on auth closes", () => {
    expect(pageLiveCloseKind(4401)).toBe("auth");
    expect(pageLiveCloseKind(4403)).toBe("auth");
  });

  it("names the force-close family", () => {
    for (const code of [4000, 4001, 4002, 4003]) expect(pageLiveCloseKind(code)).toBe("force");
  });

  it("retries everything else", () => {
    for (const code of [1000, 1001, 1005, 1006, 1011, 4408]) expect(pageLiveCloseKind(code)).toBe("retryable");
  });
});

describe("parsePageLivePayload", () => {
  it("decodes a persist failure", () => {
    expect(parsePageLivePayload(errorEvent(), PAGE_ID)).toEqual({
      kind: "error",
      error: {
        phase: "persist",
        code: "content_too_large",
        message: "The save was refused.",
        shouldDisconnect: true,
      },
    });
  });

  it("maps a fetch failure to the load phase", () => {
    const payload = JSON.stringify({
      action: "error",
      affectedPages: { currentPage: PAGE_ID },
      data: { error_message: "The load failed.", error_type: "fetch" },
    });
    expect(parsePageLivePayload(payload, PAGE_ID)).toEqual({
      kind: "error",
      error: { phase: "load", code: "live_error", message: "The load failed.", shouldDisconnect: false },
    });
  });

  it("falls back when the code is empty or missing", () => {
    for (const data of [
      { error_message: "No.", error_type: "store", error_code: "" },
      { error_message: "No.", error_type: "store", error_code: 7 },
      { error_message: "No.", error_type: "store" },
    ]) {
      const payload = JSON.stringify({ action: "error", affectedPages: { currentPage: PAGE_ID }, data });
      expect(parsePageLivePayload(payload, PAGE_ID)).toMatchObject({ kind: "error", error: { code: "live_error" } });
    }
  });

  it("decodes the double-encoded envelope", () => {
    expect(parsePageLivePayload(JSON.stringify(errorEvent()), PAGE_ID)?.kind).toBe("error");
  });

  it("ignores frames for another page", () => {
    expect(parsePageLivePayload(errorEvent(), "other-page")).toBeNull();
  });

  it("ignores malformed error frames", () => {
    const malformed = [
      "not json",
      JSON.stringify("still not an object"),
      JSON.stringify(["error"]),
      JSON.stringify(7),
      JSON.stringify({ action: "error" }),
      JSON.stringify({ action: "error", affectedPages: null }),
      JSON.stringify({ action: "error", affectedPages: { currentPage: PAGE_ID } }),
      JSON.stringify({ action: "error", affectedPages: { currentPage: PAGE_ID }, data: "nope" }),
      JSON.stringify({
        action: "error",
        affectedPages: { currentPage: PAGE_ID },
        data: { error_type: "store" },
      }),
      JSON.stringify({
        action: "error",
        affectedPages: { currentPage: PAGE_ID },
        data: { error_message: "", error_type: "store" },
      }),
      JSON.stringify({
        action: "error",
        affectedPages: { currentPage: PAGE_ID },
        data: { error_message: 7, error_type: "store" },
      }),
      JSON.stringify({
        action: "error",
        affectedPages: { currentPage: PAGE_ID },
        data: { error_message: "No.", error_type: "bogus" },
      }),
      JSON.stringify({ action: "property_updated", data: {} }),
    ];
    for (const payload of malformed) expect(parsePageLivePayload(payload, PAGE_ID)).toBeNull();
  });

  it("decodes a force-close notice", () => {
    const payload = JSON.stringify({
      type: "force_close",
      reason: "document_too_large",
      code: 4001,
      message: "The document is too large.",
      timestamp: "2026-10-26T00:00:00Z",
    });
    expect(parsePageLivePayload(payload, PAGE_ID)).toEqual({
      kind: "force-close",
      notice: { reason: "document_too_large", code: 4001, message: "The document is too large." },
    });
  });

  it("tolerates a sparse force-close notice", () => {
    expect(parsePageLivePayload(JSON.stringify({ type: "force_close" }), PAGE_ID)).toEqual({
      kind: "force-close",
      notice: { reason: "", code: 4000, message: "" },
    });
    expect(
      parsePageLivePayload(JSON.stringify({ type: "force_close", reason: 7, code: "x", message: null }), PAGE_ID)
    ).toEqual({ kind: "force-close", notice: { reason: "", code: 4000, message: "" } });
  });
});

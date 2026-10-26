// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page editor assistant (parity/specs/pages.md §9.3, PAGE-075): the
// availability gate, the tone presets and their task composition, the
// inline failure lines, and the below-selection markup wrap.

import { describe, expect, it } from "vitest";

import {
  PAGE_ASSISTANT_DEFAULT_TONE,
  PAGE_ASSISTANT_FAILED_BODY,
  PAGE_ASSISTANT_QUOTA_BODY,
  PAGE_ASSISTANT_TONES,
  pageAssistantBelowHtml,
  pageAssistantFailureMessage,
  pageAssistantReady,
  pageAssistantTask,
} from "./pageAssistant.js";

describe("pageAssistantReady", () => {
  it("requires the edition flag, member standing and a configured model", () => {
    expect(pageAssistantReady({ editionEnabled: true, authorized: true, configured: true })).toBe(true);
  });

  it("fails closed when any input is missing", () => {
    expect(pageAssistantReady({ editionEnabled: false, authorized: true, configured: true })).toBe(false);
    expect(pageAssistantReady({ editionEnabled: true, authorized: false, configured: true })).toBe(false);
    expect(pageAssistantReady({ editionEnabled: true, authorized: true, configured: false })).toBe(false);
    expect(pageAssistantReady({ editionEnabled: false, authorized: false, configured: false })).toBe(false);
  });
});

describe("pageAssistantTask", () => {
  it("sends the bare instruction under the default preset", () => {
    expect(pageAssistantTask("Summarize", PAGE_ASSISTANT_DEFAULT_TONE)).toBe("Summarize");
    expect(PAGE_ASSISTANT_TONES[0]).toEqual({ key: "default", label: "Default", direction: null });
  });

  it("extends the instruction with the preset's tone direction", () => {
    expect(pageAssistantTask("Summarize", "formal")).toBe("Summarize\n\nAnswer in a formal tone.");
    expect(pageAssistantTask("Summarize", "casual")).toBe("Summarize\n\nAnswer in a casual tone.");
  });

  it("leaves the instruction untouched for an unknown preset", () => {
    expect(pageAssistantTask("Summarize", "pirate")).toBe("Summarize");
  });

  it("offers exactly the three pinned presets in order", () => {
    expect(PAGE_ASSISTANT_TONES.map((tone) => tone.label)).toEqual(["Default", "Formal", "Casual"]);
  });
});

describe("pageAssistantFailureMessage", () => {
  it("prefers the server's detail, then its code", () => {
    expect(pageAssistantFailureMessage({ body: { detail: "No model." } })).toBe("No model.");
    expect(pageAssistantFailureMessage({ status: 400, body: { error: "Task is required" } })).toBe("Task is required");
  });

  it("answers the quota line for a bare 429", () => {
    expect(pageAssistantFailureMessage({ status: 429 })).toBe(PAGE_ASSISTANT_QUOTA_BODY);
    expect(pageAssistantFailureMessage({ status: 429, body: { detail: "Slow down." } })).toBe("Slow down.");
  });

  it("falls back to the generic line without a server message", () => {
    expect(pageAssistantFailureMessage(new Error("boom"))).toBe(PAGE_ASSISTANT_FAILED_BODY);
    expect(pageAssistantFailureMessage(null)).toBe(PAGE_ASSISTANT_FAILED_BODY);
    expect(pageAssistantFailureMessage({ body: null })).toBe(PAGE_ASSISTANT_FAILED_BODY);
    expect(pageAssistantFailureMessage({ body: { detail: "" } })).toBe(PAGE_ASSISTANT_FAILED_BODY);
    expect(pageAssistantFailureMessage({ body: { detail: 7 } })).toBe(PAGE_ASSISTANT_FAILED_BODY);
  });
});

describe("pageAssistantBelowHtml", () => {
  it("wraps the reply as one paragraph below the selection", () => {
    expect(pageAssistantBelowHtml("a<br/>b")).toBe("<p>a<br/>b</p>");
  });
});

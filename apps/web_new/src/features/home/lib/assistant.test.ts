// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  ASSISTANT_SUGGESTIONS,
  assistantLandingHref,
  assistantThreadHref,
  isAssistantConfigured,
  selectAssistantCardState,
} from "./assistant.js";

describe("isAssistantConfigured", () => {
  it("needs both a model name and a key", () => {
    expect(isAssistantConfigured({ model_name: "pilot", has_api_key: true })).toBe(true);
    expect(isAssistantConfigured({ model_name: "", has_api_key: true })).toBe(false);
    expect(isAssistantConfigured({ model_name: "   ", has_api_key: true })).toBe(false);
    expect(isAssistantConfigured({ model_name: "pilot", has_api_key: false })).toBe(false);
  });

  it("treats a missing config as unconfigured", () => {
    expect(isAssistantConfigured(null)).toBe(false);
    expect(isAssistantConfigured(undefined)).toBe(false);
  });
});

describe("selectAssistantCardState", () => {
  it("hides the card from guests", () => {
    expect(selectAssistantCardState(false, true)).toBe("hidden");
    expect(selectAssistantCardState(false, false)).toBe("hidden");
  });

  it("shows setup or prompts to members", () => {
    expect(selectAssistantCardState(true, false)).toBe("setup");
    expect(selectAssistantCardState(true, true)).toBe("ready");
  });
});

describe("assistant hrefs", () => {
  it("points chips at the landing and threads by id", () => {
    expect(assistantLandingHref("acme")).toBe("/acme/assistant");
    expect(assistantThreadHref("acme", "thread-1")).toBe("/acme/assistant/thread-1");
  });
});

describe("ASSISTANT_SUGGESTIONS", () => {
  it("offers non-blank prompts", () => {
    expect(ASSISTANT_SUGGESTIONS.length).toBeGreaterThan(0);
    for (const suggestion of ASSISTANT_SUGGESTIONS) {
      expect(suggestion.trim().length).toBeGreaterThan(0);
    }
  });
});

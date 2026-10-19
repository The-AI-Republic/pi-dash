// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  API_KEY_REMINDER,
  assistantConfigQueryOptions,
  assistantLandingHref,
  assistantThreadHref,
  AssistantLanding,
  AssistantLayout,
  Composer,
  ConversationGate,
  ConversationRail,
  HANDOFF_FAILED_TITLE,
  needsSetup,
  SETUP_CARD_ACTION,
  SETUP_CARD_TITLE,
  SetupCard,
} from "./index.js";

describe("assistant feature public API", () => {
  it("exports the entry surfaces, data reads and pinned copy", () => {
    expect(AssistantLanding).toBeTypeOf("function");
    expect(AssistantLayout).toBeTypeOf("function");
    expect(AssistantLanding).toBeTypeOf("function");
    expect(Composer).toBeTypeOf("function");
    expect(ConversationGate).toBeTypeOf("function");
    expect(ConversationRail).toBeTypeOf("function");
    expect(SetupCard).toBeTypeOf("function");
    expect(assistantConfigQueryOptions).toBeTypeOf("function");
    expect(assistantLandingHref("acme")).toBe("/acme/assistant");
    expect(assistantThreadHref("acme", "t1")).toBe("/acme/assistant/t1");
    expect(needsSetup({ model_name: "", has_api_key: false })).toBe(true);
    expect(SETUP_CARD_TITLE).toBe("Set up your AI assistant");
    expect(SETUP_CARD_ACTION).toBe("Configure provider");
    expect(API_KEY_REMINDER).toContain("Please set your API key");
    expect(HANDOFF_FAILED_TITLE).toBe("Could not send your message");
  });
});

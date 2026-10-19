// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { AssistantLanding } from "./components/AssistantLanding.js";
import { AssistantLayout } from "./components/AssistantLayout.js";
import { Composer } from "./components/Composer.js";
import { ConversationGate } from "./components/ConversationGate.js";
import { ConversationRail } from "./components/ConversationRail.js";
import { SetupCard } from "./components/SetupCard.js";
import {
  API_KEY_REMINDER,
  assistantConfigQueryOptions,
  assistantKeys,
  assistantLandingHref,
  assistantThreadHref,
  HANDOFF_FAILED_TITLE,
  needsLaneSetup,
  SETUP_CARD_ACTION,
  SETUP_CARD_TITLE,
} from "./index.js";

describe("assistant feature public API", () => {
  it("exports the routes' surfaces, and the barrel carries only the shared reads and copy", () => {
    expect(AssistantLanding).toBeTypeOf("function");
    expect(AssistantLayout).toBeTypeOf("function");
    expect(Composer).toBeTypeOf("function");
    expect(ConversationGate).toBeTypeOf("function");
    expect(ConversationRail).toBeTypeOf("function");
    expect(SetupCard).toBeTypeOf("function");
    expect(assistantKeys.all("acme")).toEqual(["ws", "acme", "assistant"]);
    expect(assistantConfigQueryOptions).toBeTypeOf("function");
    expect(assistantLandingHref("acme")).toBe("/acme/assistant");
    expect(assistantThreadHref("acme", "t1")).toBe("/acme/assistant/t1");
    expect(
      needsLaneSetup({
        provider_kind: "openai",
        base_url: "",
        model_name: "",
        has_api_key: false,
        last_verified_at: null,
      })
    ).toBe(true);
    expect(SETUP_CARD_TITLE).toBe("Set up your AI assistant");
    expect(SETUP_CARD_ACTION).toBe("Configure provider");
    expect(API_KEY_REMINDER).toContain("Please set your API key");
    expect(HANDOFF_FAILED_TITLE).toBe("Could not send your message");
  });
});

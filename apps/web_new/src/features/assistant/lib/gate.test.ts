// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { isAssistantConfigured } from "./gate.js";

describe("isAssistantConfigured", () => {
  it("needs both a model name and a stored key", () => {
    expect(isAssistantConfigured({ model_name: "pilot", has_api_key: true })).toBe(true);
    expect(isAssistantConfigured({ model_name: "pilot", has_api_key: false })).toBe(false);
    expect(isAssistantConfigured({ model_name: "", has_api_key: true })).toBe(false);
    expect(isAssistantConfigured({ model_name: "   ", has_api_key: true })).toBe(false);
  });

  it("treats a missing config as unconfigured", () => {
    expect(isAssistantConfigured(undefined)).toBe(false);
    expect(isAssistantConfigured(null)).toBe(false);
  });
});

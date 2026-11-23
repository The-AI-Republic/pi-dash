// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Key-form rules (CLOUD-033): what blocks save, which kind asks for a base,
// and what a valid draft sends. A new key carries a list of typed model
// IDs (NEWPIDASH-394): at least one, sent as `models` with the first ID
// also as `model_name` for older servers.
import { describe, expect, it } from "vitest";

import { EMPTY_BYOK_DRAFT, byokDraftValid, byokSubmission, needsBaseUrl, validateByokDraft } from "./byok.js";

describe("needsBaseUrl", () => {
  it("asks only the compatible kind", () => {
    expect(needsBaseUrl("openai_compatible")).toBe(true);
    expect(needsBaseUrl("anthropic")).toBe(false);
  });
});

describe("validateByokDraft", () => {
  it("blocks an empty form on every field it needs", () => {
    expect(validateByokDraft(EMPTY_BYOK_DRAFT)).toEqual({
      baseUrl: "Enter the address your provider answers on.",
      models: "Name the model the assistant should call.",
      apiKey: "Paste the key for this provider.",
    });
    expect(byokDraftValid(EMPTY_BYOK_DRAFT)).toBe(false);
  });

  it("treats whitespace as missing and an empty model list as missing", () => {
    const draft = { provider: "openai_compatible" as const, baseUrl: "   ", models: [], apiKey: "\t" };
    expect(byokDraftValid(draft)).toBe(false);
  });

  it("drops the base requirement for the single-vendor kind", () => {
    const draft = { provider: "anthropic" as const, baseUrl: "", models: ["claude"], apiKey: "sk" };
    expect(validateByokDraft(draft)).toEqual({ baseUrl: null, models: null, apiKey: null });
    expect(byokDraftValid(draft)).toBe(true);
  });

  it("accepts a complete compatible form", () => {
    const draft = {
      provider: "openai_compatible" as const,
      baseUrl: "https://openrouter.ai/api/v1",
      models: ["model"],
      apiKey: "key",
    };
    expect(byokDraftValid(draft)).toBe(true);
  });
});

describe("byokSubmission", () => {
  it("trims every value, sends the model-ID list, and carries the base for the compatible kind", () => {
    expect(
      byokSubmission({
        provider: "openai_compatible",
        baseUrl: "  https://openrouter.ai/api/v1  ",
        models: ["meta-llama/llama-3.3-70b-instruct", "gpt-6"],
        apiKey: "  secret ",
      })
    ).toEqual({
      providerKind: "openai_compatible",
      baseUrl: "https://openrouter.ai/api/v1",
      modelName: "meta-llama/llama-3.3-70b-instruct",
      models: ["meta-llama/llama-3.3-70b-instruct", "gpt-6"],
      apiKey: "secret",
    });
  });

  it("sends no base for the single-vendor kind", () => {
    expect(
      byokSubmission({ provider: "anthropic", baseUrl: "https://ignored.test", models: ["claude"], apiKey: " k " })
    ).toEqual({ providerKind: "anthropic", baseUrl: null, modelName: "claude", models: ["claude"], apiKey: "k" });
  });
});

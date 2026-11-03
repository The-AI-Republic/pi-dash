// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Key-form rules (CLOUD-033): what blocks save, which kind asks for a base,
// and what a valid draft sends.
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
      modelName: "Name the model the assistant should call.",
      apiKey: "Paste the key for this provider.",
    });
    expect(byokDraftValid(EMPTY_BYOK_DRAFT)).toBe(false);
  });

  it("treats whitespace as missing", () => {
    const draft = { provider: "openai_compatible" as const, baseUrl: "   ", modelName: "  ", apiKey: "\t" };
    expect(byokDraftValid(draft)).toBe(false);
  });

  it("drops the base requirement for the single-vendor kind", () => {
    const draft = { provider: "anthropic" as const, baseUrl: "", modelName: "claude", apiKey: "sk" };
    expect(validateByokDraft(draft)).toEqual({ baseUrl: null, modelName: null, apiKey: null });
    expect(byokDraftValid(draft)).toBe(true);
  });

  it("accepts a complete compatible form", () => {
    const draft = {
      provider: "openai_compatible" as const,
      baseUrl: "https://openrouter.ai/api/v1",
      modelName: "model",
      apiKey: "key",
    };
    expect(byokDraftValid(draft)).toBe(true);
  });
});

describe("byokSubmission", () => {
  it("trims every value and carries the base for the compatible kind", () => {
    expect(
      byokSubmission({
        provider: "openai_compatible",
        baseUrl: "  https://openrouter.ai/api/v1  ",
        modelName: "  meta-llama/llama-3.3-70b-instruct ",
        apiKey: "  secret ",
      })
    ).toEqual({
      providerKind: "openai_compatible",
      baseUrl: "https://openrouter.ai/api/v1",
      modelName: "meta-llama/llama-3.3-70b-instruct",
      apiKey: "secret",
    });
  });

  it("sends no base for the single-vendor kind", () => {
    expect(
      byokSubmission({ provider: "anthropic", baseUrl: "https://ignored.test", modelName: " claude ", apiKey: " k " })
    ).toEqual({ providerKind: "anthropic", baseUrl: null, modelName: "claude", apiKey: "k" });
  });
});

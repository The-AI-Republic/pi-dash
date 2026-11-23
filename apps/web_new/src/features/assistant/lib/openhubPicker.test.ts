// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// OpenHub composer picker state (NEWPIDASH-303): the visibility gate
// (OpenHub lane with access and a selected OpenHub row only) and the
// option labels (credit badge plus the default mark that points a
// credit-blocked turn back at the free model). Plus the merged picker
// list (NEWPIDASH-394): catalog models first, then BYOK models grouped
// by key, with the shared fallback notice.
import { describe, expect, it } from "vitest";

import type { AssistantConnectionList, LlmConfig } from "@pidash/api-client";

import type { ModelChoice } from "../../cloud-assistant/index.js";
import {
  buildMergedModelChoices,
  byokChoiceLabel,
  fallbackNotice,
  OPENHUB_DEFAULT_MODEL,
  OPENHUB_DEFAULT_SUFFIX,
  pickerOptionLabel,
  selectedOpenHubConnection,
  shouldShowOpenHubPicker,
  type MergedPickerCatalog,
} from "./openhubPicker.js";

const ROW = {
  id: "123e4567-e89b-12d3-a456-426614174001",
  kind: "openhub" as const,
  provider_kind: "openhub",
  base_url: "",
  model_name: "moonshotai/kimi-k3",
  has_api_key: true,
  is_selected: true,
  last_verified_at: null,
  created_at: null,
  connected: true,
  uses_chatgpt_plan: false,
};

const LIST: AssistantConnectionList = {
  connections: [ROW],
  codex_model_options: [],
  has_codex_credential: false,
  openai_login_available: false,
  openhub_model_options: ["moonshotai/kimi-k3", "openhub_free_llm"],
  openhub_wallet_url: "",
};

const OPENHUB: LlmConfig = {
  provider_kind: "openhub",
  base_url: "",
  model_name: "moonshotai/kimi-k3",
  has_api_key: true,
  last_verified_at: null,
  credential_mode: "openhub",
  has_openhub_access: true,
};

describe("selectedOpenHubConnection", () => {
  it("returns the selected OpenHub row for an OpenHub lane with access", () => {
    expect(selectedOpenHubConnection(OPENHUB, LIST)).toEqual(ROW);
  });

  it("hides the picker for a self-supplied lane", () => {
    expect(selectedOpenHubConnection({ ...OPENHUB, credential_mode: "byok", has_byok_api_key: true }, LIST)).toBeNull();
  });

  it("hides the picker for a Codex lane", () => {
    expect(selectedOpenHubConnection({ ...OPENHUB, credential_mode: "codex", has_codex_token: true }, LIST)).toBeNull();
  });

  it("hides the picker without OpenHub access", () => {
    expect(selectedOpenHubConnection({ ...OPENHUB, has_openhub_access: false }, LIST)).toBeNull();
    expect(selectedOpenHubConnection({ ...OPENHUB, has_openhub_access: undefined }, LIST)).toBeNull();
  });

  it("hides the picker when no OpenHub row is selected", () => {
    const unselected = { ...LIST, connections: [{ ...ROW, is_selected: false }] };
    expect(selectedOpenHubConnection(OPENHUB, unselected)).toBeNull();
  });

  it("hides the picker while the config or the list is unloaded", () => {
    expect(selectedOpenHubConnection(undefined, LIST)).toBeNull();
    expect(selectedOpenHubConnection(null, LIST)).toBeNull();
    expect(selectedOpenHubConnection(OPENHUB, undefined)).toBeNull();
  });
});

describe("shouldShowOpenHubPicker", () => {
  it("mirrors the gate", () => {
    expect(shouldShowOpenHubPicker(OPENHUB, LIST)).toBe(true);
    expect(shouldShowOpenHubPicker({ ...OPENHUB, credential_mode: "byok" }, LIST)).toBe(false);
  });
});

describe("pickerOptionLabel", () => {
  const choice = (overrides: Partial<ModelChoice>): ModelChoice => ({
    value: "moonshotai/kimi-k3",
    label: "Balanced hosted model",
    credit: null,
    badge: null,
    selectable: true,
    ...overrides,
  });

  it("marks the default model", () => {
    expect(OPENHUB_DEFAULT_MODEL).toBe("openhub_free_llm");
    expect(pickerOptionLabel(choice({ value: OPENHUB_DEFAULT_MODEL }), true)).toBe(
      `Balanced hosted model${OPENHUB_DEFAULT_SUFFIX}`
    );
  });

  it("stamps the credit badge and the default mark together", () => {
    expect(pickerOptionLabel(choice({ badge: "Free" }), true)).toBe(
      `Balanced hosted model (Free)${OPENHUB_DEFAULT_SUFFIX}`
    );
  });

  it("leaves a non-default model unmarked", () => {
    expect(pickerOptionLabel(choice({}), false)).toBe("Balanced hosted model");
  });
});

describe("buildMergedModelChoices", () => {
  const catalog: MergedPickerCatalog = {
    models: ["openhub_free_llm", "moonshotai/kimi-k3", "deepseek/deepseek-v4-pro"],
    labels: { "moonshotai/kimi-k3": "Kimi K3" },
    descriptions: { "moonshotai/kimi-k3": "Balanced hosted model", "deepseek/deepseek-v4-pro": "Deepest reasoning" },
  };
  const keys = [
    {
      connectionId: "123e4567-e89b-12d3-a456-426614174001",
      models: ["gpt-6", "gpt-6.2"],
      baseUrl: "https://llm.example.com/v1",
      providerKind: "openai_compatible",
    },
    {
      connectionId: "123e4567-e89b-12d3-a456-426614174002",
      models: ["claude-sonnet-4-6"],
      baseUrl: "",
      providerKind: "anthropic",
    },
  ];

  it("lists catalog models first, then BYOK models grouped by key", () => {
    const choices = buildMergedModelChoices(catalog, keys);
    expect(choices.map((choice) => choice.value)).toEqual([
      "openhub_free_llm",
      "moonshotai/kimi-k3",
      "deepseek/deepseek-v4-pro",
      "gpt-6",
      "gpt-6.2",
      "claude-sonnet-4-6",
    ]);
    expect(choices.map((choice) => choice.origin)).toEqual(["catalog", "catalog", "catalog", "byok", "byok", "byok"]);
  });

  it("labels catalog models by label, then description, then ID", () => {
    const choices = buildMergedModelChoices(catalog, []);
    expect(choices.map((choice) => choice.label)).toEqual(["openhub_free_llm", "Kimi K3", "Deepest reasoning"]);
  });

  it("labels BYOK models with the key host or provider", () => {
    const choices = buildMergedModelChoices({ models: [], labels: {}, descriptions: {} }, keys);
    expect(choices.map((choice) => choice.label)).toEqual([
      "gpt-6 (BYOK · llm.example.com)",
      "gpt-6.2 (BYOK · llm.example.com)",
      "claude-sonnet-4-6 (BYOK · anthropic)",
    ]);
    expect(choices.map((choice) => choice.connectionId)).toEqual([
      "123e4567-e89b-12d3-a456-426614174001",
      "123e4567-e89b-12d3-a456-426614174001",
      "123e4567-e89b-12d3-a456-426614174002",
    ]);
  });

  it("keeps every entry selectable with no credit badge", () => {
    const choices = buildMergedModelChoices(catalog, keys);
    for (const choice of choices) {
      expect(choice.selectable).toBe(true);
      expect(choice.badge).toBeNull();
      expect(choice.credit).toBeNull();
    }
  });

  it("lists catalog models only for the agent surface", () => {
    const choices = buildMergedModelChoices(catalog, keys, { includeByok: false });
    expect(choices.map((choice) => choice.value)).toEqual([
      "openhub_free_llm",
      "moonshotai/kimi-k3",
      "deepseek/deepseek-v4-pro",
    ]);
  });

  it("keeps the same ID listed under two keys as two choices", () => {
    const choices = buildMergedModelChoices({ models: [], labels: {}, descriptions: {} }, [
      { ...keys[0]!, models: ["gpt-6"] },
      { ...keys[1]!, models: ["gpt-6"] },
    ]);
    expect(choices).toHaveLength(2);
    expect(choices[0]?.connectionId).toBe("123e4567-e89b-12d3-a456-426614174001");
    expect(choices[1]?.connectionId).toBe("123e4567-e89b-12d3-a456-426614174002");
  });
});

describe("byokChoiceLabel", () => {
  it("names the base-URL host", () => {
    expect(byokChoiceLabel("gpt-6", { baseUrl: "https://llm.example.com/v1/", providerKind: "x" })).toBe(
      "gpt-6 (BYOK · llm.example.com)"
    );
  });

  it("falls back to the provider without a base URL", () => {
    expect(byokChoiceLabel("claude-sonnet-4-6", { baseUrl: "", providerKind: "anthropic" })).toBe(
      "claude-sonnet-4-6 (BYOK · anthropic)"
    );
  });

  it("falls back to the provider for an unparseable address", () => {
    expect(byokChoiceLabel("gpt-6", { baseUrl: "://bad", providerKind: "custom" })).toBe("gpt-6 (BYOK · custom)");
  });
});

describe("fallbackNotice", () => {
  it("names the saved model and the effective label", () => {
    expect(fallbackNotice("modelC", "OpenHub free model")).toBe(
      "modelC is no longer available. Using OpenHub free model."
    );
  });
});

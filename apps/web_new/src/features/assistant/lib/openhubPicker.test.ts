// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// OpenHub composer picker state (NEWPIDASH-303, rewritten NEWPIDASH-394):
// the option labels (credit badge plus the default mark that points a
// credit-blocked turn back at the free model), the merged picker list
// (catalog models first, then BYOK models grouped by key, with the
// shared fallback notice), and the list-derived builder inputs.
import { describe, expect, it } from "vitest";

import type { AssistantConnectionList } from "@pidash/api-client";

import {
  OPENHUB_MODEL_DESCRIPTIONS,
  type ConnectionModelFlags,
  type ModelChoice,
} from "../../cloud-assistant/index.js";
import {
  applyCreditBadges,
  buildMergedModelChoices,
  byokChoiceLabel,
  byokKeyFromConfig,
  byokKeysFromList,
  catalogFromList,
  effectiveChoiceFor,
  fallbackNotice,
  mergedChoiceKey,
  OPENHUB_DEFAULT_MODEL,
  OPENHUB_DEFAULT_SUFFIX,
  pickerOptionLabel,
  type MergedPickerCatalog,
  type MergedPickerChoice,
} from "./openhubPicker.js";

const BYOK_ROW = {
  id: "123e4567-e89b-12d3-a456-426614174001",
  kind: "byok" as const,
  provider_kind: "openai_compatible",
  base_url: "https://llm.example.com/v1",
  model_name: "gpt-6",
  models: ["gpt-6", "gpt-6.2"],
  model_verified_at: {},
  has_api_key: true,
  is_selected: false,
  last_verified_at: null,
  created_at: null,
  connected: true,
  uses_chatgpt_plan: false,
};

const LIST: AssistantConnectionList = {
  connections: [BYOK_ROW],
  codex_model_options: [],
  has_codex_credential: false,
  openai_login_available: false,
  openhub_model_options: ["moonshotai/kimi-k3", "openhub_free_llm"],
  openhub_model_labels: { "moonshotai/kimi-k3": "Kimi K3" },
  openhub_wallet_url: "",
};

describe("catalogFromList", () => {
  it("reads god-mode order with the server labels when present", () => {
    expect(catalogFromList(LIST)).toEqual({
      models: ["moonshotai/kimi-k3", "openhub_free_llm"],
      labels: { "moonshotai/kimi-k3": "Kimi K3" },
      descriptions: OPENHUB_MODEL_DESCRIPTIONS,
    });
  });

  it("defaults a missing labels map to empty", () => {
    const { openhub_model_labels: _dropped, ...rest } = LIST;
    expect(catalogFromList(rest).labels).toEqual({});
  });
});

describe("byokKeysFromList", () => {
  it("contributes every BYOK key in list order and skips other lanes", () => {
    expect(byokKeysFromList(LIST)).toEqual([
      {
        connectionId: "123e4567-e89b-12d3-a456-426614174001",
        models: ["gpt-6", "gpt-6.2"],
        baseUrl: "https://llm.example.com/v1",
        providerKind: "openai_compatible",
      },
    ]);
  });

  it("falls back to the legacy single model without a stored list", () => {
    const { models: _dropped, ...legacy } = BYOK_ROW;
    expect(byokKeysFromList({ ...LIST, connections: [{ ...legacy, models: undefined }] })[0]?.models).toEqual([
      "gpt-6",
    ]);
  });

  it("contributes no IDs for a blank legacy model", () => {
    const { models: _dropped, ...legacy } = BYOK_ROW;
    expect(
      byokKeysFromList({ ...LIST, connections: [{ ...legacy, models: undefined, model_name: "" }] })[0]?.models
    ).toEqual([]);
  });
});

describe("byokKeyFromConfig", () => {
  it("reads the stored list with the key's address", () => {
    expect(
      byokKeyFromConfig({
        models: ["gpt-6", "gpt-6.2"],
        model_name: "gpt-6",
        base_url: "https://llm.example.com/v1",
        provider_kind: "openai_compatible",
      })
    ).toEqual({
      connectionId: undefined,
      models: ["gpt-6", "gpt-6.2"],
      baseUrl: "https://llm.example.com/v1",
      providerKind: "openai_compatible",
    });
  });

  it("falls back to the legacy single model without a stored list", () => {
    expect(byokKeyFromConfig({ model_name: "gpt-6", base_url: "", provider_kind: "anthropic" }).models).toEqual([
      "gpt-6",
    ]);
  });

  it("contributes no IDs for a blank legacy model", () => {
    expect(byokKeyFromConfig({ model_name: "", base_url: "", provider_kind: "x" }).models).toEqual([]);
  });
});

describe("mergedChoiceKey", () => {
  it("namespaces catalog and BYOK choices apart", () => {
    expect(mergedChoiceKey({ origin: "catalog", value: "gpt-6", connectionId: undefined })).toBe("catalog:gpt-6");
    expect(
      mergedChoiceKey({
        origin: "byok",
        value: "gpt-6",
        connectionId: "123e4567-e89b-12d3-a456-426614174001",
      })
    ).toBe("byok:123e4567-e89b-12d3-a456-426614174001:gpt-6");
  });

  it("keys an OSS BYOK choice without a connection row apart from the catalog", () => {
    expect(mergedChoiceKey({ origin: "byok", value: "gpt-6", connectionId: undefined })).toBe("byok:local:gpt-6");
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

describe("effectiveChoiceFor", () => {
  const catalog: MergedPickerCatalog = {
    models: ["openhub_free_llm", "moonshotai/kimi-k3"],
    labels: {},
    descriptions: {},
  };
  const keys = [
    {
      connectionId: "123e4567-e89b-12d3-a456-426614174001",
      models: ["gpt-6"],
      baseUrl: "https://llm.example.com/v1",
      providerKind: "openai_compatible",
    },
  ];
  const choices: MergedPickerChoice[] = buildMergedModelChoices(catalog, keys);

  it("matches a catalog default by model with no connection", () => {
    expect(
      effectiveChoiceFor(choices, {
        saved: null,
        effective: { model: "openhub_free_llm", connection_id: null },
        fallback_reason: "none_saved",
      })?.value
    ).toBe("openhub_free_llm");
  });

  it("matches a BYOK choice by model and connection", () => {
    expect(
      effectiveChoiceFor(choices, {
        saved: { model: "gpt-6", connection_id: "123e4567-e89b-12d3-a456-426614174001" },
        effective: { model: "gpt-6", connection_id: "123e4567-e89b-12d3-a456-426614174001" },
        fallback_reason: null,
      })?.origin
    ).toBe("byok");
  });

  it("returns null when the server honors nothing in the list", () => {
    expect(
      effectiveChoiceFor(choices, { saved: null, effective: null, fallback_reason: "lane_unavailable" })
    ).toBeNull();
    expect(
      effectiveChoiceFor(choices, {
        saved: { model: "retired", connection_id: null },
        effective: { model: "removed", connection_id: null },
        fallback_reason: "model_removed",
      })
    ).toBeNull();
  });
});

describe("applyCreditBadges", () => {
  const catalog: MergedPickerCatalog = {
    models: ["openhub_free_llm", "moonshotai/kimi-k3"],
    labels: {},
    descriptions: {},
  };
  const keys = [
    {
      connectionId: "123e4567-e89b-12d3-a456-426614174001",
      models: ["gpt-6"],
      baseUrl: "https://llm.example.com/v1",
      providerKind: "openai_compatible",
    },
  ];
  const flags = (overrides: Partial<ConnectionModelFlags> = {}): ConnectionModelFlags => ({
    codexModels: [],
    codexModelLabels: {},
    openhubModels: ["openhub_free_llm", "moonshotai/kimi-k3"],
    openhubModelLabels: {},
    freeModels: ["openhub_free_llm"],
    quotaPlan: "free",
    ...overrides,
  });

  it("blocks a paid catalog model on the free plan and leaves BYOK selectable", () => {
    const stamped = applyCreditBadges(buildMergedModelChoices(catalog, keys), flags());
    expect(stamped.find((choice) => choice.value === "moonshotai/kimi-k3")).toMatchObject({
      credit: "blocked",
      selectable: false,
    });
    expect(stamped.find((choice) => choice.value === "openhub_free_llm")).toMatchObject({
      credit: "free",
      selectable: true,
    });
    expect(stamped.find((choice) => choice.value === "gpt-6")).toMatchObject({
      credit: null,
      badge: null,
      selectable: true,
    });
  });

  it("keeps every catalog model selectable on a paying plan", () => {
    const stamped = applyCreditBadges(buildMergedModelChoices(catalog, keys), flags({ quotaPlan: "team" }));
    for (const choice of stamped) expect(choice.selectable).toBe(true);
  });
});

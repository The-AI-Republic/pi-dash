// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Connection row model (CLOUD-032–034): titles per kind, markers, which
// control edits each model, which rows a session can use, and which rows a
// stale session must still show with its notice.
import { describe, expect, it } from "vitest";

import type { AssistantConnectionList, AssistantConnectionListItem } from "@pidash/api-client";

import {
  BUILTIN_CODEX_MODELS,
  connectionModelFlags,
  connectionTitle,
  isDisconnected,
  isStaleHostedRow,
  modelChoices,
  toConnectionRow,
} from "./rows.js";

const IDS = {
  codex: "123e4567-e89b-12d3-a456-426614174000",
  byok: "123e4567-e89b-12d3-a456-426614174001",
  openhub: "123e4567-e89b-12d3-a456-426614174002",
};

function row(overrides: Partial<AssistantConnectionListItem> = {}): AssistantConnectionListItem {
  return {
    id: IDS.byok,
    kind: "byok",
    provider_kind: "openai_compatible",
    base_url: "https://openrouter.ai/api/v1",
    model_name: "meta-llama/llama-3.3-70b-instruct",
    has_api_key: true,
    is_selected: true,
    last_verified_at: null,
    created_at: "2026-01-01T00:00:00Z",
    connected: true,
    uses_chatgpt_plan: false,
    ...overrides,
  };
}

function list(overrides: Partial<AssistantConnectionList> = {}): AssistantConnectionList {
  return {
    connections: [row()],
    codex_model_options: [],
    codex_model_labels: {},
    has_codex_credential: false,
    openai_login_available: true,
    openhub_model_options: [],
    openhub_available: undefined,
    openhub_wallet_url: "",
    ...overrides,
  };
}

describe("connectionTitle", () => {
  it("names managed lanes after the product", () => {
    expect(connectionTitle(row({ kind: "openhub", base_url: "" }))).toBe("OpenHub");
    expect(connectionTitle(row({ kind: "codex", base_url: "" }))).toBe("OpenAI account");
  });

  it("names a self-supplied key after its host", () => {
    expect(connectionTitle(row())).toBe("BYOK:openrouter.ai");
    expect(connectionTitle(row({ base_url: "http://localhost:11434/v1" }))).toBe("BYOK:localhost:11434");
  });

  it("falls back to the provider family for an unusable address", () => {
    expect(connectionTitle(row({ base_url: "", provider_kind: "anthropic" }))).toBe("BYOK:Anthropic");
    expect(connectionTitle(row({ base_url: "", provider_kind: "openai_compatible" }))).toBe("BYOK");
    expect(connectionTitle(row({ base_url: "   " }))).toBe("BYOK");
    expect(connectionTitle(row({ base_url: "not a url" }))).toBe("BYOK");
    expect(connectionTitle(row({ base_url: "https://" }))).toBe("BYOK");
  });

  it("treats an address with no host as unusable", () => {
    expect(connectionTitle(row({ base_url: "mailto:someone@example.com" }))).toBe("BYOK");
  });
});

describe("isDisconnected", () => {
  it("applies only to the provider account row", () => {
    expect(isDisconnected(row({ kind: "codex", connected: false }))).toBe(true);
    expect(isDisconnected(row({ kind: "codex", connected: true }))).toBe(false);
    expect(isDisconnected(row({ kind: "byok", connected: false }))).toBe(false);
  });
});

describe("modelChoices", () => {
  it("keeps the stored model selectable even when the catalog dropped it", () => {
    expect(modelChoices(["a", "b"], {}, {}, "c")).toEqual([
      { value: "a", label: "a" },
      { value: "b", label: "b" },
      { value: "c", label: "c" },
    ]);
  });

  it("prefers provider labels over descriptions, and descriptions over values", () => {
    expect(modelChoices(["a"], { a: "Provider name" }, { a: "Description", b: "B desc" }, "b")).toEqual([
      { value: "a", label: "Provider name" },
      { value: "b", label: "B desc" },
    ]);
  });

  it("does not duplicate a stored model that is already offered, or an empty one", () => {
    expect(modelChoices(["a"], {}, {}, "a")).toEqual([{ value: "a", label: "a" }]);
    expect(modelChoices(["a"], {}, {}, "")).toEqual([{ value: "a", label: "a" }]);
  });
});

describe("connection flags", () => {
  it("copies the managed options the list answered", () => {
    const flags = connectionModelFlags(
      list({ codex_model_options: ["x"], codex_model_labels: { x: "X" }, openhub_model_options: ["y"] })
    );
    expect(flags).toEqual({ codexModels: ["x"], codexModelLabels: { x: "X" }, openhubModels: ["y"] });
  });
});

describe("isStaleHostedRow", () => {
  const hosted = row({ id: IDS.openhub, kind: "openhub", base_url: "", model_name: "moonshotai/kimi-k3" });

  it("marks only hosted rows on a gated-off lane as stale", () => {
    expect(isStaleHostedRow(hosted, false)).toBe(true);
    expect(isStaleHostedRow(hosted, true)).toBe(false);
    expect(isStaleHostedRow(row(), false)).toBe(false);
  });
});

describe("toConnectionRow", () => {
  it("renders a selected self-supplied row with markers, free-text model and Remove", () => {
    const answer = list({ connections: [row({ last_verified_at: "2026-02-03T04:05:06Z" })] });
    const target = toConnectionRow(answer.connections[0]!, answer, connectionModelFlags(answer));
    expect(target).toMatchObject({
      id: IDS.byok,
      kind: "byok",
      title: "BYOK:openrouter.ai",
      selected: true,
      selectable: true,
      stale: false,
      usesChatgptPlan: false,
      modelControl: "input",
      modelName: "meta-llama/llama-3.3-70b-instruct",
      verifiedAt: "2026-02-03T04:05:06Z",
      removable: true,
      disconnectable: false,
      disconnected: false,
    });
    expect(target.markers).toEqual(["In use"]);
    expect(target.modelChoices).toEqual([]);
    expect(target.modelSuggestions.length).toBeGreaterThan(0);
  });

  it("uses the built-in managed picker when the server sends no options", () => {
    const answer = list({
      connections: [row({ kind: "codex", base_url: "", model_name: "gpt-5.5" })],
      has_codex_credential: true,
    });
    const target = toConnectionRow(answer.connections[0]!, answer, connectionModelFlags(answer));
    expect(target.modelControl).toBe("select");
    expect(target.modelChoices.map((choice) => choice.value)).toEqual([...BUILTIN_CODEX_MODELS]);
    expect(target.markers).toEqual(["In use"]);
    expect(target.disconnectable).toBe(true);
    expect(target.removable).toBe(false);
  });

  it("shows the provider picker for the hosted lane and its account marker", () => {
    const answer = list({
      connections: [row({ kind: "openhub", base_url: "", model_name: "moonshotai/kimi-k3" })],
      openhub_model_options: ["moonshotai/kimi-k3"],
    });
    const target = toConnectionRow(answer.connections[0]!, answer, connectionModelFlags(answer));
    expect(target.modelControl).toBe("select");
    expect(target.modelChoices).toEqual([{ value: "moonshotai/kimi-k3", label: "Balanced hosted model" }]);
    expect(target.markers).toContain("Included in your plan");
    expect(target.selectable).toBe(true);
  });

  it("hides the control and blocks selection on a disconnected account row", () => {
    const answer = list({ connections: [row({ kind: "codex", base_url: "", connected: false, is_selected: false })] });
    const target = toConnectionRow(answer.connections[0]!, answer, connectionModelFlags(answer));
    expect(target.disconnected).toBe(true);
    expect(target.selectable).toBe(false);
    expect(target.modelControl).toBe("hidden");
    expect(target.disconnectable).toBe(false);
    expect(target.markers).toContain("Not connected");
  });

  it("keeps a stale hosted row but hides its control and selection", () => {
    const hosted = row({ kind: "openhub", base_url: "", is_selected: false });
    const answer = list({ connections: [hosted], openhub_available: false });
    const target = toConnectionRow(hosted, answer, connectionModelFlags(answer));
    expect(target.stale).toBe(true);
    expect(target.selectable).toBe(false);
    expect(target.modelControl).toBe("hidden");
  });

  it("marks plan-backed rows so the manage-usage link renders", () => {
    const answer = list({ connections: [row({ kind: "codex", uses_chatgpt_plan: true, is_selected: false })] });
    const target = toConnectionRow(answer.connections[0]!, answer, connectionModelFlags(answer));
    expect(target.usesChatgptPlan).toBe(true);
    expect(target.markers).toEqual(["ChatGPT plan linked"]);
  });

  it("marks a plan-backed row on the active lane separately", () => {
    const answer = list({ connections: [row({ kind: "codex", uses_chatgpt_plan: true, is_selected: true })] });
    const target = toConnectionRow(answer.connections[0]!, answer, connectionModelFlags(answer));
    expect(target.markers).toContain("ChatGPT plan in use");
    expect(target.markers).toContain("In use");
  });

  it("marks the included hosted lane separately from the active row", () => {
    const answer = list({
      connections: [row({ kind: "openhub", base_url: "", is_selected: false })],
      openhub_model_options: ["moonshotai/kimi-k3"],
    });
    const target = toConnectionRow(answer.connections[0]!, answer, connectionModelFlags(answer));
    expect(target.markers).toEqual(["Included in your plan"]);
  });
});

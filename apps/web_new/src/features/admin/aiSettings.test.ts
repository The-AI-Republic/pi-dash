// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI settings logic (ADM-046, ADM-047): the pinned keys, link
// targets, and success wording, the row selector, and the save
// payload builder. The save mutation itself is covered through the
// page suite, which drives it end to end.
import { describe, expect, it } from "vitest";

import {
  AI_KEY_DOCS_URL,
  AI_MODEL_DOCS_URL,
  AI_SETTINGS_SAVED_MESSAGE,
  AI_SETTINGS_SAVED_TITLE,
  AI_VENDOR_CONTACT_URL,
  buildAiSettingsPatch,
  LLM_API_KEY,
  LLM_MODEL_KEY,
  selectAiSettings,
} from "./aiSettings.js";

describe("AI settings pins", () => {
  it("names the configuration keys the backend reads", () => {
    expect(LLM_MODEL_KEY).toBe("LLM_MODEL");
    expect(LLM_API_KEY).toBe("LLM_API_KEY");
  });

  it("points the helper links at the vendor docs", () => {
    expect(AI_MODEL_DOCS_URL).toContain("platform.openai.com/docs/models");
    expect(AI_KEY_DOCS_URL).toContain("platform.openai.com/api-keys");
  });

  it("points the callout at the company contact page", () => {
    expect(AI_VENDOR_CONTACT_URL).toContain("airepublic.com/contact");
  });

  it("pins the success notice wording", () => {
    expect(AI_SETTINGS_SAVED_TITLE).toBe("Success");
    expect(AI_SETTINGS_SAVED_MESSAGE).toContain("AI Settings updated successfully");
  });
});

describe("selectAiSettings", () => {
  function row(key: string, value: string | null) {
    return {
      id: "11111111-1111-4111-8111-111111111111",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
      created_by: null,
      updated_by: null,
      deleted_at: null,
      key,
      value,
      category: "ai",
      is_encrypted: false,
      source: "db",
      is_managed: true,
    } as const;
  }

  it("reads the model and key out of the rows", () => {
    expect(selectAiSettings([row("LLM_MODEL", "gpt-4o"), row("LLM_API_KEY", "sk-live"), row("OTHER", "x")])).toEqual({
      model: "gpt-4o",
      apiKey: "sk-live",
    });
  });

  it("reads missing keys as empty", () => {
    expect(selectAiSettings([row("OTHER", "x")])).toEqual({ model: "", apiKey: "" });
  });

  it("reads null values as empty", () => {
    expect(selectAiSettings([row("LLM_MODEL", null), row("LLM_API_KEY", null)])).toEqual({
      model: "",
      apiKey: "",
    });
  });

  it("reads absent rows as empty", () => {
    expect(selectAiSettings(undefined)).toEqual({ model: "", apiKey: "" });
  });
});

describe("buildAiSettingsPatch", () => {
  it("carries both keys", () => {
    expect(buildAiSettingsPatch("gpt-4o", "sk-live")).toEqual({ LLM_MODEL: "gpt-4o", LLM_API_KEY: "sk-live" });
  });

  it("carries empty strings so an empty save clears the values", () => {
    expect(buildAiSettingsPatch("", "")).toEqual({ LLM_MODEL: "", LLM_API_KEY: "" });
  });
});

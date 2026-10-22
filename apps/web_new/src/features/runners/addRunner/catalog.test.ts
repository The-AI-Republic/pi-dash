// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Agent/model catalog (RUN-009, RUN-049). Every agent offers a list, Claude
// Code pins Opus 4.8 and now lists Opus 5.5 above Opus 5, Codex crosses its
// tiers with effort, and an unknown selection degrades to the agent default.
import { describe, expect, it } from "vitest";

import {
  AGENT_LABELS,
  DEFAULT_MODEL_ID,
  DEFAULT_MODEL_LABEL,
  firstSupportedAgent,
  MODEL_CHOICES,
  modelContribution,
  PRESELECTED_MODEL,
  RUNNER_AGENTS,
  selectedModelLabel,
} from "./catalog.js";

describe("agent list", () => {
  it("offers the six CLIs in display order", () => {
    expect(RUNNER_AGENTS).toEqual(["claude-code", "codex", "cursor-agent", "open-claw", "grok", "muse-code"]);
    expect(RUNNER_AGENTS.map((agent) => AGENT_LABELS[agent])).toEqual([
      "Claude Code",
      "Codex",
      "Cursor",
      "OpenClaw",
      "Grok",
      "Muse Code",
    ]);
  });
});

describe("Claude Code catalog (RUN-049)", () => {
  it("lists Opus 5.5 directly above Opus 5 and keeps Opus 4.8 as the default", () => {
    const labels = MODEL_CHOICES["claude-code"].map((choice) => choice.label);
    expect(labels.indexOf("Opus 5.5")).toBe(labels.indexOf("Opus 5") - 1);
    expect(PRESELECTED_MODEL["claude-code"]).toBe("claude-opus-4-8");
    expect(selectedModelLabel("claude-code", PRESELECTED_MODEL["claude-code"])).toBe("Opus 4.8");
  });

  it("emits claude-opus-5-5 for the new entry", () => {
    expect(modelContribution("claude-code", "claude-opus-5-5")).toEqual({ model: "claude-opus-5-5" });
  });

  it("leaves Cursor's list untouched by the new Claude entry", () => {
    const cursor = MODEL_CHOICES["cursor-agent"];
    expect(cursor.some((choice) => choice.id === "claude-opus-5-5")).toBe(false);
    expect(cursor.length).toBe(13);
  });
});

describe("model lists", () => {
  it("starts every list with the default sentinel", () => {
    for (const agent of RUNNER_AGENTS) {
      expect(MODEL_CHOICES[agent][0]).toEqual({ id: DEFAULT_MODEL_ID, label: DEFAULT_MODEL_LABEL });
    }
  });

  it("crosses Codex tiers with effort tiers", () => {
    expect(MODEL_CHOICES.codex.length).toBe(29);
    expect(modelContribution("codex", "gpt-5.6-sol:high")).toEqual({
      model: "gpt-5.6-sol",
      reasoningEffort: "high",
    });
  });

  it("offers only the sentinel for agents without a curated list", () => {
    expect(MODEL_CHOICES["open-claw"]).toHaveLength(1);
    expect(MODEL_CHOICES["muse-code"]).toHaveLength(1);
  });

  it("emits no flags for the default sentinel or an unknown id", () => {
    expect(modelContribution("claude-code", DEFAULT_MODEL_ID)).toEqual({});
    expect(modelContribution("claude-code", "not-a-model")).toEqual({});
    expect(modelContribution("codex", "gpt-5.6-sol:xhigh")).toEqual({
      model: "gpt-5.6-sol",
      reasoningEffort: "xhigh",
    });
  });

  it("labels unknown selections as the agent default", () => {
    expect(selectedModelLabel("grok", "nope")).toBe(DEFAULT_MODEL_LABEL);
  });
});

describe("machine gating", () => {
  it("picks the first supported agent, or null when none is known", () => {
    expect(firstSupportedAgent(undefined)).toBe("claude-code");
    expect(firstSupportedAgent([])).toBe("claude-code");
    expect(firstSupportedAgent(["grok", "codex"])).toBe("codex");
    expect(firstSupportedAgent(["unknown-agent"])).toBeNull();
  });
});

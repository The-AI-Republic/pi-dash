/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Per-agent LLM model catalogs for the "Add runner" form. Selecting an
 * option appends `--model` (and, for Codex, `--reasoning-effort`) to the
 * generated `pidash runner add` command; the runner persists it as the
 * agent's `model_default` in its local config.toml.
 *
 * The runner itself does NOT validate against these exact strings — it only
 * checks that a model is *applicable* to the chosen agent (see
 * runner/src/cli/runner_ops.rs::model_applies_to_agent) and otherwise falls
 * back to the agent's built-in default. So a stale entry here downgrades to
 * the agent default rather than failing enrollment.
 */

// Mirrors the runner CLI's ``--agent`` value-enum (kebab-case).
export type TRunnerAgent = "claude-code" | "codex" | "cursor-agent" | "open-claw";

export interface IRunnerModelOption {
  /** Unique select value within an agent's list. */
  id: string;
  /** Human-facing label shown in the dropdown. */
  label: string;
  /** `--model` value. Omitted for the "default" sentinel. */
  model?: string;
  /** `--reasoning-effort` value. Codex only. */
  reasoningEffort?: string;
}

/** Sentinel id for "use the agent's built-in default" (omits `--model`). */
export const DEFAULT_MODEL_ID = "default";

const DEFAULT_OPTION: IRunnerModelOption = {
  id: DEFAULT_MODEL_ID,
  label: "Default (agent's built-in model)",
};

// Codex: model × reasoning effort. The effort tier is sent on `turn/start`;
// `xhigh` is Codex's "extra high" tier.
const CODEX_MODELS: Array<{ label: string; model: string }> = [
  { label: "GPT-5.5", model: "gpt-5.5" },
  { label: "GPT-5.4", model: "gpt-5.4" },
  { label: "GPT-5.4 Mini", model: "gpt-5.4-mini" },
];
const CODEX_EFFORTS: Array<{ label: string; value: string }> = [
  { label: "Low", value: "low" },
  { label: "Medium", value: "medium" },
  { label: "High", value: "high" },
  { label: "Extra High", value: "xhigh" },
];
const CODEX_OPTIONS: IRunnerModelOption[] = CODEX_MODELS.flatMap((m) =>
  CODEX_EFFORTS.map((e) => ({
    id: `${m.model}:${e.value}`,
    label: `${m.label} (${e.label})`,
    model: m.model,
    reasoningEffort: e.value,
  }))
);

// Claude Code: model ids are Anthropic slugs; `[1m]` selects the 1M-context
// variant.
const CLAUDE_OPTIONS: IRunnerModelOption[] = [
  { id: "claude-opus-4-8", label: "Opus 4.8", model: "claude-opus-4-8" },
  { id: "claude-fable-5", label: "Fable 5", model: "claude-fable-5" },
  { id: "claude-sonnet-4-6", label: "Sonnet 4.6", model: "claude-sonnet-4-6" },
  { id: "claude-sonnet-4-6-1m", label: "Sonnet 4.6 (1M context)", model: "claude-sonnet-4-6[1m]" },
];

// Cursor: a curated subset of `cursor-agent models`. Cursor bakes the
// reasoning tier into the slug itself, so there is no separate effort flag.
// Update this list as Cursor's catalog changes (run `cursor-agent models`).
const CURSOR_OPTIONS: IRunnerModelOption[] = [
  { id: "auto", label: "Auto", model: "auto" },
  { id: "composer-2.5", label: "Composer 2.5", model: "composer-2.5" },
  { id: "gpt-5.5-high", label: "GPT-5.5 (High)", model: "gpt-5.5-high" },
  { id: "gpt-5.5-extra-high", label: "GPT-5.5 (Extra High)", model: "gpt-5.5-extra-high" },
  { id: "gpt-5.4-medium", label: "GPT-5.4", model: "gpt-5.4-medium" },
  { id: "gpt-5.4-mini-medium", label: "GPT-5.4 Mini", model: "gpt-5.4-mini-medium" },
  { id: "claude-opus-4-8-high", label: "Claude Opus 4.8", model: "claude-opus-4-8-high" },
  {
    id: "claude-opus-4-8-thinking-high",
    label: "Claude Opus 4.8 (Thinking)",
    model: "claude-opus-4-8-thinking-high",
  },
  { id: "claude-4.6-sonnet-medium", label: "Claude Sonnet 4.6", model: "claude-4.6-sonnet-medium" },
  {
    id: "claude-4.6-sonnet-medium-thinking",
    label: "Claude Sonnet 4.6 (Thinking)",
    model: "claude-4.6-sonnet-medium-thinking",
  },
  { id: "gemini-3.1-pro", label: "Gemini 3.1 Pro", model: "gemini-3.1-pro" },
  { id: "grok-4.3", label: "Grok 4.3", model: "grok-4.3" },
];

export const RUNNER_MODEL_OPTIONS: Record<TRunnerAgent, IRunnerModelOption[]> = {
  "claude-code": [DEFAULT_OPTION, ...CLAUDE_OPTIONS],
  codex: [DEFAULT_OPTION, ...CODEX_OPTIONS],
  "cursor-agent": [DEFAULT_OPTION, ...CURSOR_OPTIONS],
  "open-claw": [DEFAULT_OPTION],
};

/**
 * Pre-selected model option id per agent in the "Add runner" form. All agents
 * fall back to the agent's own built-in model (the ``"default"`` sentinel).
 * The id must exist in that agent's `RUNNER_MODEL_OPTIONS` list.
 */
export const DEFAULT_MODEL_BY_AGENT: Record<TRunnerAgent, string> = {
  "claude-code": DEFAULT_MODEL_ID,
  codex: DEFAULT_MODEL_ID,
  "cursor-agent": DEFAULT_MODEL_ID,
  "open-claw": DEFAULT_MODEL_ID,
};

/** Look up a selected option's label; falls back to the default label. */
export function runnerModelLabel(agent: TRunnerAgent, id: string): string {
  const opt = RUNNER_MODEL_OPTIONS[agent].find((o) => o.id === id);
  return opt?.label ?? DEFAULT_OPTION.label;
}

/**
 * Resolve a selected option id to the CLI args it contributes. Returns an
 * empty object for the "default" sentinel (so no `--model` is emitted).
 */
export function resolveRunnerModel(agent: TRunnerAgent, id: string): { model?: string; reasoningEffort?: string } {
  const opt = RUNNER_MODEL_OPTIONS[agent].find((o) => o.id === id);
  if (!opt) return {};
  return { model: opt.model, reasoningEffort: opt.reasoningEffort };
}

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Agent and model catalog for the add-runner form (RUN-009, RUN-049). The
// list is advisory: the runner accepts any model slug and falls back to the
// agent's own default, so unknown entries degrade instead of failing. The
// Claude Code default is pinned so the generated command never inherits a
// machine-wide global.

/** Agent identifiers accepted on the runner CLI's `--agent` flag. */
export const RUNNER_AGENTS = ["claude-code", "codex", "cursor-agent", "open-claw", "grok", "muse-code"] as const;
export type RunnerAgent = (typeof RUNNER_AGENTS)[number];

/** Human labels, in picker order. */
export const AGENT_LABELS: Record<RunnerAgent, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  "cursor-agent": "Cursor",
  "open-claw": "OpenClaw",
  grok: "Grok",
  "muse-code": "Muse Code",
};

/** Sentinel id meaning "let the agent use its own default" (omits `--model`). */
export const DEFAULT_MODEL_ID = "default";
export const DEFAULT_MODEL_LABEL = "Default (agent's built-in model)";

export interface ModelChoice {
  /** Picker value, unique within the agent's list. */
  id: string;
  /** Human label. */
  label: string;
  /** Value for `--model`; the sentinel omits it. */
  model?: string;
  /** Value for `--reasoning-effort` (Codex only). */
  reasoningEffort?: string;
}

const AGENT_DEFAULT: ModelChoice = { id: DEFAULT_MODEL_ID, label: DEFAULT_MODEL_LABEL };

const CLAUDE_MODELS: ModelChoice[] = [
  { id: "claude-fable-5", label: "Fable 5", model: "claude-fable-5" },
  // RUN-049: Opus 5.5 sits above Opus 5; Opus 4.8 stays the default.
  { id: "claude-opus-5-5", label: "Opus 5.5", model: "claude-opus-5-5" },
  { id: "claude-opus-5", label: "Opus 5", model: "claude-opus-5" },
  { id: "claude-opus-4-8", label: "Opus 4.8", model: "claude-opus-4-8" },
  { id: "claude-sonnet-4-6", label: "Sonnet 4.6", model: "claude-sonnet-4-6" },
  { id: "claude-sonnet-4-6-1m", label: "Sonnet 4.6 (1M context)", model: "claude-sonnet-4-6[1m]" },
];

/** Codex tiers crossed with its effort dimension. */
const CODEX_BASE: Array<{ label: string; model: string }> = [
  { label: "GPT-6 Astra", model: "gpt-6-astra" },
  { label: "GPT-5.6 Sol", model: "gpt-5.6-sol" },
  { label: "GPT-5.6 Terra", model: "gpt-5.6-terra" },
  { label: "GPT-5.6 Luna", model: "gpt-5.6-luna" },
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

const CODEX_MODELS: ModelChoice[] = CODEX_BASE.flatMap((tier) =>
  CODEX_EFFORTS.map((effort) => ({
    id: `${tier.model}:${effort.value}`,
    label: `${tier.label} (${effort.label})`,
    model: tier.model,
    reasoningEffort: effort.value,
  }))
);

/** Cursor folds the reasoning tier into the slug, so there is no effort flag. */
const CURSOR_MODELS: ModelChoice[] = [
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

const GROK_MODELS: ModelChoice[] = [
  { id: "grok-4.3", label: "Grok 4.3", model: "grok-4.3" },
  { id: "grok-code-fast-1", label: "Grok Code Fast 1", model: "grok-code-fast-1" },
];

/** Model choices per agent, sentinel first. */
export const MODEL_CHOICES: Record<RunnerAgent, ModelChoice[]> = {
  "claude-code": [AGENT_DEFAULT, ...CLAUDE_MODELS],
  codex: [AGENT_DEFAULT, ...CODEX_MODELS],
  "cursor-agent": [AGENT_DEFAULT, ...CURSOR_MODELS],
  "open-claw": [AGENT_DEFAULT],
  grok: [AGENT_DEFAULT, ...GROK_MODELS],
  "muse-code": [AGENT_DEFAULT],
};

/**
 * Preselected model per agent. Claude Code pins Opus 4.8 so a runner created
 * from the form never inherits a stale Claude global; the rest defer to the
 * agent.
 */
export const PRESELECTED_MODEL: Record<RunnerAgent, string> = {
  "claude-code": "claude-opus-4-8",
  codex: DEFAULT_MODEL_ID,
  "cursor-agent": DEFAULT_MODEL_ID,
  "open-claw": DEFAULT_MODEL_ID,
  grok: DEFAULT_MODEL_ID,
  "muse-code": DEFAULT_MODEL_ID,
};

/** Label for a selection; unknown ids read as the agent default. */
export function selectedModelLabel(agent: RunnerAgent, id: string): string {
  return MODEL_CHOICES[agent].find((choice) => choice.id === id)?.label ?? DEFAULT_MODEL_LABEL;
}

/** The CLI contribution of a selection; the sentinel and unknown ids emit nothing. */
export function modelContribution(agent: RunnerAgent, id: string): { model?: string; reasoningEffort?: string } {
  const choice = MODEL_CHOICES[agent].find((candidate) => candidate.id === id);
  if (choice?.model === undefined) return {};
  return {
    model: choice.model,
    ...(choice.reasoningEffort === undefined ? {} : { reasoningEffort: choice.reasoningEffort }),
  };
}

/** Effort display labels (the `:<effort>` id suffixes stay machine values). */
export const EFFORT_LABELS: Record<string, string> = {
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra High",
};

/** One catalog row as the add-runner form reads it. */
export interface CatalogRow {
  model: string;
  label: string;
  reasoning_efforts: string[];
  sort_order: number;
  is_preselected: boolean;
}

/**
 * Expand catalog rows into picker choices for one agent: the Default
 * sentinel first, then one choice per row (one per effort for rows
 * offering efforts, with the effort label appended). Rows sort by
 * (sort_order, model) — the server already orders them, this keeps the
 * picker deterministic if the cache is used directly.
 */
export function choicesForAgent(rows: readonly CatalogRow[]): ModelChoice[] {
  const ordered = [...rows].sort((a, b) => a.sort_order - b.sort_order || a.model.localeCompare(b.model));
  const choices: ModelChoice[] = [AGENT_DEFAULT];
  for (const entry of ordered) {
    if (entry.reasoning_efforts.length === 0) {
      choices.push({ id: entry.model, label: entry.label, model: entry.model });
    } else {
      for (const effort of entry.reasoning_efforts) {
        choices.push({
          id: `${entry.model}:${effort}`,
          label: `${entry.label} (${EFFORT_LABELS[effort] ?? effort})`,
          model: entry.model,
          reasoningEffort: effort,
        });
      }
    }
  }
  return choices;
}

/**
 * The preselected choice id for one agent's catalog rows: the first
 * expanded choice of the preselected row (the first effort when the
 * row offers efforts), or the Default sentinel when none is marked.
 */
export function preselectedId(rows: readonly CatalogRow[]): string {
  const marked = rows.find((entry) => entry.is_preselected);
  if (marked === undefined) return DEFAULT_MODEL_ID;
  if (marked.reasoning_efforts.length === 0) return marked.model;
  const firstEffort = marked.reasoning_efforts[0];
  return firstEffort === undefined ? DEFAULT_MODEL_ID : `${marked.model}:${firstEffort}`;
}

/** Label for a selection against resolved choices; unknown ids read as the default. */
export function selectedModelLabelFrom(choices: readonly ModelChoice[], id: string): string {
  return choices.find((choice) => choice.id === id)?.label ?? DEFAULT_MODEL_LABEL;
}

/** The CLI contribution of a selection against resolved choices. */
export function modelContributionFrom(
  choices: readonly ModelChoice[],
  id: string
): { model?: string; reasoningEffort?: string } {
  const choice = choices.find((candidate) => candidate.id === id);
  if (choice?.model === undefined) return {};
  return {
    model: choice.model,
    ...(choice.reasoningEffort === undefined ? {} : { reasoningEffort: choice.reasoningEffort }),
  };
}

/** The first agent a machine can run, or null when it advertises none we know. */
export function firstSupportedAgent(supported: readonly string[] | undefined): RunnerAgent | null {
  if (supported === undefined || supported.length === 0) return RUNNER_AGENTS[0];
  return RUNNER_AGENTS.find((agent) => supported.includes(agent)) ?? null;
}

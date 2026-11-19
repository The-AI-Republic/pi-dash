// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner edit rules. A runner's name lives on the server; its working
// directory, agent and model live in the daemon's config on its machine,
// so the form reads them from what the daemon last reported and writes them
// back through the machine (reconfigure). The project is fixed: a runner
// serving another project is a new runner. Plain functions keep the seed,
// the changed subset and the gating out of the component.

import type { Runner, RunnerReconfigure } from "@pidash/api-client";

import { isPidashAgentRunner } from "../../../shared/lib/runnerIdentity.js";
import { DEFAULT_MODEL_ID, RUNNER_AGENTS, type ModelChoice, type RunnerAgent } from "../addRunner/catalog.js";
import { NAME_SHAPE } from "../addRunner/model.js";

export const EDIT_RUNNER_TITLE = "Edit runner";
export const EDIT_RUNNER_HINT =
  "Rename the runner or change what it runs with on its machine. Its project cannot change; add a new runner to serve another project.";
export const EDIT_RUNNER_SUBMIT = "Save changes";
export const EDIT_RUNNER_SAVING = "Applying on the machine…";
export const EDIT_RUNNER_DONE_TITLE = "Runner updated";
export const EDIT_RUNNER_TIMEOUT =
  "The machine did not confirm the change in time. Check the runner's settings in a moment; if it keeps happening, update pidash on that machine.";
export const EDIT_RUNNER_OFFLINE =
  "This runner's machine is offline. Start its daemon to change the working directory, agent or model.";
export const EDIT_RUNNER_BUSY =
  "This runner has a run in progress. Wait for it to finish, or cancel it, to change the working directory, agent or model.";
export const EDIT_RUNNER_MODEL_UNREPORTED =
  "This runner's daemon does not report its model. Update pidash on that machine to see it here.";
export const EDIT_RUNNER_AGENT_UNREPORTED =
  "This runner's daemon does not report its agent. Update pidash on that machine to change the working directory, agent or model.";
export const EDIT_RUNNER_AGENT_HINT =
  "Switching agent needs that agent's CLI installed on the machine; the runner restarts with it.";

/** Poll cadence and budget while the machine applies a change. */
export const RECONFIGURE_POLL_MS = 2_000;
export const RECONFIGURE_TIMEOUT_MS = 60_000;

/** What the form edits. `modelId` is a picker id from the agent's choices. */
export interface RunnerEditValues {
  name: string;
  workingDir: string;
  agent: RunnerAgent;
  modelId: string;
}

/** The agent a runner reports through its `agent:<kind>` capability. */
export function runnerAgentOf(runner: Pick<Runner, "capabilities">): RunnerAgent | null {
  for (const capability of runner.capabilities) {
    if (!capability.startsWith("agent:")) continue;
    const kind = capability.slice("agent:".length).trim().replaceAll("_", "-");
    const agent = RUNNER_AGENTS.find((candidate) => candidate === kind);
    if (agent !== undefined) return agent;
  }
  return null;
}

/** Whether the runner's daemon reports its configured model at all. */
export function runnerReportsModel(runner: Pick<Runner, "dev_metadata">): boolean {
  return "model" in runner.dev_metadata || "reasoning_effort" in runner.dev_metadata;
}

/** The picker id of a reported model, or the default sentinel. */
export function modelIdFor(
  choices: readonly ModelChoice[],
  model: string | undefined,
  effort: string | undefined
): string {
  if (model === undefined || model === "") return DEFAULT_MODEL_ID;
  const exact = choices.find((choice) => choice.model === model && (choice.reasoningEffort ?? "") === (effort ?? ""));
  if (exact !== undefined) return exact.id;
  return customModelId(model, effort);
}

/** Picker id for a model the catalog does not list (kept so it can be shown). */
export function customModelId(model: string, effort: string | undefined): string {
  return effort === undefined || effort === "" ? `custom:${model}` : `custom:${model}:${effort}`;
}

/**
 * The agent's choices plus the runner's current model when the catalog no
 * longer lists it, so opening the form never silently changes the model.
 */
export function choicesWithCurrent(
  choices: readonly ModelChoice[],
  model: string | undefined,
  effort: string | undefined
): ModelChoice[] {
  if (model === undefined || model === "") return [...choices];
  const id = modelIdFor(choices, model, effort);
  if (choices.some((choice) => choice.id === id)) return [...choices];
  const label = effort === undefined || effort === "" ? `${model} (current)` : `${model} (${effort}, current)`;
  return [
    ...choices,
    {
      id,
      label,
      model,
      ...(effort === undefined || effort === "" ? {} : { reasoningEffort: effort }),
    },
  ];
}

/** The form's starting values, from the runner row. */
export function seedRunnerEdit(runner: Runner, choices: readonly ModelChoice[]): RunnerEditValues {
  return {
    name: runner.name,
    workingDir: runner.dev_metadata.working_dir ?? "",
    agent: runnerAgentOf(runner) ?? RUNNER_AGENTS[0],
    modelId: modelIdFor(choices, runner.dev_metadata.model, runner.dev_metadata.reasoning_effort),
  };
}

/** A name the server accepts, or the reason it does not. */
export function validateRunnerName(raw: string): { value: string } | { error: string } {
  const value = raw.trim();
  if (value === "") return { error: "Name cannot be empty." };
  if (value.length > 128) return { error: "Use at most 128 characters." };
  if (!NAME_SHAPE.test(value))
    return {
      error: "Letters, digits, underscore, dot and dash are allowed; it must begin with a letter, digit or underscore.",
    };
  return { value };
}

/** What a save sends: a server rename and/or a machine reconfigure. */
export interface RunnerEditPlan {
  name: string | null;
  reconfigure: RunnerReconfigure | null;
}

/**
 * The changed subset. A new agent always carries its model choice (the old
 * agent's model would not apply); a new model carries its effort, so moving
 * off a Codex effort clears it.
 */
export function runnerEditPlan(
  seed: RunnerEditValues,
  draft: RunnerEditValues,
  choices: readonly ModelChoice[]
): RunnerEditPlan {
  const name = draft.name.trim() === seed.name ? null : draft.name.trim();
  const reconfigure: RunnerReconfigure = {};
  const workingDir = draft.workingDir.trim();
  if (workingDir !== "" && workingDir !== seed.workingDir) reconfigure.working_dir = workingDir;
  const agentChanged = draft.agent !== seed.agent;
  if (agentChanged) reconfigure.agent = draft.agent;
  if (agentChanged || draft.modelId !== seed.modelId) {
    const choice = choices.find((candidate) => candidate.id === draft.modelId);
    reconfigure.model = choice?.model ?? "";
    reconfigure.reasoning_effort = choice?.reasoningEffort ?? "";
  }
  return { name, reconfigure: Object.keys(reconfigure).length === 0 ? null : reconfigure };
}

/** Whether the row offers Edit at all (design: engines take their settings from Pi Dash). */
export function canEditRunner(
  runner: Pick<Runner, "provisioning" | "revoked_at" | "status" | "owner">,
  userId: string | null
): boolean {
  if (userId === null || runner.owner !== userId) return false;
  if (isPidashAgentRunner(runner)) return false;
  return runner.revoked_at === null && runner.status !== "revoked";
}

/**
 * Why the machine-side fields are locked right now, or null when they can
 * change. The name stays editable either way (it lives on the server).
 */
export function machineFieldsBlocker(runner: Pick<Runner, "status" | "dev_machine" | "capabilities">): string | null {
  if (runner.dev_machine === null)
    return "This runner was enrolled without a dev machine, so Pi Dash cannot reach it to change its settings.";
  if (runner.status === "offline") return EDIT_RUNNER_OFFLINE;
  if (runner.status === "busy") return EDIT_RUNNER_BUSY;
  // Without the agent the form would seed a guess and send models for it.
  if (runnerAgentOf(runner) === null) return EDIT_RUNNER_AGENT_UNREPORTED;
  return null;
}

/** Text for a refused or failed reconfigure. */
export function reconfigureErrorText(reason: string | undefined): string {
  switch (reason) {
    case "machine_offline":
      return EDIT_RUNNER_OFFLINE;
    case "runner_busy":
      return EDIT_RUNNER_BUSY;
    case "runner_revoked":
      return "This runner is revoked.";
    case "no_dev_machine":
      return "This runner was enrolled without a dev machine, so Pi Dash cannot reach it to change its settings.";
    case "pidash_agent_not_editable":
      return "The Pi Dash Agent's settings come from Pi Dash.";
    case "delivery_failed":
      return "Pi Dash could not deliver the change to the machine. Try again.";
    case undefined:
    case "":
      return "The machine could not apply the change. Try again.";
    default:
      // Unknown message types are dropped by daemons too old to edit runners.
      return reason;
  }
}

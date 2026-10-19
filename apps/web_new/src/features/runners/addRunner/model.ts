// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add-runner form rules (RUN-006, RUN-007, RUN-009). Plain functions keep the
// cascade, validation, gating and remote-create state machine out of the
// component so every branch is unit-tested. The form holds the project
// identifier (not its uuid) because both the CLI flag and the create payload
// take the slug.

import type { Pod, Project, ProjectLite } from "@pidash/api-client";

import { firstSupportedAgent, PRESELECTED_MODEL, RUNNER_AGENTS, type RunnerAgent } from "./catalog.js";

/** Picker value meaning "do not target a machine; show the command". */
export const MANUAL_MACHINE = "";
/** Label of that picker entry. */
export const MANUAL_MACHINE_LABEL = "Run `pidash runner add` manually";
/** Sea-level pod value; omitted from the command and the payload. */
export const DEFAULT_POD = "";
/** Label of the default-pod entry. */
export const DEFAULT_POD_LABEL = "(default pod)";

/** How long the form waits for the daemon's verdict (RUN-007). */
export const REMOTE_TIMEOUT_MS = 90_000;
/** Status read cadence during that window. */
export const REMOTE_POLL_MS = 2_000;

/** Machine list refresh cadence while the modal is open. */
export const MACHINE_REFRESH_MS = 15_000;

export interface AddRunnerValues {
  devMachineId: string;
  projectIdentifier: string;
  podName: string;
  name: string;
  workingDir: string;
  agent: RunnerAgent;
  model: string;
}

export function emptyValues(): AddRunnerValues {
  return {
    devMachineId: MANUAL_MACHINE,
    projectIdentifier: "",
    podName: DEFAULT_POD,
    name: "",
    workingDir: "",
    agent: "claude-code",
    model: PRESELECTED_MODEL["claude-code"],
  };
}

/** The name rule: blank auto-assigns; otherwise start alnum/underscore, then a bounded safe run. */
const NAME_WHITESPACE = /\s/;
const NAME_SHAPE = /^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$/;

export const NAME_RULE_TEXT =
  "Runner name cannot contain spaces. It must start with a letter, digit, or underscore and contain only letters, digits, underscore, dot, or dash.";
export const PROJECT_REQUIRED_TEXT = "Pick a project.";

/** Field errors the form can show. */
export interface AddRunnerErrors {
  projectIdentifier?: string;
  name?: string;
}

/** Trimmed name, rejecting blank and malformed input. */
export function validateValues(values: Pick<AddRunnerValues, "projectIdentifier" | "name">): AddRunnerErrors {
  const errors: AddRunnerErrors = {};
  if (values.projectIdentifier.trim() === "") errors.projectIdentifier = PROJECT_REQUIRED_TEXT;
  const name = values.name.trim();
  if (name !== "" && (NAME_WHITESPACE.test(name) || !NAME_SHAPE.test(name))) errors.name = NAME_RULE_TEXT;
  return errors;
}

/** A connected machine the picker can offer. */
export interface ConnectedMachine {
  id: string;
  label: string;
  supportedAgents?: readonly string[] | undefined;
}

/** Visible label for a machine row; falls back to its host label, then id. */
export function machineLabel(machine: { label: string; host_label: string; id: string }): string {
  return machine.label || machine.host_label || machine.id;
}

/**
 * The picker's connected machines: online and not revoked. Only these can
 * receive a pushed create command.
 */
export function connectedMachines<T extends { control_online: boolean; revoked_at: string | null }>(
  machines: readonly T[] | undefined
): T[] {
  return (machines ?? []).filter((machine) => machine.control_online && machine.revoked_at === null);
}

/**
 * One-shot auto-selection of the first connected machine. Returns the id to
 * select, or null when nothing should change (already touched, nothing
 * connected, or a selection is already set).
 */
export function autoSelectMachine(
  current: string,
  machines: readonly ConnectedMachine[],
  alreadyTouched: boolean
): string | null {
  if (alreadyTouched || machines.length === 0 || current !== MANUAL_MACHINE) return null;
  return machines[0]!.id;
}

/**
 * Keep the selection honest when a machine drops offline: a chosen machine
 * that is no longer connected snaps back to manual. Manual and still-connected
 * selections pass through unchanged.
 */
export function settleMachineSelection(current: string, machines: readonly ConnectedMachine[]): string {
  if (current === MANUAL_MACHINE) return MANUAL_MACHINE;
  return machines.some((machine) => machine.id === current) ? current : MANUAL_MACHINE;
}

/** The project a picked pod belongs to, resolved against the project list. */
export function projectForPod(
  podName: string,
  pods: readonly Pod[] | undefined,
  projects: readonly ProjectLite[] | Project[] | undefined
): string | null {
  if (podName === DEFAULT_POD) return null;
  const pod = pods?.find((row) => row.name === podName);
  if (pod === undefined || pod.project_identifier === "") return null;
  return projects?.some((project) => project.identifier === pod.project_identifier) ? pod.project_identifier : null;
}

/**
 * Pods the picker offers: every pod until a project is chosen (so a
 * pod-first selection is possible), then only that project's pods.
 */
export function podsForProject(pods: readonly Pod[] | undefined, projectIdentifier: string): Pod[] {
  const rows = pods ?? [];
  if (projectIdentifier === "") return [...rows];
  return rows.filter((pod) => pod.project_identifier === projectIdentifier);
}

/**
 * Pod value after a project change: the default sentinel always fits;
 * a pod from another project is cleared.
 */
export function settlePodSelection(
  podName: string,
  projectIdentifier: string,
  pods: readonly Pod[] | undefined
): string {
  if (podName === DEFAULT_POD) return DEFAULT_POD;
  return podsForProject(pods, projectIdentifier).some((pod) => pod.name === podName) ? podName : DEFAULT_POD;
}

/** Label a pod option carries in the picker. */
export function podOptionLabel(pod: Pod): string {
  return pod.project_identifier === "" ? pod.name : `${pod.name} (${pod.project_identifier})`;
}

/**
 * Agents the picker disables for the chosen machine, and whether the hint
 * shows. An absent machine (manual mode) or an empty advertised set gates
 * nothing — older daemons do not advertise their set.
 */
export function agentGating(machine: ConnectedMachine | undefined): { disabled: RunnerAgent[]; hint: string | null } {
  const supported = machine?.supportedAgents;
  if (machine === undefined || supported === undefined || supported.length === 0) {
    return { disabled: [], hint: null };
  }
  const disabled = RUNNER_AGENTS.filter((agent) => !supported.includes(agent));
  return {
    disabled,
    hint:
      disabled.length === 0
        ? null
        : "This machine can run only some of these agents. Update pidash on it to unlock the rest.",
  };
}

/**
 * Snap the agent after a machine change: a machine that cannot run the
 * current agent moves the selection to its first supported one. A machine
 * with no known supported agent keeps the current one (nothing to snap to).
 */
export function settleAgent(agent: RunnerAgent, machine: ConnectedMachine | undefined): RunnerAgent {
  if (machine === undefined) return agent;
  const supported = machine.supportedAgents;
  if (supported === undefined || supported.length === 0 || supported.includes(agent)) return agent;
  return firstSupportedAgent(supported) ?? agent;
}

/** Default model for an agent; also the reset target on every agent change. */
export function defaultModelFor(agent: RunnerAgent): string {
  return PRESELECTED_MODEL[agent];
}

/** Remote-create phase the modal renders (RUN-007). */
export type RemotePhase = "creating" | "ok" | "error" | "timeout";

export interface RemoteState {
  phase: RemotePhase;
  machineLabel: string;
  runnerName?: string;
  error?: string;
}

/**
 * Text for a failed create. The cloud's `unsupported_agent` rejection and the
 * daemon's raw `unknown agent kind "…"` write-back both become the daemon
 * upgrade hint; machine-offline is distinct from an unreachable cloud.
 */
export function remoteFailureText(code: string | undefined): string {
  if (code === "machine_offline") return "The dev machine went offline before the command could be delivered.";
  if (code === "unsupported_agent") return DAEMON_TOO_OLD_TEXT;
  if (code !== undefined && UNKNOWN_AGENT_PATTERN.test(code)) return DAEMON_TOO_OLD_TEXT;
  return code ?? "Could not reach the cloud to start the runner creation.";
}

export const DAEMON_TOO_OLD_TEXT =
  "The runner daemon on this machine is too old to support the selected agent. Upgrade pidash on the machine and try again.";

const UNKNOWN_AGENT_PATTERN = /unknown agent kind/i;

/** Whether a daemon error message is the unknown-agent write-back. */
export function isUnknownAgentError(message: string | undefined): boolean {
  return message !== undefined && UNKNOWN_AGENT_PATTERN.test(message);
}

/**
 * A status write-back that reports a failure. The daemon may answer the raw
 * unknown-agent text instead of a code, so both routes map to the hint.
 */
export function remoteStatusError(error: string | undefined): string {
  return isUnknownAgentError(error) ? DAEMON_TOO_OLD_TEXT : (error ?? "");
}

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dev-machines presentation model (NEWFRONT-555, RUN-037). The server sends
// raw counts and timestamps; the status a row shows is derived here so the
// table never invents a server field. A revoked machine outranks its runner
// counts; a machine with an online runner is active; a machine that only
// hosts offline runners is offline; the rest registered.

import type { DevMachine, Runner } from "@pidash/api-client";
import type { BadgeTone } from "@pidash/kit";

import { isEngineRunner } from "../../../shared/lib/runnerIdentity.js";

export const DEV_MACHINE_STATUSES = ["active", "offline", "registered", "revoked"] as const;
export type DevMachineStatus = (typeof DEV_MACHINE_STATUSES)[number];

export const DEV_MACHINE_STATUS_LABELS: Record<DevMachineStatus, string> = {
  active: "Active",
  offline: "Offline",
  registered: "Registered",
  revoked: "Revoked",
};

export const DEV_MACHINE_STATUS_TONES: Record<DevMachineStatus, BadgeTone> = {
  active: "success",
  offline: "neutral",
  registered: "accent",
  revoked: "warning",
};

/** Revocation wins; otherwise the online count, then any host, then none. */
export function devMachineStatus(
  machine: Pick<DevMachine, "revoked_at" | "online_runner_count" | "runner_count">
): DevMachineStatus {
  if (machine.revoked_at !== null) return "revoked";
  if (machine.online_runner_count > 0) return "active";
  if (machine.runner_count > 0) return "offline";
  return "registered";
}

/** First eight characters of the id, used when the machine has no name. */
export function shortMachineId(id: string): string {
  return id.slice(0, 8);
}

/** Label, then host label, then a short id — always something to click. */
export function devMachineName(machine: Pick<DevMachine, "label" | "host_label" | "id">): string {
  return machine.label !== ""
    ? machine.label
    : machine.host_label !== ""
      ? machine.host_label
      : shortMachineId(machine.id);
}

/** The second line: the host when it differs from the name, else the short id. */
export function devMachineSubline(machine: Pick<DevMachine, "label" | "host_label" | "id">): string {
  const name = devMachineName(machine);
  return machine.host_label !== "" && machine.host_label !== name
    ? machine.host_label
    : `id ${shortMachineId(machine.id)}`;
}

/** "N active / M total" — the counts sentence the table renders verbatim. */
export function runnerCountLabel(machine: Pick<DevMachine, "online_runner_count" | "runner_count">): string {
  return `${machine.online_runner_count} active / ${machine.runner_count} total`;
}

/**
 * A timestamp cell. The formatter is injectable so tests pin locale output;
 * null means the machine never reported, which reads as "Never".
 */
export function machineTime(
  value: string | null,
  format: (iso: string) => string = (iso) => new Date(iso).toLocaleString()
): string {
  return value === null ? "Never" : format(value);
}

export type MachineAction = "rotate" | "revoke" | "delete";

/** Rotate and revoke need live credentials; delete is always available. */
export function machineActions(machine: Pick<DevMachine, "revoked_at">): MachineAction[] {
  return machine.revoked_at === null ? ["rotate", "revoke", "delete"] : ["delete"];
}

export interface MachineActionCopy {
  title: string;
  body: string;
  confirm: string;
}

/** Confirmation copy per action; each names the consequence the server applies. */
export const MACHINE_ACTION_COPY: Record<MachineAction, MachineActionCopy> = {
  rotate: {
    title: "Rotate dev machine token?",
    body: "The current auth token for this dev machine is invalidated. Runners hosted here stop connecting until someone runs `pidash auth login` on the machine again.",
    confirm: "Rotate",
  },
  revoke: {
    title: "Revoke dev machine?",
    body: "This permanently revokes the dev machine, invalidates its auth token, and revokes runners hosted on it. The row stays with only delete available.",
    confirm: "Revoke",
  },
  delete: {
    title: "Remove this computer?",
    body: "This revokes both of the computer's credentials (Pi Dash and the built-in engine) and every runner on it, built-in engine included, and removes the computer from this list. Pi Dash on that computer shows as signed out. Nothing is uninstalled from the computer itself.",
    confirm: "Remove this computer",
  },
};

// --- Built-in engine state (built-in engine design §4.3, §4.11, §8) ---

/** Plain wording for the server's automatic engine pause reasons. */
export const ENGINE_AUTO_PAUSE_TEXT: Readonly<Record<string, string>> = {
  engine_sign_in_required: "sign-in required",
  gateway_session_revoked: "model gateway access was revoked",
  wallet_empty: "wallet is empty",
  profile_unavailable: "no model profile available",
  engine_disabled: "turned off on this server",
};

/** The engine state a machine row shows, or null when there is none to show. */
export interface EngineStateView {
  label: string;
  tone: BadgeTone;
}

/** True when the server reports any engine field for this machine. */
export function machineReportsEngine(
  machine: Pick<DevMachine, "engine_paused_by_owner" | "engine_auto_pause_reason" | "engine_sign_in_state">
): boolean {
  return (
    machine.engine_paused_by_owner !== undefined ||
    machine.engine_auto_pause_reason !== undefined ||
    machine.engine_sign_in_state !== undefined
  );
}

/**
 * The engine's state for one machine. The owner's pause wins, then the
 * server's automatic pause, then the sign-in state; a server that reports
 * no engine field at all yields null so the row shows nothing invented.
 */
export function engineState(
  machine: Pick<
    DevMachine,
    | "revoked_at"
    | "engine_paused_by_owner"
    | "engine_auto_pause_reason"
    | "engine_sign_in_state"
    | "engine_credential_expires_at"
  >,
  formatDate: (iso: string) => string = (iso) => new Date(iso).toLocaleDateString()
): EngineStateView | null {
  if (machine.revoked_at !== null || !machineReportsEngine(machine)) return null;
  if (machine.engine_paused_by_owner === true) return { label: "Engine paused by you", tone: "warning" };
  const reason = machine.engine_auto_pause_reason ?? "";
  if (reason !== "") {
    return { label: `Engine paused: ${ENGINE_AUTO_PAUSE_TEXT[reason] ?? reason}`, tone: "warning" };
  }
  switch (machine.engine_sign_in_state) {
    case "never":
      return { label: "Engine not signed in", tone: "neutral" };
    case "signed_out":
      return { label: "Engine signed out", tone: "neutral" };
    case "revoked":
      return { label: "Engine sign-in revoked", tone: "neutral" };
    case "expired":
      return { label: "Engine sign-in expired", tone: "warning" };
    case "expiring": {
      const at = machine.engine_credential_expires_at ?? null;
      return {
        label: at === null ? "Engine sign-in expiring" : `Engine expiring on ${formatDate(at)}`,
        tone: "warning",
      };
    }
    default:
      return { label: "Engine available", tone: "success" };
  }
}

/** Sign-in states with a live or renewable credential to revoke. */
const SIGNED_IN_STATES: ReadonlySet<string> = new Set(["active", "expiring", "expired"]);

/** Which engine and machine settings a row can offer. */
export interface MachineSettings {
  rename: boolean;
  pause: boolean;
  teamRuns: boolean;
  signOutEngine: boolean;
}

/**
 * Settings available for one machine. A revoked machine offers none; each
 * engine control needs its field on the payload, so an older server
 * simply hides it. Sign-out needs the sign-in state the newer server
 * reports, and a credential to revoke.
 */
export function machineSettings(
  machine: Pick<DevMachine, "revoked_at" | "engine_paused_by_owner" | "engine_team_runs" | "engine_sign_in_state">
): MachineSettings {
  const live = machine.revoked_at === null;
  return {
    rename: live,
    pause: live && machine.engine_paused_by_owner !== undefined,
    teamRuns: live && machine.engine_team_runs !== undefined,
    signOutEngine:
      live && machine.engine_sign_in_state !== undefined && SIGNED_IN_STATES.has(machine.engine_sign_in_state),
  };
}

type LivenessFields = Pick<DevMachine, "id" | "host_label" | "revoked_at" | "control_online" | "online_runner_count">;

/** A machine whose daemon is connected right now. */
export function machineIsLive(machine: LivenessFields): boolean {
  return machine.revoked_at === null && (machine.control_online || machine.online_runner_count > 0);
}

/**
 * The live machine that likely replaced this offline one after its data
 * was wiped (design §4.10): same hostname, another id, connected now.
 * Two computers can share a hostname, so this is only an offer the owner
 * confirms, never an automatic merge. Null when nothing qualifies.
 */
export function replacementCandidate<T extends LivenessFields & Pick<DevMachine, "merged_into">>(
  machine: T,
  machines: readonly T[]
): T | null {
  if (machine.revoked_at !== null || machineIsLive(machine) || (machine.merged_into ?? null) !== null) return null;
  if (machine.host_label.trim() === "") return null;
  return (
    machines.find(
      (other) => other.id !== machine.id && other.host_label === machine.host_label && machineIsLive(other)
    ) ?? null
  );
}

/** "Merged into <name>" for a merged machine, or null. */
export function mergedIntoText(
  machine: Pick<DevMachine, "merged_into">,
  machines: readonly Pick<DevMachine, "id" | "label" | "host_label">[]
): string | null {
  const target = machine.merged_into ?? null;
  if (target === null || target === "") return null;
  const row = machines.find((entry) => entry.id === target);
  return `Merged into ${row === undefined ? shortMachineId(target) : devMachineName(row)}`;
}

/** One machine's runners, split the way the expanded row lists them. */
export interface MachineRunners {
  engines: Runner[];
  ownAgents: Runner[];
}

/** Group a runner list under its machine, engines apart from own agents. */
export function runnersByMachine(runners: readonly Runner[]): Map<string, MachineRunners> {
  const out = new Map<string, MachineRunners>();
  for (const runner of runners) {
    if (runner.dev_machine === null) continue;
    let entry = out.get(runner.dev_machine);
    if (entry === undefined) {
      entry = { engines: [], ownAgents: [] };
      out.set(runner.dev_machine, entry);
    }
    (isEngineRunner(runner) ? entry.engines : entry.ownAgents).push(runner);
  }
  return out;
}

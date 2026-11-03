// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dev-machines presentation model (NEWFRONT-555, RUN-037). The server sends
// raw counts and timestamps; the status a row shows is derived here so the
// table never invents a server field. A revoked machine outranks its runner
// counts; a machine with an online runner is active; a machine that only
// hosts offline runners is offline; the rest registered.

import type { DevMachine } from "@pidash/api-client";
import type { BadgeTone } from "@pidash/kit";

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
    title: "Delete dev machine?",
    body: "The machine connection is removed: its auth token is invalidated, hosted runners are torn down, and the machine disappears from this list. Deleting a dev machine does not uninstall the AI agent (such as Codex or Claude) on the machine itself.",
    confirm: "Delete",
  },
};

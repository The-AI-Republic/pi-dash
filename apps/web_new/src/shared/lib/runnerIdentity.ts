// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// How a runner is named to people (built-in engine design §5, §11).
// A built-in engine runner has a server-assigned name nobody should
// read; it is shown as "Built-in engine · <machine>", so renaming the
// machine renames its engine everywhere. Own-agent runners keep their
// own name. Every surface that names a runner — lists, pickers, the
// runs page, the issue panel — reads these helpers so one runner never
// carries two names.

/** The machine identity fields a name is derived from. */
export interface MachineIdentity {
  id?: string;
  label: string;
  host_label: string;
}

/** The runner fields a display name is derived from. */
export interface RunnerIdentity {
  name: string;
  provisioning?: string | undefined;
  host_label?: string | undefined;
  dev_machine_detail?: MachineIdentity | null | undefined;
}

/** Stored provisioning value that marks a built-in engine runner. */
export const ENGINE_PROVISIONING = "desktop_bundled";

/** The product name of the bundled agent. */
export const BUILT_IN_ENGINE = "Built-in engine";

/** Short badge text shown beside engine runners. */
export const BUILT_IN_BADGE = "Built-in";

/** True for a built-in engine runner. */
export function isEngineRunner(runner: Pick<RunnerIdentity, "provisioning">): boolean {
  return runner.provisioning === ENGINE_PROVISIONING;
}

/**
 * A machine's name: the user's label, then the hostname, then a short
 * id, so there is always something to show.
 */
export function machineDisplayName(machine: MachineIdentity): string {
  if (machine.label.trim() !== "") return machine.label;
  if (machine.host_label.trim() !== "") return machine.host_label;
  return machine.id === undefined ? "Unknown computer" : machine.id.slice(0, 8);
}

/**
 * The machine a runner runs on, or null when the runner predates
 * machines and reports no hostname either.
 */
export function runnerMachineName(runner: RunnerIdentity): string | null {
  if (runner.dev_machine_detail !== null && runner.dev_machine_detail !== undefined) {
    return machineDisplayName(runner.dev_machine_detail);
  }
  const host = runner.host_label ?? "";
  return host.trim() === "" ? null : host;
}

/** "Built-in engine · <machine>", or the bare product name without one. */
export function engineLabel(machineName: string | null): string {
  return machineName === null ? BUILT_IN_ENGINE : `${BUILT_IN_ENGINE} · ${machineName}`;
}

/** The name a person sees for a runner. */
export function runnerDisplayName(runner: RunnerIdentity): string {
  return isEngineRunner(runner) ? engineLabel(runnerMachineName(runner)) : runner.name;
}

/** Plain wording for the server's automatic engine pause reasons (design §8). */
export const ENGINE_AUTO_PAUSE_TEXT: Readonly<Record<string, string>> = {
  engine_sign_in_required: "sign-in required",
  gateway_session_revoked: "model gateway access was revoked",
  wallet_empty: "wallet is empty",
  profile_unavailable: "no model profile available",
  engine_disabled: "turned off on this server",
};

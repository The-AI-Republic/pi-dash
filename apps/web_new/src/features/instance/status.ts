// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Startup-gate decision (parity/specs/workspace-settings.md §34-35,
// WSET-034/035). Pure so the gate and its tests share the branch order:
// a failed read is maintenance; a settled read with setup unfinished is
// the not-ready screen; otherwise the app renders. The gate never treats
// a pending read as maintenance, and a response carrying data wins over
// an error status — the spec requires both.

import type { InstanceBootstrap } from "@pidash/api-client";

export type AppInstanceBranch = "loading" | "maintenance" | "not-ready" | "ready";

export interface AppInstanceGateInput {
  readonly isPending: boolean;
  readonly isError: boolean;
  readonly data: InstanceBootstrap | undefined;
}

/** Exactly one startup branch for the single instance-info read. */
export function decideAppInstanceGate(input: AppInstanceGateInput): AppInstanceBranch {
  if (input.data !== undefined) {
    return input.data.instance.is_setup_done ? "ready" : "not-ready";
  }
  if (input.isError) return "maintenance";
  if (input.isPending) return "loading";
  return "maintenance";
}

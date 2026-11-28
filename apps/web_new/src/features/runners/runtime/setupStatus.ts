// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { DaemonSetUpStep } from "./service.js";

export type SetupStatus = { kind: "running"; step: DaemonSetUpStep } | { kind: "failed"; error: unknown } | null;
let status: SetupStatus = null;
const listeners = new Set<() => void>();
export function getSetupStatus(): SetupStatus {
  return status;
}
export function subscribeSetupStatus(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
export function publishSetupStatus(next: SetupStatus): void {
  status = next;
  for (const listener of listeners) listener();
}

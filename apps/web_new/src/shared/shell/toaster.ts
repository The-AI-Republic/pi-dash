// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Toasts (Architecture > shared/shell). One kit manager for the app;
// features notify through these helpers instead of owning managers.

import { createKitToastManager, showToast, type ToastHostManager } from "@pidash/kit";

let manager: ToastHostManager | null = null;

/** The manager the AppShell hands to the kit ToastHost. */
export function getToastManager(): ToastHostManager {
  if (!manager) {
    manager = createKitToastManager();
  }
  return manager;
}

export function notifySuccess(title: string, description?: string): void {
  showToast(getToastManager(), { title, ...(description === undefined ? {} : { description }) });
}

/** Success toast with one optional follow-up action. */
export interface ToastFollowUp {
  actionLabel: string;
  onAction: () => void;
}

export function notifySuccessWithAction(title: string, description: string | undefined, followUp: ToastFollowUp): void {
  showToast(getToastManager(), {
    title,
    ...(description === undefined ? {} : { description }),
    actions: [{ label: followUp.actionLabel, run: followUp.onAction }],
  });
}

export function notifyInfo(title: string, description?: string): void {
  showToast(getToastManager(), { title, ...(description === undefined ? {} : { description }) });
}

/**
 * A notice that stays on screen while a write flies and is then replaced in
 * place by its outcome (PSET-043/053/054's pending/success/error cycle).
 * Resolves `{ id, settle }`; calling `settle` twice rewrites the same toast.
 */
export function notifyPending(
  title: string,
  description?: string
): {
  id: string;
  settle: (outcome: { title: string; description?: string; failed?: boolean }) => void;
} {
  const manager = getToastManager();
  const id = manager.add({
    title,
    ...(description === undefined ? {} : { description }),
    timeout: 0,
  });
  return {
    id,
    settle: (outcome) => {
      manager.update(id, {
        title: outcome.title,
        ...(outcome.description === undefined ? {} : { description: outcome.description }),
        ...(outcome.failed === true ? { timeout: 8000 } : {}),
      });
    },
  };
}

export function notifyError(title: string, description?: string): void {
  showToast(getToastManager(), {
    title,
    ...(description === undefined ? {} : { description }),
    timeout: 8000,
  });
}

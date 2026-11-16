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
  showToast(getToastManager(), { title, ...(description === undefined ? {} : { description }), kind: "success" });
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
    kind: "success",
  });
}

export function notifyInfo(title: string, description?: string): void {
  showToast(getToastManager(), { title, ...(description === undefined ? {} : { description }), kind: "info" });
}

/** How long a settled notice stays up; a failure lingers longer. */
export const NOTICE_SUCCESS_TIMEOUT_MS = 5000;
export const NOTICE_FAILURE_TIMEOUT_MS = 8000;

export interface PendingNotice {
  /** The sticky toast currently on screen. */
  id: string;
  /**
   * Replace the sticky notice with its outcome. The outcome is a fresh,
   * timed toast: the kit's `update` carries a new `timeout` but does not
   * schedule it, so settling the old toast in place would leave it up
   * forever (PSET-043/053/054's pending/success/error cycle). Settling
   * twice swaps the latest outcome; the stale id then closes nothing.
   */
  settle: (outcome: { title: string; description?: string; failed?: boolean }) => void;
}

/**
 * A notice that stays on screen while a write flies and is then replaced by
 * its outcome. Success auto-dismisses on the kit's ordinary delay; a
 * failure lingers longer. Never leaves a success notice stuck on screen.
 */
export function notifyPending(title: string, description?: string): PendingNotice {
  const manager = getToastManager();
  let current = manager.add({
    title,
    ...(description === undefined ? {} : { description }),
    timeout: 0,
  });
  const notice: PendingNotice = {
    get id() {
      return current;
    },
    settle: (outcome) => {
      manager.close(current);
      current = manager.add({
        title: outcome.title,
        ...(outcome.description === undefined ? {} : { description: outcome.description }),
        timeout: outcome.failed === true ? NOTICE_FAILURE_TIMEOUT_MS : NOTICE_SUCCESS_TIMEOUT_MS,
        type: outcome.failed === true ? "error" : "success",
      });
    },
  };
  return notice;
}

export function notifyError(title: string, description?: string): void {
  showToast(getToastManager(), {
    title,
    ...(description === undefined ? {} : { description }),
    timeout: 8000,
    kind: "error",
  });
}

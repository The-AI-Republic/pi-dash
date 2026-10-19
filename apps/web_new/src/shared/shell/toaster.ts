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
    actionLabel: followUp.actionLabel,
    onAction: followUp.onAction,
  });
}

export function notifyInfo(title: string, description?: string): void {
  showToast(getToastManager(), { title, ...(description === undefined ? {} : { description }) });
}

export function notifyError(title: string, description?: string): void {
  showToast(getToastManager(), {
    title,
    ...(description === undefined ? {} : { description }),
    timeout: 8000,
  });
}

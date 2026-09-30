// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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

export function notifyError(title: string, description?: string): void {
  showToast(getToastManager(), {
    title,
    ...(description === undefined ? {} : { description }),
    timeout: 8000,
  });
}

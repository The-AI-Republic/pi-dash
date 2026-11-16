// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings dialog dismissal rule (parity/specs/workspace-settings.md §37,
// WSET-037). Escape and the backdrop close create/edit/confirm dialogs
// without a server write. The secret-reveal steps are the only dialogs
// that require an explicit close: the open state only changes from
// Cancel/Close, never from a dismissal reason the surface generates.

export type WorkspaceSettingsDialogDismissReason = "escape-key" | "outside-press" | "close-press" | "focus-out";

export interface WorkspaceSettingsDialogOpenChange {
  open: boolean;
  reason: string;
}

/**
 * Reduce a dialog open-change into the next open state. Closing through
 * Escape/backdrop/close always dismisses; a reveal dialog ignores every
 * dismissal the component generates so only its explicit action closes.
 */
export function nextWorkspaceSettingsDialogOpen(
  current: boolean,
  change: WorkspaceSettingsDialogOpenChange,
  options: { reveal: boolean }
): boolean {
  if (!change.open && options.reveal) return current;
  return change.open;
}

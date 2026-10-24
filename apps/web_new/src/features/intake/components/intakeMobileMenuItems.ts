// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Mobile action menu entries (parity/specs/intake.md INT-030). Pure:
// the narrow-viewport header's overflow items with the desktop gates.
// An open request carries the triage entries plus delete and the
// work-item link for the same viewers the desktop overflow serves; a
// guest outside those gates opens an empty menu. A resolved request
// carries only the link and open entries, for every viewer.

import type { MenuEntry } from "@pidash/kit";

import { COPY_WORK_ITEM_LINK_LABEL } from "../../../shared/shell/CopyWorkItemLinkButton.js";
import {
  TRIAGE_DELETE_LABEL,
  TRIAGE_DUPLICATE_LABEL,
  TRIAGE_SNOOZE_LABEL,
  TRIAGE_UNSNOOZE_LABEL,
} from "../lib/triage.js";
import { OPEN_WORK_ITEM_LABEL } from "./IntakeDetailHeader.js";
import { TRIAGE_ACCEPT_LABEL, TRIAGE_DECLINE_LABEL } from "./IntakeTriageActions.js";

export interface IntakeMobileMenuInput {
  /** Accepted, declined or duplicate: link and open entries only. */
  resolved: boolean;
  /** Admin or member standing on an open request (the desktop triage gate). */
  showTriage: boolean;
  /** Admin or creator standing on an open request (the desktop delete gate). */
  showDelete: boolean;
  /** The request is snoozed, so the entry clears instead of opening. */
  snoozed: boolean;
  onSnooze: () => void;
  onUnsnooze: () => void;
  onDuplicate: () => void;
  onAccept: () => void;
  onDecline: () => void;
  onDelete: () => void;
  onCopyLink: () => void;
  onOpenWorkItem: () => void;
}

/**
 * The mobile header's menu items in spec order. Accept and decline fire
 * through the shared triage controls, so a member's click is refused
 * with the same notice as on desktop.
 */
export function intakeMobileMenuItems(input: IntakeMobileMenuInput): MenuEntry[] {
  if (input.resolved) {
    return [
      { label: COPY_WORK_ITEM_LINK_LABEL, onSelect: input.onCopyLink },
      { label: OPEN_WORK_ITEM_LABEL, onSelect: input.onOpenWorkItem },
    ];
  }
  // The menu trigger is not gated like the desktop overflow (INT-030), so
  // a creator outside the triage gate still gets the delete entry that
  // INT-025 promises them. Everyone else outside the gate gets no entries.
  if (!input.showTriage) {
    return input.showDelete ? [{ label: TRIAGE_DELETE_LABEL, onSelect: input.onDelete }] : [];
  }
  return [
    input.snoozed
      ? { label: TRIAGE_UNSNOOZE_LABEL, onSelect: input.onUnsnooze }
      : { label: TRIAGE_SNOOZE_LABEL, onSelect: input.onSnooze },
    { label: TRIAGE_DUPLICATE_LABEL, onSelect: input.onDuplicate },
    { label: TRIAGE_ACCEPT_LABEL, onSelect: input.onAccept },
    { label: TRIAGE_DECLINE_LABEL, onSelect: input.onDecline },
    ...(input.showDelete ? [{ label: TRIAGE_DELETE_LABEL, onSelect: input.onDelete }] : []),
    { label: COPY_WORK_ITEM_LINK_LABEL, onSelect: input.onCopyLink },
  ];
}

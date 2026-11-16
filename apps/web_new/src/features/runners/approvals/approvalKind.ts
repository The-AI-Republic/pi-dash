// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Headings for the pending-approval queue (NEWFRONT-547, RUN-023). One
// short label per request kind; the chat prompts carry their own
// sentence-shaped wording, so this module stays queue-specific.

import type { ApprovalKind } from "@pidash/api-client";

/** Card heading per approval kind. */
export const APPROVAL_KIND_LABELS: Record<ApprovalKind, string> = {
  command_execution: "Run a shell command",
  file_change: "Change a file",
  network_access: "Reach the network",
  other: "Needs your approval",
};

/** The card heading for one request kind. */
export function approvalKindLabel(kind: ApprovalKind): string {
  return APPROVAL_KIND_LABELS[kind];
}

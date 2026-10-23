/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { TAgentRunStatus } from "@pi-dash/types";
import type { TBadgeVariant } from "@pi-dash/ui";
import { Badge } from "@pi-dash/ui";

/**
 * Shared AgentRun status badge, used by the runners Runs page and the
 * scheduler-binding detail page so every surface renders the same variant
 * and label per status. Covers all AgentRunStatus values — do not collapse.
 */

const STATUS_BADGE_VARIANT: Record<TAgentRunStatus, TBadgeVariant> = {
  queued: "accent-neutral",
  assigned: "accent-primary",
  running: "primary",
  cancel_requested: "accent-warning",
  awaiting_approval: "accent-warning",
  awaiting_reauth: "accent-warning",
  paused_awaiting_input: "accent-warning",
  blocked: "accent-warning",
  completed: "accent-success",
  failed: "accent-destructive",
  cancelled: "accent-neutral",
  refused: "accent-destructive",
};

const RUN_STATUS_I18N_LABELS: Record<TAgentRunStatus, string> = {
  queued: "queued",
  assigned: "assigned",
  running: "running",
  cancel_requested: "cancellation requested",
  awaiting_approval: "awaiting approval",
  awaiting_reauth: "awaiting reauth",
  paused_awaiting_input: "paused awaiting input",
  blocked: "blocked",
  completed: "completed",
  failed: "failed",
  cancelled: "cancelled",
  refused: "refused",
};

type TranslationFn = (key: string, vars?: Record<string, unknown>) => string;

// Historical runs can still carry a status that is no longer in the union
// (e.g. the retired `waiting_for_worktree`). Fall back so those rows render
// with a neutral badge and their raw status label instead of crashing.
export function statusBadgeVariant(status: TAgentRunStatus): TBadgeVariant {
  return (STATUS_BADGE_VARIANT as Partial<Record<string, TBadgeVariant>>)[status] ?? "accent-neutral";
}

export function statusLabel(status: TAgentRunStatus, t: TranslationFn): string {
  return t((RUN_STATUS_I18N_LABELS as Partial<Record<string, string>>)[status] ?? status);
}

type Props = {
  status: TAgentRunStatus;
  t: TranslationFn;
};

export function RunStatusBadge({ status, t }: Props) {
  return (
    <Badge variant={statusBadgeVariant(status)} size="sm">
      {statusLabel(status, t)}
    </Badge>
  );
}

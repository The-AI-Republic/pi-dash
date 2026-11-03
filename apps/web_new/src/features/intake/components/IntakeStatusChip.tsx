// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Request status chip (parity/specs/intake.md INT-022/024). The detail
// header's reading of the intake status: the resolved names, Pending,
// and the snooze countdown in whole days. A lapsed snooze keeps the
// chip with the passed tone instead of vanishing (NEWFRONT-272).

import { cn } from "@pidash/kit";
import * as React from "react";

import { todayIsoDay } from "../../issues/index.js";
import { selectIntakeStatusChip } from "../lib/triage.js";

export interface IntakeStatusChipProps {
  status: number;
  snoozedTill: string | null;
}

export function IntakeStatusChip({ status, snoozedTill }: IntakeStatusChipProps): React.ReactElement {
  const chip = selectIntakeStatusChip(status, snoozedTill, todayIsoDay());
  return (
    <span
      data-testid="intake-status-chip"
      data-tone={chip.tone}
      className={cn(
        "text-meta inline-flex shrink-0 items-center rounded-(--radius-full) px-(--space-3) py-(--space-1) font-medium",
        chip.tone === "danger" ? "bg-(--subtle) text-(--danger)" : "bg-(--subtle) text-(--text-muted)"
      )}
    >
      {chip.text}
    </span>
  );
}

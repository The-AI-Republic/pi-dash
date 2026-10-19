// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics dialog body (parity/specs/issues.md §9, ISS-107). Split from
// the header entry so the kit dialog stays out of the initial route closure;
// the page opens it on demand.

import { Dialog } from "@pidash/kit";
import * as React from "react";

export interface AnalyticsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  isEpics: boolean;
  projectName: string;
  total: number;
  groups: readonly { label: string; count: number }[];
}

export function AnalyticsDialog({
  open,
  onOpenChange,
  isEpics,
  projectName,
  total,
  groups,
}: AnalyticsDialogProps): React.ReactElement {
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={isEpics ? "Epic analytics" : "Work items analytics"}
      description={`Work items in ${projectName}`}
    >
      <div className="flex flex-col gap-(--space-3)" data-testid="issues-analytics-body">
        <p className="text-body text-(--text)">
          Total Work items <span data-testid="analytics-total">{total}</span>
        </p>
        {groups.length > 0 ? (
          <dl className="flex flex-col gap-(--space-1)">
            {groups.map((row) => (
              <div key={row.label} className="flex items-center justify-between gap-(--space-4)">
                <dt className="text-body text-(--text-muted)">{row.label}</dt>
                <dd className="text-body text-(--text)">{row.count}</dd>
              </div>
            ))}
          </dl>
        ) : (
          <p className="text-body text-(--text-muted)">No grouped counts to show yet.</p>
        )}
      </div>
    </Dialog>
  );
}

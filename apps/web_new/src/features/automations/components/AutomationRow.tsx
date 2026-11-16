// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One idle-automation row (parity/specs/agents.md §8, AGT-034/035).
// The row is a level-4 heading with its explanatory line and a switch;
// switching it on reveals the row's detail panel, which the caller
// supplies. Callers own the write, so a refused update leaves the row
// exactly as the server last stored it.

import { Switch } from "@pidash/kit";
import * as React from "react";

export interface AutomationRowProps {
  title: string;
  description: string;
  /** Decorative leading glyph; the row is named by its heading. */
  icon?: React.ReactNode;
  checked: boolean;
  disabled?: boolean;
  onToggle: (next: boolean) => void;
  /** Detail panel shown only while the automation is on. */
  children?: React.ReactNode;
}

export function AutomationRow({
  title,
  description,
  icon,
  checked,
  disabled = false,
  onToggle,
  children,
}: AutomationRowProps): React.ReactElement {
  const descriptionId = React.useId();
  return (
    <div data-automation-row="" className="flex flex-col gap-(--space-3) border-b border-(--border) py-(--space-4)">
      <div className="flex items-start gap-(--space-3)">
        {icon === undefined ? null : (
          <span
            aria-hidden="true"
            className="grid size-9 shrink-0 place-items-center rounded-(--radius-control) bg-(--surface) text-(--text-muted)"
          >
            {icon}
          </span>
        )}
        <div className="flex min-w-0 flex-1 items-start justify-between gap-(--space-4)">
          <div className="flex min-w-0 flex-col gap-(--space-1)">
            <h4 className="text-body font-medium text-(--text)">{title}</h4>
            <p id={descriptionId} className="text-meta text-(--text-muted)">
              {description}
            </p>
          </div>
          <Switch
            checked={checked}
            onCheckedChange={onToggle}
            label={title}
            disabled={disabled}
            className="mt-(--space-1)"
          />
        </div>
      </div>
      {checked ? <div className="ml-12 flex flex-col gap-(--space-3)">{children}</div> : null}
    </div>
  );
}

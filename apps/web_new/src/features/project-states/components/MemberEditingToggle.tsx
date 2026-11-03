// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin-only member-editing toggle (parity/specs/
// project-settings-states.md, PSET-034). It flips the project's
// members-may-edit-states flag; the switch reflects the cached project
// value so it reads back immediately, and a refusal leaves it where it
// was with a notice.

import { Switch } from "@pidash/kit";
import * as React from "react";

export interface MemberEditingToggleProps {
  enabled: boolean;
  pending: boolean;
  onChange: (enabled: boolean) => void;
}

export function MemberEditingToggle({ enabled, pending, onChange }: MemberEditingToggleProps): React.ReactElement {
  const labelId = React.useId();
  return (
    <div
      data-testid="state-member-editing"
      className="flex items-center justify-between gap-(--space-3) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) py-(--space-3)"
    >
      <div className="min-w-0">
        <div id={labelId} className="text-body font-medium text-(--text)">
          Let members edit states
        </div>
        <p className="text-body text-(--text-muted)">
          When on, project members can create, edit, reorder and delete states.
        </p>
      </div>
      <Switch
        data-testid="state-member-editing-switch"
        aria-labelledby={labelId}
        checked={enabled}
        disabled={pending}
        onCheckedChange={onChange}
      />
    </div>
  );
}

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module lifecycle status control (parity/specs/modules.md §4.2,
// MOD-014). The pill paints the current status label in its status color
// (the oracle reads both back); the menu lists all six statuses with
// their dots. Labels, colors and order come from the shared status
// table slice B owns — this control reads it, never a parallel copy.
// Picking one saves immediately with the pinned notice, while re-picking
// the current status is a silent no-op. Guests and archived rows get the
// same pill as a disabled button with no menu. The row, the card and
// (later) the analytics sidebar mount this same control; only the test
// handle differs.

import { Menu } from "@pidash/kit";
import type { ModuleRow, ModuleStatus } from "@pidash/api-client";
import * as React from "react";

import { MODULE_STATUS_OPTIONS, moduleStatusColor, moduleStatusLabel } from "../../lib/moduleListControls.js";
import { useModuleInlineSave } from "../../lib/useModuleInlineSave.js";

export interface ModuleStatusControlProps {
  workspaceSlug: string;
  projectId: string;
  module: Pick<ModuleRow, "id" | "name" | "status">;
  /** Guests and archived rows render the pill disabled with no menu. */
  disabled: boolean;
  /** Test handle the pill carries (the oracle reads it back). */
  testId: string;
}

export function ModuleStatusControl({
  workspaceSlug,
  projectId,
  module,
  disabled,
  testId,
}: ModuleStatusControlProps): React.ReactElement {
  const save = useModuleInlineSave({ workspaceSlug, projectId });
  const label = moduleStatusLabel(module.status);
  const color = moduleStatusColor(module.status);

  const pick = (status: ModuleStatus): void => {
    if (status === module.status) return;
    save.saveStatus(module.id, status);
  };

  if (disabled) {
    return (
      <button
        type="button"
        disabled
        aria-label={`Change status of ${module.name}`}
        data-testid={testId}
        data-status-color={color}
        style={{ color }}
        className="text-body inline-flex cursor-not-allowed items-center rounded-(--radius-control) px-(--space-2) py-(--space-1) font-medium"
      >
        {label}
      </button>
    );
  }

  return (
    <Menu
      trigger={
        <button
          type="button"
          aria-label={`Change status of ${module.name}`}
          data-testid={testId}
          data-status-color={color}
          style={{ color }}
          className="text-body inline-flex cursor-pointer items-center rounded-(--radius-control) px-(--space-2) py-(--space-1) font-medium hover:bg-(--subtle)"
        >
          {label}
        </button>
      }
      items={MODULE_STATUS_OPTIONS.map((option) => ({
        label: option.label,
        icon: (
          <span
            aria-hidden="true"
            style={{ backgroundColor: option.color }}
            className="inline-block size-2 shrink-0 rounded-full"
          />
        ),
        onSelect: () => pick(option.value),
      }))}
    />
  );
}

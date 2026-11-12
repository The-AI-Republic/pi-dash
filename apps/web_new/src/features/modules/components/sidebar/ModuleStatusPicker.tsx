// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module lifecycle control (parity/specs/modules.md §4.2, MOD-033).
// One picker over the six statuses with the label and tone from the
// module model; it writes the selected wire value through the sidebar's
// partial PATCH. The trigger is a badge rather than the generic button so
// the status reads as a state, not an action.

import { Badge, Combobox, type BadgeTone, type ComboboxOption } from "@pidash/kit";
import { MODULE_STATUSES, type ModuleStatus } from "@pidash/api-client";
import * as React from "react";

import { MODULE_STATUS_LABELS } from "./moduleSidebarModel.js";

export const MODULE_STATUS_TONES: Record<ModuleStatus, BadgeTone> = {
  backlog: "neutral",
  planned: "accent",
  "in-progress": "warning",
  paused: "neutral",
  completed: "success",
  cancelled: "danger",
};

export interface ModuleStatusPickerProps {
  value: ModuleStatus;
  disabled: boolean;
  onChange: (status: ModuleStatus) => void;
}

function toOption(status: ModuleStatus): ComboboxOption {
  return {
    id: status,
    label: MODULE_STATUS_LABELS[status],
    trailing: (
      <Badge tone={MODULE_STATUS_TONES[status]} dot>
        {MODULE_STATUS_LABELS[status]}
      </Badge>
    ),
  };
}

export function ModuleStatusPicker({ value, disabled, onChange }: ModuleStatusPickerProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const selected = MODULE_STATUS_LABELS[value];

  return (
    <Combobox
      trigger={
        <button
          type="button"
          aria-label={selected}
          disabled={disabled}
          className="rounded-(--radius-full) outline-none focus:ring-1 focus:ring-(--accent) disabled:cursor-not-allowed"
        >
          <Badge tone={MODULE_STATUS_TONES[value]} dot>
            {selected}
          </Badge>
        </button>
      }
      options={MODULE_STATUSES.map(toOption)}
      value={value}
      onSelect={(status) => {
        setOpen(false);
        if (status !== value) onChange(status as ModuleStatus);
      }}
      listLabel="Module statuses"
      searchEnabled={false}
      emptyText="No statuses found"
      disabled={disabled}
      open={open}
      onOpenChange={setOpen}
    />
  );
}

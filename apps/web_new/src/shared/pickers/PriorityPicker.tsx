// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Priority picker (parity/specs/issues.md §17, ISS-210). Single-select
// over the five fixed levels with per-level glyphs; urgent renders
// outlined. None is a real value rather than a clear action. Static:
// no reads, no loading state.
import { Button, Combobox, IconButton, type ButtonVariant, type ComboboxOption } from "@pidash/kit";
import * as React from "react";

import {
  PRIORITY_ICONS,
  PRIORITY_TONES,
  selectPriorityOptions,
  type PriorityLevel,
  type PriorityOption,
} from "./priorities.js";

export interface PriorityPickerProps {
  value: PriorityLevel;
  onChange: (level: PriorityLevel) => void;
  disabled?: boolean;
  /** Button variant of the default trigger. */
  variant?: ButtonVariant;
  /** Default trigger shows the glyph without the label. */
  iconOnly?: boolean;
  /** Fully custom trigger; overrides the default value button. */
  trigger?: React.ReactElement<Record<string, unknown>>;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}

function PriorityGlyph({ option }: { option: PriorityOption }): React.ReactElement {
  const Icon = PRIORITY_ICONS[option.level];
  const glyph = <Icon size={16} strokeWidth={1.5} className={PRIORITY_TONES[option.level]} aria-hidden="true" />;
  if (option.level !== "urgent") return glyph;
  return (
    <span className="inline-flex items-center rounded-(--radius-control) border border-(--danger) p-px">{glyph}</span>
  );
}

function toComboboxOption(option: PriorityOption): ComboboxOption {
  return {
    id: option.level,
    label: option.label,
    icon: <PriorityGlyph option={option} />,
  };
}

export function PriorityPicker({
  value,
  onChange,
  disabled = false,
  variant = "secondary",
  iconOnly = false,
  trigger,
  open: controlledOpen,
  onOpenChange,
}: PriorityPickerProps): React.ReactElement {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const open = controlledOpen ?? uncontrolledOpen;

  function setOpen(next: boolean): void {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }

  const options = React.useMemo(() => selectPriorityOptions(), []);
  // The value is one of the five levels, so the find always lands.
  const current = options.find((option) => option.level === value)!;

  function defaultTrigger(): React.ReactElement<Record<string, unknown>> {
    if (iconOnly) {
      return (
        <IconButton label={current.label} variant={variant}>
          <PriorityGlyph option={current} />
        </IconButton>
      );
    }
    return (
      <Button variant={variant}>
        <span aria-hidden="true" className="inline-flex shrink-0 items-center">
          <PriorityGlyph option={current} />
        </span>
        <span className="truncate">{current.label}</span>
      </Button>
    );
  }

  return (
    <Combobox
      trigger={trigger ?? defaultTrigger()}
      options={options.map(toComboboxOption)}
      value={value}
      onSelect={(id) => onChange(id as PriorityLevel)}
      listLabel="Priorities"
      searchLabel="Search priorities"
      emptyText="No matching results"
      disabled={disabled}
      open={open}
      onOpenChange={setOpen}
    />
  );
}

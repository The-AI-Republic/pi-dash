// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle picker (parity/specs/issues.md §17, ISS-212). Single-select
// over the project's active cycles with search; a No-cycle row clears
// the assignment. Completed cycles never list. The assigned cycle stays
// listed unless the caller excludes it. There is no inline cycle
// creation. Reads resolve lazily on open; a caller that has to name an
// existing value before the popup opens preloads the list instead.
import { Button, Combobox, IconButton, type ButtonVariant, type ComboboxOption } from "@pidash/kit";
import { Repeat } from "lucide-react";
import * as React from "react";

import { selectCycleOptions, type CycleOption } from "./cycles.js";
import { usePickerCycles } from "./reference.js";

export interface CyclePickerProps {
  workspaceSlug: string;
  projectId: string;
  value: string | null;
  onChange: (id: string | null) => void;
  /** False hides the No-cycle clear row. */
  allowClear?: boolean;
  /** True lists completed cycles too (the cycle detail switcher, CYC-033). */
  includeCompleted?: boolean;
  /** Cycles hidden from the list (no caller passes this yet). */
  excludeCycleIds?: readonly string[];
  disabled?: boolean;
  /** Button variant of the default trigger. */
  variant?: ButtonVariant;
  /** Default trigger shows the glyph without the name. */
  iconOnly?: boolean;
  /** Fully custom trigger; overrides the default value button. */
  trigger?: React.ReactElement<Record<string, unknown>>;
  /** Default trigger text when nothing is selected. */
  placeholder?: string;
  /** Explicit keyboard position inside a dialog's tab order. */
  tabIndex?: number | undefined;
  /** Stable id for the trigger, so dialogs can name the field. */
  triggerId?: string | undefined;
  /** Read the list up front so a set value names itself before the popup opens. */
  preload?: boolean;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}

function CycleGlyph(): React.ReactElement {
  return <Repeat size={16} strokeWidth={1.5} className="text-(--text-muted)" aria-hidden="true" />;
}

function toComboboxOption(option: CycleOption): ComboboxOption {
  return {
    id: option.id,
    label: option.name,
    icon: <CycleGlyph />,
  };
}

export function CyclePicker({
  workspaceSlug,
  projectId,
  value,
  onChange,
  allowClear = true,
  includeCompleted = false,
  excludeCycleIds,
  disabled = false,
  variant = "secondary",
  iconOnly = false,
  trigger,
  placeholder = "No cycle",
  tabIndex,
  triggerId,
  preload = false,
  open: controlledOpen,
  onOpenChange,
}: CyclePickerProps): React.ReactElement {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const open = controlledOpen ?? uncontrolledOpen;

  function setOpen(next: boolean): void {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }

  const cycles = usePickerCycles(workspaceSlug, projectId, open || preload);
  const options = React.useMemo(
    () => selectCycleOptions({ cycles: cycles.data ?? [], excludeIds: excludeCycleIds, allowClear, includeCompleted }),
    [cycles.data, excludeCycleIds, allowClear, includeCompleted]
  );
  const loading = open && !disabled && cycles.isPending;
  const errorText = cycles.isError ? "Couldn't load cycles." : undefined;

  function handleSelect(id: string): void {
    onChange(id === "" ? null : id);
  }

  function defaultTrigger(): React.ReactElement<Record<string, unknown>> {
    const current = value === null ? undefined : options.find((option) => option.id === value);
    const text = current?.name ?? placeholder;
    if (iconOnly) {
      return (
        <IconButton label={text} variant={variant}>
          <CycleGlyph />
        </IconButton>
      );
    }
    return (
      <Button
        variant={variant}
        {...(triggerId === undefined ? {} : { id: triggerId })}
        {...(tabIndex === undefined ? {} : { tabIndex })}
      >
        <CycleGlyph />
        <span className="truncate">{text}</span>
      </Button>
    );
  }

  return (
    <Combobox
      trigger={trigger ?? defaultTrigger()}
      options={options.map(toComboboxOption)}
      value={value}
      onSelect={handleSelect}
      listLabel="Cycles"
      searchLabel="Search cycles"
      emptyText="No matches found"
      loading={loading}
      loadingText="Loading cycles"
      errorText={errorText}
      disabled={disabled}
      open={open}
      onOpenChange={setOpen}
    />
  );
}

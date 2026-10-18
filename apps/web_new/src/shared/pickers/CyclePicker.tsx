// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle picker (parity/specs/issues.md §17, ISS-212). Single-select
// over the project's active cycles with search; a No-cycle row clears
// the assignment. Completed cycles never list. The assigned cycle stays
// listed unless the caller excludes it. There is no inline cycle
// creation. Reads resolve lazily on open.
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
  excludeCycleIds,
  disabled = false,
  variant = "secondary",
  iconOnly = false,
  trigger,
  placeholder = "No cycle",
  open: controlledOpen,
  onOpenChange,
}: CyclePickerProps): React.ReactElement {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const open = controlledOpen ?? uncontrolledOpen;

  function setOpen(next: boolean): void {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }

  const cycles = usePickerCycles(workspaceSlug, projectId, open && !disabled);
  const options = React.useMemo(
    () => selectCycleOptions({ cycles: cycles.data ?? [], excludeIds: excludeCycleIds, allowClear }),
    [cycles.data, excludeCycleIds, allowClear]
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
      <Button variant={variant}>
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

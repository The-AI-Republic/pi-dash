// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake-state picker (parity/specs/issues.md §17, ISS-217).
// Single-select over the project's intake states with search, each
// with its group glyph in the state's own color. There is no clear
// row; the trigger falls back to the first state. Reads resolve
// lazily on open.
import { Button, Combobox, IconButton, type ButtonVariant, type ComboboxOption } from "@pidash/kit";
import * as React from "react";

import { selectIntakeFallback } from "./intakeStates.js";
import { usePickerIntakeStates } from "./reference.js";
import { StateGlyph } from "./StatePicker.js";
import { selectStateOptions, type StateOption } from "./states.js";

export interface IntakeStatePickerProps {
  workspaceSlug: string;
  projectId: string;
  value: string | null;
  onChange: (id: string) => void;
  disabled?: boolean;
  /** Read states up front so a set selection names itself before the popup opens. */
  preload?: boolean;
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

function toComboboxOption(option: StateOption): ComboboxOption {
  return {
    id: option.id,
    label: option.name,
    searchText: `${option.name} ${option.group}`,
    icon: <StateGlyph option={option} />,
  };
}

export function IntakeStatePicker({
  workspaceSlug,
  projectId,
  value,
  onChange,
  disabled = false,
  preload = false,
  variant = "secondary",
  iconOnly = false,
  trigger,
  placeholder = "Select intake state",
  open: controlledOpen,
  onOpenChange,
}: IntakeStatePickerProps): React.ReactElement {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const open = controlledOpen ?? uncontrolledOpen;

  function setOpen(next: boolean): void {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }

  const readsAllowed = open || preload;
  const states = usePickerIntakeStates(workspaceSlug, projectId, readsAllowed);
  const options = React.useMemo(() => selectStateOptions(states.data ?? []), [states.data]);
  const loading = open && !disabled && states.isPending;
  const errorText = states.isError ? "Couldn't load intake states." : undefined;

  function defaultTrigger(): React.ReactElement<Record<string, unknown>> {
    const current = value === null ? undefined : options.find((option) => option.id === value);
    const fallback = value === null ? (selectIntakeFallback(states.data ?? []) ?? undefined) : undefined;
    const shown = current ?? fallback;
    const text = shown?.name ?? placeholder;
    if (iconOnly) {
      return (
        <IconButton label={text} variant={variant}>
          {shown === undefined ? null : <StateGlyph option={shown} />}
        </IconButton>
      );
    }
    return (
      <Button variant={variant}>
        {shown === undefined ? null : (
          <span aria-hidden="true" className="inline-flex shrink-0 items-center">
            <StateGlyph option={shown} />
          </span>
        )}
        <span className="truncate">{text}</span>
      </Button>
    );
  }

  return (
    <Combobox
      trigger={trigger ?? defaultTrigger()}
      options={options.map(toComboboxOption)}
      value={value}
      onSelect={onChange}
      listLabel="Intake states"
      searchLabel="Search intake states"
      emptyText="No matching results"
      loading={loading}
      loadingText="Loading intake states"
      errorText={errorText}
      disabled={disabled}
      open={open}
      onOpenChange={setOpen}
    />
  );
}

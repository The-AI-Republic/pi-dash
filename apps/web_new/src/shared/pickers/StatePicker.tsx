// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// State picker (parity/specs/issues.md §17, ISS-209). Single-select over
// the project's workflow states in server order, each with its group
// glyph in the state's own color. There is no clear row — a state is
// always set — and no workflow-aware option in this build. Reads resolve
// lazily on open behind a spinner; a caller that has to name an existing
// value before the popup opens preloads the list instead.
import { Button, Combobox, IconButton, type ButtonVariant, type ComboboxOption } from "@pidash/kit";
import * as React from "react";

import { usePickerStates } from "./reference.js";
import { selectStateOptions, STATE_GROUP_ICONS, type StateOption } from "./states.js";

export interface StatePickerProps {
  workspaceSlug: string;
  projectId: string;
  value: string | null;
  onChange: (id: string) => void;
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
  /** Read the list up front so a set value names itself before the popup opens. */
  preload?: boolean;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}

/** Group glyph in the state's own color; the intake picker reuses it for triage states. */
export function StateGlyph({ option, size = 16 }: { option: StateOption; size?: number }): React.ReactElement {
  const Icon = STATE_GROUP_ICONS[option.group];
  return <Icon size={size} strokeWidth={1.5} style={{ color: option.color }} aria-hidden="true" />;
}

function toComboboxOption(option: StateOption): ComboboxOption {
  return {
    id: option.id,
    label: option.name,
    searchText: `${option.name} ${option.group}`,
    icon: <StateGlyph option={option} />,
  };
}

export function StatePicker({
  workspaceSlug,
  projectId,
  value,
  onChange,
  disabled = false,
  variant = "secondary",
  iconOnly = false,
  trigger,
  placeholder = "Select state",
  tabIndex,
  preload = false,
  open: controlledOpen,
  onOpenChange,
}: StatePickerProps): React.ReactElement {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const open = controlledOpen ?? uncontrolledOpen;

  function setOpen(next: boolean): void {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }

  const states = usePickerStates(workspaceSlug, projectId, open || preload);
  const options = React.useMemo(() => selectStateOptions(states.data ?? []), [states.data]);
  const loading = open && !disabled && states.isPending;
  const errorText = states.isError ? "Couldn't load states." : undefined;

  function defaultTrigger(): React.ReactElement<Record<string, unknown>> {
    const current = value === null ? undefined : options.find((option) => option.id === value);
    const text = current?.name ?? placeholder;
    if (iconOnly) {
      return (
        <IconButton label={text} variant={variant}>
          {current === undefined ? null : <StateGlyph option={current} />}
        </IconButton>
      );
    }
    return (
      <Button variant={variant} {...(tabIndex === undefined ? {} : { tabIndex })}>
        {current === undefined ? null : (
          <span aria-hidden="true" className="inline-flex shrink-0 items-center">
            <StateGlyph option={current} />
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
      listLabel="States"
      searchLabel="Search states"
      emptyText="No matching results"
      loading={loading}
      loadingText="Loading states"
      errorText={errorText}
      disabled={disabled}
      open={open}
      onOpenChange={setOpen}
    />
  );
}

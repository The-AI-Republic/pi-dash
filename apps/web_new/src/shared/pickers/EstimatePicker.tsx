// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimate picker (parity/specs/issues.md §17, ISS-216).
// Single-select over the active system's points with search; a
// No-estimate row clears the point. TIME-system points render as
// hours and minutes. Without an active system the list holds one
// non-interactive empty row. Reads resolve lazily on open. Search
// matches the displayed labels (EST-017): a deliberate divergence
// from the old picker, which indexed the raw stored value (bug
// NEWFRONT-820) — the palette page filters rendered text too, so
// both surfaces agree.
import { Button, Combobox, IconButton, type ButtonVariant, type ComboboxOption } from "@pidash/kit";
import { Hash } from "lucide-react";
import * as React from "react";
import { selectActiveEstimate } from "@pidash/api-client";

import { NO_ESTIMATE_ID, selectEstimateOptions, type EstimateOption } from "./estimates.js";
import { usePickerEstimates } from "./reference.js";

export interface EstimatePickerProps {
  workspaceSlug: string;
  projectId: string;
  value: string | null;
  onChange: (id: string | null) => void;
  disabled?: boolean;
  /** Button variant of the default trigger. */
  variant?: ButtonVariant;
  /** Default trigger shows the glyph without the value. */
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

function EstimateGlyph(): React.ReactElement {
  return <Hash size={16} strokeWidth={1.5} className="text-(--text-muted)" aria-hidden="true" />;
}

function toComboboxOption(option: EstimateOption): ComboboxOption {
  return {
    id: option.id,
    label: option.label,
    disabled: option.disabled,
    icon: <EstimateGlyph />,
  };
}

export function EstimatePicker({
  workspaceSlug,
  projectId,
  value,
  onChange,
  disabled = false,
  variant = "secondary",
  iconOnly = false,
  trigger,
  placeholder = "None",
  tabIndex,
  preload = false,
  open: controlledOpen,
  onOpenChange,
}: EstimatePickerProps): React.ReactElement {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const open = controlledOpen ?? uncontrolledOpen;

  function setOpen(next: boolean): void {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }

  const estimates = usePickerEstimates(workspaceSlug, projectId, open || preload);
  const options = React.useMemo(
    () => selectEstimateOptions(estimates.data === undefined ? undefined : selectActiveEstimate(estimates.data)),
    [estimates.data]
  );
  const loading = open && !disabled && estimates.isPending;
  const errorText = estimates.isError ? "Couldn't load estimates." : undefined;

  function handleSelect(id: string): void {
    onChange(id === NO_ESTIMATE_ID ? null : id);
  }

  function defaultTrigger(): React.ReactElement<Record<string, unknown>> {
    const current = value === null ? undefined : options.find((option) => option.id === value);
    const text = current?.label ?? placeholder;
    if (iconOnly) {
      return (
        <IconButton label={text} variant={variant}>
          <EstimateGlyph />
        </IconButton>
      );
    }
    return (
      <Button variant={variant} {...(tabIndex === undefined ? {} : { tabIndex })}>
        <EstimateGlyph />
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
      listLabel="Estimates"
      searchLabel="Search estimates"
      emptyText="No matching results"
      loading={loading}
      loadingText="Loading estimates"
      errorText={errorText}
      disabled={disabled}
      open={open}
      onOpenChange={setOpen}
    />
  );
}

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Layout picker (parity/specs/issues.md §17, ISS-219). A
// search-disabled dropdown over the five layouts in switcher order
// with icon plus label and a checkmark on the selected entry; an
// exclusion list hides options. Labels reuse the layout kinds so the
// picker and the header switcher name layouts identically.
import { Button, Combobox, IconButton, type ButtonVariant, type ComboboxOption } from "@pidash/kit";
import * as React from "react";

import { LayoutIcon } from "../components/LayoutIcon.js";
import { LAYOUT_LABELS, type LayoutKind } from "../layouts/kinds.js";
import { layoutOptionList, type LayoutOption } from "./layoutOptions.js";

export interface LayoutPickerProps {
  /** Selected layout. The trigger always names it, even when excluded. */
  value: LayoutKind;
  onChange: (kind: LayoutKind) => void;
  /** Kinds hidden from the menu. */
  excluded?: readonly LayoutKind[] | undefined;
  disabled?: boolean | undefined;
  /** Button variant of the default trigger. */
  variant?: ButtonVariant | undefined;
  /** Default trigger shows the glyph without the name. */
  iconOnly?: boolean | undefined;
  /** Fully custom trigger; overrides the default value button. */
  trigger?: React.ReactElement<Record<string, unknown>> | undefined;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}

function toComboboxOption(option: LayoutOption): ComboboxOption {
  return {
    id: option.kind,
    label: option.label,
    icon: <LayoutIcon kind={option.kind} />,
  };
}

/** Layout dropdown for the view-create form and layout switchers. */
export function LayoutPicker({
  value,
  onChange,
  excluded,
  disabled = false,
  variant = "secondary",
  iconOnly = false,
  trigger,
  open: controlledOpen,
  onOpenChange,
}: LayoutPickerProps): React.ReactElement | null {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const open = controlledOpen ?? uncontrolledOpen;

  function setOpen(next: boolean): void {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }

  const options = layoutOptionList(excluded);
  if (options.length === 0) return null;

  function handleSelect(id: string): void {
    onChange(id as LayoutKind);
  }

  function defaultTrigger(): React.ReactElement<Record<string, unknown>> {
    const text = LAYOUT_LABELS[value];
    if (iconOnly) {
      return (
        <IconButton label={text} variant={variant}>
          <LayoutIcon kind={value} />
        </IconButton>
      );
    }
    return (
      <Button variant={variant}>
        <LayoutIcon kind={value} />
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
      listLabel="Layouts"
      searchEnabled={false}
      emptyText="No layouts available"
      disabled={disabled}
      open={open}
      onOpenChange={setOpen}
    />
  );
}

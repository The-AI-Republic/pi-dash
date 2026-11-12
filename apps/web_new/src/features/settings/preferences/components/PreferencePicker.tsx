// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// A preferences row picker (parity/specs/profile.md §7, PROF-016/017). The
// trigger reads the current label; the list offers every option and marks
// the stored one. Picking reports the value and closes; the caller saves.

import { Combobox, type ComboboxOption } from "@pidash/kit";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import type { PreferenceOption } from "../lib/choices.js";

export interface PreferencePickerProps<T extends string | number> {
  /** Accessible name of the control and its list. */
  label: string;
  /** Current label, shown on the trigger. */
  valueLabel: string;
  /** Stored value; marks the selected option. */
  value: T;
  options: ReadonlyArray<PreferenceOption<T>>;
  onSelect: (value: T) => void;
  disabled?: boolean;
  /** Extra searchable text per option (timezone offsets, for example). */
  searchText?: (option: PreferenceOption<T>) => string | undefined;
}

export const PICKER_SEARCH_LABEL = "Search";

export function PreferencePicker<T extends string | number>({
  label,
  valueLabel,
  value,
  options,
  onSelect,
  disabled = false,
  searchText,
}: PreferencePickerProps<T>): React.ReactElement {
  const items: ComboboxOption[] = options.map((option) => {
    const extra = searchText?.(option);
    return { id: String(option.value), label: option.label, ...(extra === undefined ? {} : { searchText: extra }) };
  });
  return (
    <Combobox
      trigger={
        <button
          type="button"
          aria-label={label}
          className="text-body flex h-(--control-height) min-w-48 cursor-pointer items-center justify-between gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) text-left text-(--text) disabled:cursor-not-allowed disabled:opacity-50"
        >
          <span aria-label={valueLabel}>{valueLabel}</span>
          <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
        </button>
      }
      options={items}
      value={String(value)}
      onSelect={(id) => {
        const picked = options.find((option) => String(option.value) === id);
        if (picked !== undefined) onSelect(picked.value);
      }}
      listLabel={`${label} options`}
      searchLabel={PICKER_SEARCH_LABEL}
      emptyText="No matching options."
      disabled={disabled}
    />
  );
}

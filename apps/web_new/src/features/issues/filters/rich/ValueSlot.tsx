// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rich-filter value slot (parity/specs/issues.md §9, ISS-097). The core
// slice supplies the choice-based slot the live field catalogue uses:
// multi-select values for state, state group and priority. The richer
// single/date/range editors and their commit rules belong to ISS-100–103
// and replace this component through the same props boundary.

import { Button, Combobox, type ComboboxOption } from "@pidash/kit";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import { usePickerStates } from "../../../../shared/pickers/index.js";
import { selectStateOptions } from "../../../../shared/pickers/states.js";
import type { RichCondition, RichFilterField } from "./expression.js";

export interface ValueSlotProps {
  workspaceSlug: string;
  projectId: string;
  field: RichFilterField;
  condition: RichCondition;
  onChange: (value: unknown) => void;
}

/** Selected values as a string array; a lone stored value reads as one pick. */
export function selectedValues(value: unknown): string[] {
  if (Array.isArray(value)) return value.filter((entry): entry is string => typeof entry === "string");
  return typeof value === "string" && value.length > 0 ? [value] : [];
}

function chosenLabel(
  field: RichFilterField,
  values: readonly string[],
  stateLabels: ReadonlyMap<string, string>
): string {
  if (values.length === 0) return "Select value";
  const labels = values.map(
    (value) => stateLabels.get(value) ?? field.staticValues?.find((option) => option.value === value)?.label ?? value
  );
  if (labels.length <= 2) return labels.join(", ");
  return `${labels.length} selected`;
}

export function ValueSlot({
  workspaceSlug,
  projectId,
  field,
  condition,
  onChange,
}: ValueSlotProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const selected = selectedValues(condition.value);
  const states = usePickerStates(workspaceSlug, projectId, field.stem === "state_id");
  const staticOptions = field.staticValues ?? [];
  const options: ComboboxOption[] = React.useMemo(() => {
    if (field.stem === "state_id") {
      return selectStateOptions(states.data ?? []).map((option) => ({ id: option.id, label: option.name }));
    }
    return staticOptions.map((option) => ({ id: option.value, label: option.label }));
  }, [field.stem, states.data, staticOptions]);
  const stateLabels = React.useMemo(() => {
    const labels = new Map<string, string>();
    for (const state of selectStateOptions(states.data ?? [])) labels.set(state.id, state.name);
    return labels;
  }, [states.data]);

  function toggle(id: string): void {
    const next = selected.includes(id) ? selected.filter((entry) => entry !== id) : [...selected, id];
    onChange(next.length === 0 ? null : next);
  }

  const loading = field.stem === "state_id" && open && states.isPending;
  const errorText = field.stem === "state_id" && states.isError ? "Couldn't load states." : undefined;

  return (
    <Combobox
      trigger={
        <Button variant="ghost" size="small">
          <span className="max-w-48 truncate">{chosenLabel(field, selected, stateLabels)}</span>
          <ChevronDown size={12} strokeWidth={2} aria-hidden="true" />
        </Button>
      }
      options={options}
      value={selected}
      multiple
      onSelect={toggle}
      listLabel={`${field.label} values`}
      searchLabel={`Search ${field.label.toLowerCase()} values`}
      emptyText="No matching values"
      loading={loading}
      loadingText="Loading values"
      errorText={errorText}
      open={open}
      onOpenChange={setOpen}
    />
  );
}

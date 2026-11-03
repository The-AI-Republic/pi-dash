// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module option shaping (parity/specs/issues.md §17, ISS-213). Project
// modules in server order with selected-first sorting. Single-select
// leads with a No-module clear row; multi-select toggles without one.
// The list stays thin: full module ownership lives in the modules area.

/** One module row in display order. */
export interface ModuleOption {
  id: string;
  name: string;
  selected: boolean;
  /** True for the leading No-module clear row. */
  clear: boolean;
}

export interface ModuleOptionsInput {
  /** Thin rows; only identity and name shape the options. */
  modules: readonly { id: string; name: string }[];
  selectedIds: readonly string[];
  single: boolean;
  allowClear?: boolean | undefined;
}

/** Completed modules render their issues read-only. */
export function isCompletedModule(status: string | undefined): boolean {
  return status !== undefined && status.toLowerCase() === "completed";
}

export function selectModuleOptions(input: ModuleOptionsInput): ModuleOption[] {
  const options = input.modules.map((module) => ({
    id: module.id,
    name: module.name,
    selected: input.selectedIds.includes(module.id),
    clear: false,
  }));
  options.sort((left, right) => {
    if (left.selected && !right.selected) return -1;
    if (right.selected && !left.selected) return 1;
    return 0;
  });
  if (input.single && (input.allowClear ?? true)) {
    options.unshift({ id: "", name: "No module", selected: false, clear: true });
  }
  return options;
}

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module picker (parity/specs/issues.md §17, ISS-213). Single-select
// with a No-module clear row, or multi-select that stays open across
// picks with a counted trigger; callers wanting chips render
// ModuleChips. Search, selected-first sorting, no inline module
// creation. Reads resolve lazily on open.
import { Button, Combobox, IconButton, type ButtonVariant, type ComboboxOption } from "@pidash/kit";
import { Package, X } from "lucide-react";
import * as React from "react";

import { selectModuleOptions, type ModuleOption } from "./modules.js";
import { usePickerModules } from "./reference.js";

interface ModulePickerBase {
  workspaceSlug: string;
  projectId: string;
  /** False hides the single-mode No-module clear row. */
  allowClear?: boolean;
  disabled?: boolean;
  /** Button variant of the default trigger. */
  variant?: ButtonVariant;
  /** Default trigger shows the glyph without the label. */
  iconOnly?: boolean;
  /** Fully custom trigger; overrides the default value button. */
  trigger?: React.ReactElement<Record<string, unknown>>;
  /** Default trigger text when nothing is selected. */
  placeholder?: string;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}

export type ModulePickerProps = ModulePickerBase &
  (
    | { multiple: true; value: readonly string[]; onChange: (ids: string[]) => void }
    | { multiple?: false | undefined; value: string | null; onChange: (id: string | null) => void }
  );

function ModuleGlyph(): React.ReactElement {
  return <Package size={16} strokeWidth={1.5} className="text-(--text-muted)" aria-hidden="true" />;
}

function toComboboxOption(option: ModuleOption): ComboboxOption {
  return {
    id: option.id,
    label: option.name,
    icon: <ModuleGlyph />,
  };
}

/** One removable chip; without onRemove the chips render static. */
export interface ModuleChipsProps {
  modules: ReadonlyArray<{ id: string; name: string }>;
  onRemove?: ((id: string) => void) | undefined;
  disabled?: boolean;
}

export function ModuleChips({ modules, onRemove, disabled = false }: ModuleChipsProps): React.ReactElement {
  return (
    <span className="flex flex-wrap items-center gap-(--space-2)">
      {modules.map((module) => (
        <span
          key={module.id}
          className="text-caption inline-flex items-center gap-(--space-1) rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px text-(--text)"
        >
          <span className="truncate">{module.name}</span>
          {onRemove === undefined ? null : (
            <button
              type="button"
              aria-label={`Remove ${module.name}`}
              disabled={disabled}
              onClick={() => {
                onRemove(module.id);
              }}
              className="inline-flex items-center rounded-(--radius-full) text-(--text-muted) hover:text-(--text) disabled:cursor-not-allowed"
            >
              <X size={12} strokeWidth={1.5} aria-hidden="true" />
            </button>
          )}
        </span>
      ))}
    </span>
  );
}

export function ModulePicker(props: ModulePickerProps): React.ReactElement {
  const {
    workspaceSlug,
    projectId,
    allowClear = true,
    disabled = false,
    variant = "secondary",
    iconOnly = false,
    trigger,
    placeholder = "No module",
    open: controlledOpen,
    onOpenChange,
  } = props;
  const multiple = props.multiple ?? false;
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const open = controlledOpen ?? uncontrolledOpen;

  function setOpen(next: boolean): void {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }

  const modules = usePickerModules(workspaceSlug, projectId, open && !disabled);
  const options = React.useMemo(
    () =>
      selectModuleOptions({
        modules: modules.data ?? [],
        selectedIds: props.multiple ? props.value : props.value === null ? [] : [props.value],
        single: !multiple,
        allowClear,
      }),
    [modules.data, multiple, allowClear, props.multiple, props.value]
  );
  const loading = open && !disabled && modules.isPending;
  const errorText = modules.isError ? "Couldn't load modules." : undefined;

  function handleSelect(id: string): void {
    if (props.multiple) {
      props.onChange(props.value.includes(id) ? props.value.filter((entry) => entry !== id) : [...props.value, id]);
    } else {
      props.onChange(id === "" ? null : id);
    }
  }

  function defaultTrigger(): React.ReactElement<Record<string, unknown>> {
    if (!props.multiple) {
      const current = props.value === null ? undefined : options.find((option) => option.id === props.value);
      const text = current?.name ?? placeholder;
      if (iconOnly) {
        return (
          <IconButton label={text} variant={variant}>
            <ModuleGlyph />
          </IconButton>
        );
      }
      return (
        <Button variant={variant}>
          <ModuleGlyph />
          <span className="truncate">{text}</span>
        </Button>
      );
    }
    const selected = props.value.map((id) => options.find((option) => option.id === id));
    const known = selected.filter((option) => option !== undefined);
    const text =
      props.value.length === 0
        ? placeholder
        : known.length === props.value.length && props.value.length === 1
          ? known[0]!.name
          : props.value.length === 1
            ? "1 module"
            : `${props.value.length} Modules`;
    if (iconOnly) {
      return (
        <IconButton label={text} variant={variant}>
          <ModuleGlyph />
        </IconButton>
      );
    }
    return (
      <Button variant={variant}>
        <ModuleGlyph />
        <span className="truncate">{text}</span>
      </Button>
    );
  }

  return (
    <Combobox
      trigger={trigger ?? defaultTrigger()}
      options={options.map(toComboboxOption)}
      value={props.value}
      multiple={multiple}
      onSelect={handleSelect}
      listLabel="Modules"
      searchLabel="Search modules"
      emptyText="No matching results"
      loading={loading}
      loadingText="Loading modules"
      errorText={errorText}
      disabled={disabled}
      open={open}
      onOpenChange={setOpen}
    />
  );
}

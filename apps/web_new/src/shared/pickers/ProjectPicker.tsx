// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project picker (parity/specs/issues.md §17, ISS-211). Single- or
// multi-select over the joined workspace projects with logo tiles,
// name search and selected-first sorting. The current project stays
// listed unless the caller excludes it. Multi-select stays open across
// picks and counts the trigger. Reads resolve lazily on open.
import { Button, Combobox, IconButton, type ButtonVariant, type ComboboxOption } from "@pidash/kit";
import { Layers } from "lucide-react";
import * as React from "react";

import { selectProjectMark, selectProjectOptions, type ProjectOption } from "./projects.js";
import { usePickerProjects } from "./reference.js";

interface ProjectPickerBase {
  workspaceSlug: string;
  /** Projects hidden from the list (the modal excludes nothing). */
  excludeProjectIds?: readonly string[];
  disabled?: boolean;
  /** Button variant of the default trigger. */
  variant?: ButtonVariant;
  /** Default trigger shows logo tiles without names. */
  iconOnly?: boolean;
  /** Fully custom trigger; overrides the default value button. */
  trigger?: React.ReactElement<Record<string, unknown>>;
  /** Default trigger text when nothing is selected. */
  placeholder?: string;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}

export type ProjectPickerProps = ProjectPickerBase &
  (
    | { multiple: true; value: readonly string[]; onChange: (ids: string[]) => void }
    | { multiple?: false | undefined; value: string | null; onChange: (id: string) => void }
  );

function ProjectMark({ option }: { option: ProjectOption }): React.ReactElement {
  return (
    <span
      aria-hidden="true"
      className="text-meta flex size-6 shrink-0 items-center justify-center rounded-(--radius-control) bg-(--subtle) font-medium text-(--text-muted)"
    >
      {selectProjectMark(option.logoProps, option.identifier)}
    </span>
  );
}

function toComboboxOption(option: ProjectOption): ComboboxOption {
  return {
    id: option.id,
    label: option.name,
    searchText: option.searchText,
    icon: <ProjectMark option={option} />,
  };
}

export function ProjectPicker(props: ProjectPickerProps): React.ReactElement {
  const {
    workspaceSlug,
    excludeProjectIds,
    disabled = false,
    variant = "secondary",
    iconOnly = false,
    trigger,
    placeholder,
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

  const projects = usePickerProjects(workspaceSlug, open && !disabled);
  const options = React.useMemo(
    () =>
      selectProjectOptions({
        projects: projects.data ?? [],
        excludeIds: excludeProjectIds,
        selectedIds: props.multiple ? props.value : props.value === null ? [] : [props.value],
      }),
    [projects.data, excludeProjectIds, props.multiple, props.value]
  );
  const loading = open && !disabled && projects.isPending;
  const errorText = projects.isError ? "Couldn't load projects." : undefined;

  function handleSelect(id: string): void {
    if (props.multiple) {
      props.onChange(props.value.includes(id) ? props.value.filter((entry) => entry !== id) : [...props.value, id]);
    } else {
      props.onChange(id);
    }
  }

  function defaultTrigger(): React.ReactElement<Record<string, unknown>> {
    const prompt = placeholder ?? (multiple ? "Select projects" : "Select project");
    if (!props.multiple) {
      const current = props.value === null ? undefined : options.find((option) => option.id === props.value);
      const text = current?.name ?? prompt;
      if (iconOnly) {
        return (
          <IconButton label={text} variant={variant}>
            {current === undefined ? (
              <Layers size={16} strokeWidth={1.5} aria-hidden="true" />
            ) : (
              <ProjectMark option={current} />
            )}
          </IconButton>
        );
      }
      return (
        <Button variant={variant}>
          {current === undefined ? null : <ProjectMark option={current} />}
          <span className="truncate">{text}</span>
        </Button>
      );
    }
    const selected = props.value.map((id) => options.find((option) => option.id === id));
    const known = selected.filter((option) => option !== undefined);
    const text =
      props.value.length === 0
        ? prompt
        : known.length === props.value.length && props.value.length === 1
          ? known[0]!.name
          : props.value.length === 1
            ? "1 project"
            : `${props.value.length} projects`;
    const shown = known.slice(0, 3);
    const extra = props.value.length - shown.length;
    const stack =
      shown.length === 0 ? null : (
        <span className="flex shrink-0 items-center -space-x-2" aria-hidden="true">
          {shown.map((option) => (
            <span key={option.id} className="rounded-(--radius-control) ring-2 ring-(--bg)">
              <ProjectMark option={option} />
            </span>
          ))}
          {extra > 0 ? (
            <span className="text-caption rounded-(--radius-full) bg-(--subtle) px-(--space-2) text-(--text-muted)">
              +{extra}
            </span>
          ) : null}
        </span>
      );
    if (iconOnly) {
      return (
        <IconButton label={text} variant={variant}>
          {stack ?? <Layers size={16} strokeWidth={1.5} aria-hidden="true" />}
        </IconButton>
      );
    }
    return (
      <Button variant={variant}>
        {stack}
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
      listLabel="Projects"
      searchLabel="Search projects"
      emptyText="No matching results"
      loading={loading}
      loadingText="Loading projects"
      errorText={errorText}
      disabled={disabled}
      open={open}
      onOpenChange={setOpen}
    />
  );
}

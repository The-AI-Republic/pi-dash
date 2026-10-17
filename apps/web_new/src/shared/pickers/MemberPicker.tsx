// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Member / assignee picker (parity/specs/issues.md §17, ISS-208).
// Project members join to the workspace directory for names and avatars;
// without a project scope the directory is offered directly. Guests are
// never offered; suspended members render with a pill and stay
// unselectable. The current member reads as "You" inside the options
// and leads the list, followed by the selected members. Multi-select
// stays open across picks and toggles. Reads resolve lazily on open.
import { Avatar, Button, Combobox, IconButton, type ButtonVariant, type ComboboxOption } from "@pidash/kit";
import { User } from "lucide-react";
import * as React from "react";

import { useMe } from "../../core/session/hooks.js";
import { selectMemberOptions, type MemberOption } from "./members.js";
import { usePickerProjectMembers, usePickerWorkspaceMembers } from "./reference.js";

interface MemberPickerBase {
  workspaceSlug: string;
  /** Project scope; absent offers the workspace directory. */
  projectId?: string;
  disabled?: boolean;
  /** Button variant of the default trigger. */
  variant?: ButtonVariant;
  /** Default trigger shows avatars without names. */
  iconOnly?: boolean;
  /** Fully custom trigger; overrides the default value button. */
  trigger?: React.ReactElement<Record<string, unknown>>;
  /** Default trigger text when nothing is selected. */
  placeholder?: string;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}

export type MemberPickerProps = MemberPickerBase &
  (
    | { multiple: true; value: readonly string[]; onChange: (ids: string[]) => void }
    | { multiple?: false | undefined; value: string | null; onChange: (id: string) => void }
  );

function displayNameOf(option: MemberOption): string {
  return option.label === "You" ? option.searchText : option.label;
}

function SuspendedPill(): React.ReactElement {
  return (
    <span className="text-caption rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px text-(--text-muted)">
      Suspended
    </span>
  );
}

function toComboboxOption(option: MemberOption): ComboboxOption {
  const name = displayNameOf(option);
  return {
    id: option.userId,
    label: option.label,
    searchText: option.searchText,
    disabled: option.suspended,
    icon: <Avatar name={name} {...(option.avatarUrl === null ? {} : { src: option.avatarUrl })} size="small" />,
    trailing: option.suspended ? <SuspendedPill /> : undefined,
  };
}

export function MemberPicker(props: MemberPickerProps): React.ReactElement {
  const {
    workspaceSlug,
    projectId,
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

  const members = usePickerProjectMembers(workspaceSlug, projectId ?? "", open && !disabled && projectId !== undefined);
  const directory = usePickerWorkspaceMembers(workspaceSlug, open && !disabled);
  const me = useMe();

  const options = React.useMemo(() => {
    if (directory.data === undefined) return [];
    if (projectId !== undefined && members.data === undefined) return [];
    return selectMemberOptions({
      projectMembers: members.data,
      directory: directory.data,
      meId: me.data?.id ?? null,
      selectedIds: props.multiple ? props.value : props.value === null ? [] : [props.value],
    });
  }, [directory.data, members.data, projectId, me.data?.id, props.multiple, props.value]);

  const loading =
    (projectId !== undefined && open && !disabled && members.isPending) || (open && !disabled && directory.isPending);
  const errorText = members.isError || directory.isError ? "Couldn't load members." : undefined;

  function handleSelect(id: string): void {
    if (props.multiple) {
      props.onChange(props.value.includes(id) ? props.value.filter((entry) => entry !== id) : [...props.value, id]);
    } else {
      props.onChange(id);
    }
  }

  function defaultTrigger(): React.ReactElement<Record<string, unknown>> {
    const prompt = placeholder ?? (multiple ? "Select members" : "Select member");
    if (!props.multiple) {
      const current = props.value === null ? undefined : options.find((option) => option.userId === props.value);
      const name = current === undefined ? null : displayNameOf(current);
      // A set-but-unknown value renders the prompt until the first open
      // loads the directory; every single picker shares this fallback.
      const text = name ?? prompt;
      if (iconOnly) {
        return (
          <IconButton label={name ?? prompt} variant={variant}>
            {current === undefined ? (
              <User size={16} strokeWidth={1.5} aria-hidden="true" />
            ) : (
              <Avatar
                name={name ?? prompt}
                {...(current.avatarUrl === null ? {} : { src: current.avatarUrl })}
                size="small"
              />
            )}
          </IconButton>
        );
      }
      return (
        <Button variant={variant}>
          {current === undefined ? null : (
            <span aria-hidden="true" className="inline-flex shrink-0 items-center">
              <Avatar
                name={name ?? prompt}
                {...(current.avatarUrl === null ? {} : { src: current.avatarUrl })}
                size="small"
              />
            </span>
          )}
          <span className="truncate">{text}</span>
        </Button>
      );
    }
    const selected = props.value.map((id) => options.find((option) => option.userId === id));
    const known = selected.filter((option) => option !== undefined);
    const names = known.map(displayNameOf);
    const text =
      props.value.length === 0
        ? prompt
        : known.length === props.value.length && props.value.length <= 2
          ? names.join(", ")
          : props.value.length === 1
            ? "1 member"
            : `${props.value.length} members`;
    const shown = known.slice(0, 3);
    const extra = props.value.length - shown.length;
    const stack =
      shown.length === 0 ? null : (
        <span className="flex shrink-0 items-center -space-x-2" aria-hidden="true">
          {shown.map((option) => (
            <Avatar
              key={option.userId}
              name={displayNameOf(option)}
              {...(option.avatarUrl === null ? {} : { src: option.avatarUrl })}
              size="small"
              className="ring-2 ring-(--bg)"
            />
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
          {stack ?? <User size={16} strokeWidth={1.5} aria-hidden="true" />}
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
      listLabel="Members"
      searchLabel="Search members"
      emptyText="No matching results"
      loading={loading}
      loadingText="Loading members"
      errorText={errorText}
      disabled={disabled}
      open={open}
      onOpenChange={setOpen}
    />
  );
}

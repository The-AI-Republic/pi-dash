// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rich-filter value slot (parity/specs/issues.md §9, ISS-082–088).
// The slot adapts to the field's source: workflow states with their group
// glyph, the fixed state-group and priority sets, project members with
// avatars for assignee/creator/mentions, and project labels with colored
// dots. Multi-select slots stay open across picks and clear a re-picked
// value. Single/date editors arrive with ISS-100–103.

import { Avatar, Button, Combobox, type ComboboxOption } from "@pidash/kit";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import { useMe } from "../../../../core/session/hooks.js";
import {
  PRIORITY_ICONS,
  StateGlyph,
  STATE_GROUP_ICONS,
  selectMemberOptions,
  selectStateOptions,
  usePickerProjectMembers,
  usePickerStates,
  usePickerWorkspaceMembers,
  type StateGroup,
} from "../../../../shared/pickers/index.js";
import { useLabels } from "../../labels/api.js";
import type { RichCondition, RichFilterField } from "./expression.js";
import { orderLabelOptions, orderMemberOptions, selectedIds } from "./filterValues.js";

export interface ValueSlotProps {
  workspaceSlug: string;
  projectId: string;
  field: RichFilterField;
  condition: RichCondition;
  onChange: (value: unknown) => void;
}

/** Selected values as a string array; a lone stored value reads as one pick. */
export function selectedValues(value: unknown): string[] {
  return selectedIds(value);
}

function chosenLabel(field: RichFilterField, values: readonly string[], labels: ReadonlyMap<string, string>): string {
  if (values.length === 0) return "Select value";
  const names = values.map((value) => labels.get(value) ?? value);
  if (names.length <= 2) return names.join(", ");
  return `${names.length} selected`;
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
  // A stored condition must resolve its names before the popup opens, so
  // reads stay enabled while ids are selected as well as while open.
  const readValues = open || selected.length > 0;
  const states = usePickerStates(workspaceSlug, projectId, field.source === "states" && readValues);
  const members = usePickerProjectMembers(workspaceSlug, projectId, field.source === "members" && readValues);
  const directory = usePickerWorkspaceMembers(workspaceSlug, field.source === "members" && readValues);
  const labels = useLabels(workspaceSlug, projectId, field.source === "labels" && readValues);
  const me = useMe();
  const meId = me.data?.id ?? null;

  // Applied states lead and the rest keep server order (ISS-082).
  const stateOptions = React.useMemo(() => {
    if (field.source !== "states") return [];
    const chosenSet = new Set(selected);
    const options = selectStateOptions(states.data ?? []);
    return [
      ...options.filter((option) => chosenSet.has(option.id)),
      ...options.filter((option) => !chosenSet.has(option.id)),
    ];
  }, [field.source, selected, states.data]);

  const memberOptions = React.useMemo(() => {
    if (field.source !== "members" || directory.data === undefined || members.data === undefined) return [];
    return orderMemberOptions(
      selectMemberOptions({
        projectMembers: members.data,
        directory: directory.data,
        meId,
        selectedIds: selected,
      }),
      selected,
      meId
    );
  }, [directory.data, field.source, meId, members.data, selected]);

  const labelOptions = React.useMemo(
    () => (field.source === "labels" ? orderLabelOptions(labels.data ?? [], selected) : []),
    [field.source, labels.data, selected]
  );

  const labelsById = React.useMemo(() => {
    const map = new Map<string, string>();
    for (const state of stateOptions) map.set(state.id, state.name);
    for (const member of memberOptions) map.set(member.userId, member.label);
    for (const label of labelOptions) map.set(label.id, label.name);
    for (const option of field.staticValues ?? []) map.set(option.value, option.label);
    return map;
  }, [field.staticValues, labelOptions, memberOptions, stateOptions]);

  const options: ComboboxOption[] = React.useMemo(() => {
    if (field.source === "states") {
      return stateOptions.map((option) => ({
        id: option.id,
        label: option.name,
        searchText: `${option.name} ${option.group}`,
        icon: <StateGlyph option={option} />,
      }));
    }
    if (field.source === "members") {
      return memberOptions.map((option) => ({
        id: option.userId,
        label: option.label,
        searchText: option.searchText,
        disabled: option.suspended,
        icon: (
          <Avatar
            name={option.searchText}
            {...(option.avatarUrl === null ? {} : { src: option.avatarUrl })}
            size="small"
          />
        ),
      }));
    }
    if (field.source === "labels") {
      return labelOptions.map((label) => ({
        id: label.id,
        label: label.name,
        icon: (
          <span
            aria-hidden="true"
            className="inline-block size-2.5 shrink-0 rounded-(--radius-full)"
            style={{ backgroundColor: label.color }}
          />
        ),
      }));
    }
    return (field.staticValues ?? []).map((option) => {
      const Icon =
        field.stem === "state_group"
          ? STATE_GROUP_ICONS[option.value as StateGroup]
          : field.stem === "priority"
            ? PRIORITY_ICONS[option.value as keyof typeof PRIORITY_ICONS]
            : undefined;
      return {
        id: option.value,
        label: option.label,
        icon: Icon === undefined ? undefined : <Icon size={14} strokeWidth={1.5} aria-hidden="true" />,
      };
    });
  }, [field.source, field.staticValues, field.stem, labelOptions, memberOptions, stateOptions]);

  function toggle(id: string): void {
    const next = selected.includes(id) ? selected.filter((entry) => entry !== id) : [...selected, id];
    onChange(next.length === 0 ? null : next);
  }

  const loading =
    open &&
    ((field.source === "states" && states.isPending) ||
      (field.source === "members" && (members.isPending || directory.isPending)) ||
      (field.source === "labels" && labels.isPending));
  const failed =
    (field.source === "states" && states.isError) ||
    (field.source === "members" && (members.isError || directory.isError)) ||
    (field.source === "labels" && labels.isError);
  const errorText = failed ? `Couldn't load ${field.label.toLowerCase()} values.` : undefined;

  return (
    <Combobox
      trigger={
        <Button variant="ghost" size="small">
          <span className="max-w-48 truncate">{chosenLabel(field, selected, labelsById)}</span>
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

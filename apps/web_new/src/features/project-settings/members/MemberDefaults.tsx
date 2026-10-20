// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project defaults on the Members tab (parity/specs/project-settings-members.md
// §"Project defaults"; PSET-026). Lead and default-assignee pickers list the
// current project members who are not guests and offer a None choice that
// clears the slot; every pick or clear saves through the project update at
// once. The guest-visibility switch flips the project flag the same way.
// Non-admins see all three controls, disabled.

import { Button, Combobox, Skeleton, Switch, cn, type ComboboxOption } from "@pidash/kit";
import * as React from "react";

import type { ProjectDetail } from "@pidash/api-client";

import { DEFAULTS_NONE_LABEL, defaultSlotLabel, type DefaultMemberOption } from "./writes.js";

export const LEAD_FIELD_LABEL = "Project lead";
export const ASSIGNEE_FIELD_LABEL = "Default assignee";
export const GUEST_VISIBILITY_LABEL = "Guests can view all work items";

export interface MemberDefaultsProps {
  project: ProjectDetail | undefined;
  members: readonly DefaultMemberOption[];
  /** Admin standing: everyone else gets the same controls, disabled. */
  canEdit: boolean;
  pending: boolean;
  onSave: (patch: {
    project_lead?: string | null;
    default_assignee?: string | null;
    guest_view_all_features?: boolean;
  }) => void;
}

function slotOptions(members: readonly DefaultMemberOption[]): ComboboxOption[] {
  return [
    { id: "none", label: DEFAULTS_NONE_LABEL },
    ...[...members]
      .sort((a, b) => a.label.toLowerCase().localeCompare(b.label.toLowerCase()))
      .map((member) => ({ id: member.userId, label: member.label })),
  ];
}

function SlotPicker({
  label,
  value,
  options,
  disabled,
  onPick,
  testId,
}: {
  label: string;
  value: string | null;
  options: ComboboxOption[];
  disabled: boolean;
  onPick: (userId: string | null) => void;
  testId: string;
}): React.ReactElement {
  return (
    <Combobox
      trigger={
        <Button
          variant="secondary"
          aria-label={label}
          aria-haspopup="listbox"
          disabled={disabled}
          data-testid={testId}
          className={cn("w-56 justify-between", value === null && "text-(--text-muted)")}
        >
          {defaultSlotLabel(
            options
              .filter((option) => option.id !== "none")
              .map((option) => ({ userId: option.id, label: option.label })),
            value
          )}
        </Button>
      }
      options={options}
      value={value ?? "none"}
      onSelect={(id) => onPick(id === "none" ? null : id)}
      listLabel={label}
      searchEnabled={false}
      emptyText="No members to choose from"
      disabled={disabled}
      className="w-64"
    />
  );
}

export function MemberDefaults({
  project,
  members,
  canEdit,
  pending,
  onSave,
}: MemberDefaultsProps): React.ReactElement {
  const options = React.useMemo(() => slotOptions(members), [members]);
  // Each control reads its own saved value; a missing project row keeps the
  // loaders up rather than guessing defaults.
  if (project === undefined) {
    return (
      <section aria-label="Project defaults" data-testid="members-defaults" className="flex flex-col gap-(--space-3)">
        <Skeleton height="20px" width="40%" />
        <Skeleton height="32px" width="14rem" />
      </section>
    );
  }
  const disabled = !canEdit || pending;
  return (
    <section aria-label="Project defaults" data-testid="members-defaults" className="flex flex-col gap-(--space-3)">
      <div className="flex flex-wrap items-center gap-(--space-3)">
        <SlotPicker
          label={LEAD_FIELD_LABEL}
          value={project.project_lead}
          options={options}
          disabled={disabled}
          onPick={(userId) => onSave({ project_lead: userId })}
          testId="members-lead"
        />
        <SlotPicker
          label={ASSIGNEE_FIELD_LABEL}
          value={project.default_assignee ?? null}
          options={options}
          disabled={disabled}
          onPick={(userId) => onSave({ default_assignee: userId })}
          testId="members-assignee"
        />
      </div>
      <label className="text-body flex items-center gap-(--space-3) text-(--text)">
        <Switch
          label={GUEST_VISIBILITY_LABEL}
          checked={project.guest_view_all_features}
          disabled={disabled}
          onCheckedChange={(checked) => onSave({ guest_view_all_features: checked })}
          data-testid="members-guest-visibility"
        />
        <span>{GUEST_VISIBILITY_LABEL}</span>
      </label>
    </section>
  );
}

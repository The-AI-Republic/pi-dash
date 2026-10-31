// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The General tab's identity fields (parity/specs/project-settings-general.md
// §PSET-006): name, description, short code and base branch, each with its
// own rule and inline message. Visibility and timezone are pickers, so they
// live with their own controls in the form.

import { Button, Combobox, Input, Textarea } from "@pidash/kit";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import {
  DESCRIPTION_PLACEHOLDER,
  GENERAL_IDENTIFIER_MAX_LENGTH,
  GENERAL_NAME_MAX_LENGTH,
  VISIBILITY_OPTIONS,
  type GeneralFieldErrors,
  type GeneralFormValues,
} from "../rules.js";

export interface GeneralDetailsFieldsProps {
  values: GeneralFormValues;
  errors: GeneralFieldErrors;
  disabled: boolean;
  /** Identifier availability answer: true while it is still being asked. */
  checkingIdentifier: boolean;
  /** Timezone choices; UTC leads and the stored zone is always present. */
  timezones: ReadonlyArray<{ id: string; label: string; searchText: string }>;
  timezonesLoading: boolean;
  timezonesFailed: boolean;
  onChange: (patch: Partial<GeneralFormValues>) => void;
}

export function GeneralDetailsFields({
  values,
  errors,
  disabled,
  checkingIdentifier,
  timezones,
  timezonesLoading,
  timezonesFailed,
  onChange,
}: GeneralDetailsFieldsProps): React.ReactElement {
  const identifierHint = checkingIdentifier
    ? "Checking availability…"
    : `Up to ${GENERAL_IDENTIFIER_MAX_LENGTH} letters and numbers`;
  return (
    <div className="grid grid-cols-1 gap-(--space-6) md:grid-cols-2">
      <div className="flex flex-col gap-(--space-6)">
        <Input
          id="name"
          name="name"
          label="Project name"
          value={values.name}
          maxLength={GENERAL_NAME_MAX_LENGTH}
          disabled={disabled}
          autoComplete="off"
          {...(errors.name === undefined ? {} : { error: errors.name })}
          onChange={(event) => onChange({ name: event.target.value })}
        />
        <Input
          id="identifier"
          name="identifier"
          label="Short code"
          value={values.identifier}
          disabled={disabled}
          autoComplete="off"
          hint={identifierHint}
          data-testid="general-identifier"
          {...(errors.identifier === undefined ? {} : { error: errors.identifier })}
          onChange={(event) => onChange({ identifier: event.target.value })}
        />
      </div>
      <div className="flex flex-col gap-(--space-6)">
        <div className="flex flex-col gap-(--space-2)">
          <span className="text-body font-medium text-(--text)">Visibility</span>
          <Combobox
            trigger={
              <Button
                variant="secondary"
                disabled={disabled}
                data-testid="general-visibility-trigger"
                className="w-full justify-between"
              >
                <span data-testid="general-visibility">{visibilityOptionLabel(values.network)}</span>
                <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
              </Button>
            }
            options={VISIBILITY_OPTIONS.map((option) => ({ id: String(option.value), label: option.label }))}
            value={String(values.network)}
            onSelect={(value) => onChange({ network: Number(value) })}
            listLabel="Visibility"
            searchEnabled={false}
            emptyText="No visibility options"
            disabled={disabled}
          />
        </div>
        <div className="flex flex-col gap-(--space-2)">
          <span className="text-body font-medium text-(--text)">Timezone</span>
          <Combobox
            trigger={
              <Button
                variant="secondary"
                disabled={disabled || timezonesFailed}
                data-testid="general-timezone-trigger"
                className="w-full justify-between"
              >
                <span data-testid="general-timezone" className="truncate">
                  {values.timezone}
                </span>
                <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
              </Button>
            }
            options={timezones.map((zone) => ({ id: zone.id, label: zone.label, searchText: zone.searchText }))}
            value={values.timezone}
            onSelect={(zone) => onChange({ timezone: zone })}
            listLabel="Project timezone"
            searchLabel="Search timezones"
            emptyText="No timezones match"
            loading={timezonesLoading}
            loadingText="Loading timezones"
            disabled={disabled}
          />
        </div>
      </div>
      <div className="md:col-span-2">
        <Textarea
          id="description"
          name="description"
          label="Description"
          placeholder={DESCRIPTION_PLACEHOLDER}
          value={values.description}
          disabled={disabled}
          rows={5}
          onChange={(event) => onChange({ description: event.target.value })}
        />
      </div>
      <Input
        id="baseBranch"
        name="baseBranch"
        label="Base branch"
        value={values.baseBranch}
        disabled={disabled}
        autoComplete="off"
        {...(errors.baseBranch === undefined ? {} : { error: errors.baseBranch })}
        onChange={(event) => onChange({ baseBranch: event.target.value })}
      />
    </div>
  );
}

function visibilityOptionLabel(network: number): string {
  return VISIBILITY_OPTIONS.find((option) => option.value === network)?.label ?? "Private";
}

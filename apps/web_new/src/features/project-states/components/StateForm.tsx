// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline state form (parity/specs/project-settings-states.md, PSET-029,
// PSET-030). One form serves creation and editing: a swatch button that
// opens the group-aware palette, a capped name field with inline
// validation, a description field and save/cancel. The form never closes
// itself on failure; the caller closes it only after a successful save,
// so a duplicate keeps the prefilled fields in front of the operator.

import { Button, Input, Popover, Textarea } from "@pidash/kit";
import * as React from "react";

import { capStateName, STATE_NAME_MAX_LENGTH, validateStateName } from "../validation.js";

/**
 * Swatches offered by the form: the group tints first, then a broader
 * spread for states that should stand out from their neighbours.
 */
export const STATE_FORM_PALETTE = [
  "#9CA3AF",
  "#6B7280",
  "#F59E0B",
  "#3B82F6",
  "#8B5CF6",
  "#10B981",
  "#EF4444",
  "#EC4899",
  "#14B8A6",
  "#EAB308",
  "#6366F1",
  "#64748B",
] as const;

export interface StateFormValue {
  name: string;
  description: string;
  color: string;
}

export interface StateFormProps {
  mode: "create" | "edit";
  groupLabel: string;
  initialName?: string;
  initialDescription?: string;
  initialColor: string;
  pending: boolean;
  onSubmit: (value: StateFormValue) => void;
  onCancel: () => void;
}

export function StateForm({
  mode,
  groupLabel,
  initialName = "",
  initialDescription = "",
  initialColor,
  pending,
  onSubmit,
  onCancel,
}: StateFormProps): React.ReactElement {
  const [name, setName] = React.useState(initialName);
  const [description, setDescription] = React.useState(initialDescription);
  const [color, setColor] = React.useState(initialColor);
  const [error, setError] = React.useState<string | null>(null);
  const [pickerOpen, setPickerOpen] = React.useState(false);

  function submit(): void {
    const problem = validateStateName(name);
    if (problem !== null) {
      setError(problem);
      return;
    }
    onSubmit({ name, description, color });
  }

  return (
    <form
      data-testid="state-form"
      aria-label={mode === "create" ? `Create state in ${groupLabel}` : `Edit state ${initialName}`}
      className="flex flex-col gap-(--space-3) rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-4)"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <div className="flex items-start gap-(--space-3)">
        <Popover
          trigger={
            <button
              type="button"
              data-testid="state-form-swatch"
              aria-label="Pick a state color"
              aria-expanded={pickerOpen}
              disabled={pending}
              style={{ backgroundColor: color }}
              className="mt-(--space-6) size-6 shrink-0 cursor-pointer rounded-full border border-(--border) disabled:cursor-not-allowed"
            />
          }
          title="State color"
          open={pickerOpen}
          onOpenChange={setPickerOpen}
        >
          <div role="group" aria-label="Pick a color" className="grid max-w-56 grid-cols-6 gap-(--space-2)">
            {STATE_FORM_PALETTE.map((swatch) => (
              <button
                key={swatch}
                type="button"
                data-testid="state-swatch"
                data-color={swatch}
                aria-label={swatch}
                aria-pressed={swatch === color}
                style={{ backgroundColor: swatch }}
                onClick={() => {
                  setColor(swatch);
                  setPickerOpen(false);
                }}
                className="size-7 cursor-pointer rounded-full border border-(--border) aria-pressed:ring-2 aria-pressed:ring-(--accent)"
              />
            ))}
          </div>
        </Popover>
        <div className="min-w-0 flex-1">
          <Input
            label="State name"
            data-testid="state-form-name"
            value={name}
            maxLength={STATE_NAME_MAX_LENGTH}
            placeholder="State name"
            autoComplete="off"
            disabled={pending}
            onChange={(event) => {
              setName(capStateName(event.currentTarget.value));
              if (error !== null) setError(null);
            }}
            {...(error === null ? {} : { error })}
          />
        </div>
      </div>
      <Textarea
        label="Description"
        data-testid="state-form-description"
        value={description}
        placeholder="What this state means"
        rows={2}
        disabled={pending}
        onChange={(event) => setDescription(event.currentTarget.value)}
      />
      <div className="flex justify-end gap-(--space-2)">
        <Button variant="ghost" type="button" onClick={onCancel} disabled={pending}>
          Cancel
        </Button>
        <Button variant="primary" type="submit">
          {mode === "create" ? "Create state" : "Save changes"}
        </Button>
      </div>
    </form>
  );
}

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline label form (parity/specs/issues.md §19, ISS-226/227): the
// color dot with its swatch popover plus the validated name field.
// Creates seed a random swatch; edits prefill both fields. Failures
// surface through the caller's mutation notices; the form stays open.

import { Button, Input, Popover } from "@pidash/kit";
import * as React from "react";

import { LABEL_PALETTE, randomLabelColor } from "./palette.js";
import { validateLabelName } from "./validation.js";

export interface LabelFormValue {
  name: string;
  color: string;
}

export interface LabelFormProps {
  mode: "create" | "edit";
  initialName?: string | undefined;
  initialColor?: string | undefined;
  /** Mutation in flight: the fields lock while the save lands. */
  pending: boolean;
  onSubmit: (value: LabelFormValue) => void;
  onCancel: () => void;
}

export function LabelForm({
  mode,
  initialName = "",
  initialColor,
  pending,
  onSubmit,
  onCancel,
}: LabelFormProps): React.ReactElement {
  const [name, setName] = React.useState(initialName);
  const [color, setColor] = React.useState(() => initialColor ?? randomLabelColor());
  const [error, setError] = React.useState<string | null>(null);
  const [pickerOpen, setPickerOpen] = React.useState(false);

  function submit(): void {
    const issue = validateLabelName(name);
    if (issue !== null) {
      setError(issue);
      return;
    }
    onSubmit({ name, color });
  }

  return (
    <form
      data-testid="label-form"
      aria-label={mode === "create" ? "Create label" : "Edit label"}
      className="flex items-start gap-(--space-3) rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-4)"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <Popover
        trigger={
          <button
            type="button"
            data-testid="label-form-dot"
            aria-label="Pick a label color"
            disabled={pending}
            style={{ backgroundColor: color }}
            className="mt-(--space-6) size-6 shrink-0 cursor-pointer rounded-full border border-(--border) disabled:cursor-not-allowed"
          />
        }
        title="Label color"
        open={pickerOpen}
        onOpenChange={setPickerOpen}
      >
        <div role="group" aria-label="Pick a color" className="grid max-w-56 grid-cols-5 gap-(--space-2)">
          {LABEL_PALETTE.map((swatch) => (
            <button
              key={swatch}
              type="button"
              data-testid="label-swatch"
              data-color={swatch}
              aria-label={swatch}
              aria-pressed={swatch === color}
              style={{ backgroundColor: swatch }}
              onClick={() => {
                setColor(swatch);
                setPickerOpen(false);
              }}
              className="size-7 cursor-pointer place-items-center rounded-full border border-(--border) aria-pressed:ring-2 aria-pressed:ring-(--accent)"
            />
          ))}
        </div>
      </Popover>
      <div className="min-w-0 flex-1">
        <Input
          label="Label name"
          data-testid="label-form-name"
          value={name}
          placeholder="Label name"
          autoComplete="off"
          disabled={pending}
          onChange={(event) => {
            setName(event.target.value);
            if (error !== null) setError(null);
          }}
          {...(error === null ? {} : { error })}
        />
      </div>
      <div className="flex shrink-0 items-center gap-(--space-2)">
        <Button variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
        <Button variant="primary" type="submit" loading={pending}>
          {mode === "create" ? "Create label" : "Save changes"}
        </Button>
      </div>
    </form>
  );
}

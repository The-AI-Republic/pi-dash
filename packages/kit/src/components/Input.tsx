// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import * as React from "react";
import { cn } from "../lib/cn";
import { FOCUS_RING } from "../lib/styles";
import { FieldScaffold, fieldInputClasses, fieldInputErrorClasses } from "./FieldScaffold";

export interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  label: string;
  hint?: string;
  error?: string;
}

export function Input({ label, hint, error, id, className, ...rest }: InputProps): React.ReactElement {
  const generatedId = React.useId();
  const fieldId = id ?? generatedId;
  return (
    <FieldScaffold label={label} hint={hint} error={error} fieldId={fieldId}>
      {(describedBy) => (
        <input
          id={fieldId}
          aria-invalid={error ? true : undefined}
          aria-describedby={describedBy}
          className={cn(fieldInputClasses, fieldInputErrorClasses(Boolean(error)), FOCUS_RING, className)}
          {...rest}
        />
      )}
    </FieldScaffold>
  );
}

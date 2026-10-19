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
  /** Control pinned to the input's right edge, e.g. a clear or show/hide button. */
  trailing?: React.ReactNode;
  /** Handle on the underlying input for callers that focus it directly. */
  inputRef?: React.Ref<HTMLInputElement> | undefined;
}

export function Input({
  label,
  hint,
  error,
  id,
  className,
  trailing,
  inputRef,
  ...rest
}: InputProps): React.ReactElement {
  const generatedId = React.useId();
  const fieldId = id ?? generatedId;
  return (
    <FieldScaffold label={label} hint={hint} error={error} fieldId={fieldId}>
      {(describedBy) => (
        // The wrapper stays mounted with or without a trailing control so
        // the field never remounts (and loses focus) when one appears.
        <div className="relative">
          <input
            ref={inputRef}
            id={fieldId}
            aria-invalid={error ? true : undefined}
            aria-describedby={describedBy}
            className={cn(
              fieldInputClasses,
              fieldInputErrorClasses(Boolean(error)),
              FOCUS_RING,
              trailing === undefined ? null : "pr-(--space-12)",
              className
            )}
            {...rest}
          />
          {trailing === undefined ? null : (
            <div className="absolute top-1/2 right-(--space-2) -translate-y-1/2">{trailing}</div>
          )}
        </div>
      )}
    </FieldScaffold>
  );
}

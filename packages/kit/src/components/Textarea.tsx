// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import * as React from "react";
import { cn } from "../lib/cn";
import { FOCUS_RING } from "../lib/styles";
import { FieldScaffold, fieldInputErrorClasses } from "./FieldScaffold";

export interface TextareaProps extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {
  label: string;
  hint?: string;
  error?: string;
}

export function Textarea({ label, hint, error, id, rows = 4, className, ...rest }: TextareaProps): React.ReactElement {
  const generatedId = React.useId();
  const fieldId = id ?? generatedId;
  return (
    <FieldScaffold label={label} hint={hint} error={error} fieldId={fieldId}>
      {(describedBy) => (
        <textarea
          id={fieldId}
          rows={rows}
          aria-invalid={error ? true : undefined}
          aria-describedby={describedBy}
          className={cn(
            "text-body min-h-20 w-full resize-y rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) py-(--space-3) text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none disabled:cursor-not-allowed disabled:opacity-50",
            fieldInputErrorClasses(Boolean(error)),
            FOCUS_RING,
            className
          )}
          {...rest}
        />
      )}
    </FieldScaffold>
  );
}

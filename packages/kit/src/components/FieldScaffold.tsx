// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared label/hint/error scaffolding for text fields. The label is always
// associated, and hint/error copy is wired through aria-describedby.
import * as React from "react";
import { cn } from "../lib/cn";

export interface FieldScaffoldProps {
  label: string;
  hint?: string | undefined;
  error?: string | undefined;
  fieldId: string;
  children: (describedBy: string | undefined) => React.ReactNode;
}

export function FieldScaffold({ label, hint, error, fieldId, children }: FieldScaffoldProps): React.ReactElement {
  const hintId = `${fieldId}-hint`;
  const errorId = `${fieldId}-error`;
  const describedBy = error ? errorId : hint ? hintId : undefined;
  return (
    <div className="flex flex-col gap-(--space-2)">
      <label htmlFor={fieldId} className="text-body font-medium text-(--text)">
        {label}
      </label>
      {children(describedBy)}
      {error ? (
        <p id={errorId} role="alert" className="text-meta text-(--danger)">
          {error}
        </p>
      ) : hint ? (
        <p id={hintId} className="text-meta text-(--text-muted)">
          {hint}
        </p>
      ) : null}
    </div>
  );
}

export const fieldInputClasses =
  "h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-body text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none disabled:cursor-not-allowed disabled:opacity-50";

export function fieldInputErrorClasses(invalid: boolean): string {
  return cn(invalid && "border-(--danger) focus:border-(--danger)");
}

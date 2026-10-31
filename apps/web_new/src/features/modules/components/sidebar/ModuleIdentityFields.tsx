// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The sidebar's text fields (parity/specs/modules.md §9.1, MOD-033).
// The name commits on Enter or blur and refuses an empty value locally;
// the description commits on blur. Both render as plain edges until they
// have focus, so the panel reads like a properties list rather than a
// form. A refusal leaves the typed value in place for a retry; the
// parent's mutation owns the notice and cache rollback.

import { cn } from "@pidash/kit";
import * as React from "react";

/** The pinned success notice for a module field edit (§4). */
export const MODULE_NAME_EMPTY = "A name is required.";

export interface ModuleNameFieldProps {
  value: string;
  disabled: boolean;
  onSave: (name: string) => void;
}

export function ModuleNameField({ value, disabled, onSave }: ModuleNameFieldProps): React.ReactElement {
  const [draft, setDraft] = React.useState(value);
  const [editing, setEditing] = React.useState(false);
  const [invalid, setInvalid] = React.useState(false);

  React.useEffect(() => {
    if (!editing) {
      setDraft(value);
      setInvalid(false);
    }
  }, [editing, value]);

  function commit(): void {
    const next = draft.trim();
    if (next === "") {
      setInvalid(true);
      return;
    }
    setEditing(false);
    if (next !== value) onSave(next);
  }

  return (
    <input
      aria-label="Module name"
      aria-invalid={invalid ? true : undefined}
      data-testid="modules-sidebar-name"
      value={draft}
      disabled={disabled}
      onChange={(event) => {
        setDraft(event.target.value);
        setInvalid(false);
      }}
      onFocus={() => setEditing(true)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          commit();
          event.currentTarget.blur();
        }
        if (event.key === "Escape") {
          setDraft(value);
          setInvalid(false);
          setEditing(false);
          event.currentTarget.blur();
        }
      }}
      className={cn(
        "text-emphasis w-full min-w-0 rounded-(--radius-control) bg-transparent px-(--space-1) py-px font-semibold text-(--text) outline-none",
        "hover:bg-(--subtle) focus:bg-(--subtle) focus:ring-1 focus:ring-(--accent) disabled:cursor-not-allowed disabled:hover:bg-transparent",
        invalid && "ring-1 ring-(--danger)"
      )}
    />
  );
}

export interface ModuleDescriptionFieldProps {
  value: string;
  disabled: boolean;
  onSave: (description: string) => void;
}

export function ModuleDescriptionField({ value, disabled, onSave }: ModuleDescriptionFieldProps): React.ReactElement {
  const [draft, setDraft] = React.useState(value);

  React.useEffect(() => {
    setDraft(value);
  }, [value]);

  return (
    <textarea
      aria-label="Module description"
      data-testid="modules-sidebar-description"
      rows={3}
      value={draft}
      disabled={disabled}
      placeholder="Add a description"
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => {
        if (draft !== value) onSave(draft);
      }}
      className="text-body min-h-16 w-full resize-y rounded-(--radius-control) border border-transparent bg-transparent px-(--space-1) py-(--space-1) text-(--text-muted) outline-none placeholder:text-(--text-muted) hover:bg-(--subtle) focus:border-(--border) focus:bg-(--bg) disabled:cursor-not-allowed disabled:hover:bg-transparent"
    />
  );
}

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One labelled text field for the assistant settings sections. Every field
// in this tab is a visible control with a persistent label; passwords stay
// masked and only ever carry a placeholder saying a key is stored.

import * as React from "react";

export const ASSISTANT_FIELD_CLASSES =
  "h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-body text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none";

export interface TextFieldProps {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  type?: "text" | "password";
  autoComplete?: string;
}

export function TextField({
  id,
  label,
  value,
  onChange,
  placeholder,
  type = "text",
  autoComplete,
}: TextFieldProps): React.ReactElement {
  return (
    <div className="flex flex-col gap-(--space-2)">
      <label htmlFor={id} className="text-body font-medium text-(--text)">
        {label}
      </label>
      <input
        id={id}
        type={type}
        value={value}
        placeholder={placeholder}
        autoComplete={autoComplete}
        className={ASSISTANT_FIELD_CLASSES}
        onChange={(event) => onChange(event.target.value)}
      />
    </div>
  );
}

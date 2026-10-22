// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// View access seam (parity/specs/views.md §5/§14, VIEW-019). The dialog
// mounts this where the shared-vs-owner-only selector belongs. Editions
// that fill the `viewAccess` slot render their own selector; the OSS
// edition leaves the slot absent and nothing renders in its place — no
// disabled control, no placeholder.

import * as React from "react";

import { edition, type ViewAccessValue } from "../../../core/edition/index.js";

export interface ViewAccessFieldProps {
  /** Current choice; the dialog defaults it to shared. */
  value: ViewAccessValue;
  onChange: (access: ViewAccessValue) => void;
  /** True while a submit is in flight; the selector takes no edits. */
  disabled: boolean;
}

/** The model default every create lands on without a stored choice. */
export const SHARED_ACCESS: ViewAccessValue = 1;
/** Visible only to the owner; stored through the slot's own call. */
export const OWNER_ACCESS: ViewAccessValue = 0;

/** The edition's access selector, or nothing when the edition has none. */
export function ViewAccessField({ value, onChange, disabled }: ViewAccessFieldProps): React.ReactElement | null {
  const slot = edition.slots?.viewAccess;
  if (slot === undefined) return null;
  const Field = slot.Field;
  return (
    <div data-testid="view-access-field">
      <Field value={value} onChange={onChange} disabled={disabled} />
    </div>
  );
}

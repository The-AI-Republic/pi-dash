// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Team-size dropdown (AUTH-040). The size picker the standalone
// create-workspace route injects into the shared create form through its
// renderSizeSelector seam: same sizes as the onboarding pills, as a
// native dropdown. Written from parity/specs/auth.md.

import * as React from "react";

import type { TeamSizeSelectorField } from "./WorkspaceCreateForm.js";

export const TEAM_SIZE_DROPDOWN_PLACEHOLDER = "Select a size";

export function TeamSizeDropdown(field: TeamSizeSelectorField): React.ReactElement {
  return (
    <select
      aria-labelledby={field.labelId}
      value={field.value}
      disabled={field.disabled}
      onChange={(event) => field.onSelect(event.target.value)}
      className="text-body rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-3) py-(--space-2) text-(--text)"
    >
      <option value="" disabled>
        {TEAM_SIZE_DROPDOWN_PLACEHOLDER}
      </option>
      {field.options.map((option) => (
        <option key={option} value={option}>
          {option}
        </option>
      ))}
    </select>
  );
}

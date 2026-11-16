// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// First-run zero state for a project without modules (parity/specs/
// modules.md §3.2, MOD-009). Art, a title and an explanation of what a
// module is, plus the same creation dialog the header opens. Members and
// admins get an enabled action; guests and read-only roles still read the
// art and copy but the action stays disabled — one view, no separate guest
// wording. This never stands in for the no-match state, which appears only
// when modules exist but the search or filters hide them.

import { EmptyState } from "@pidash/kit";
import * as React from "react";

export const MODULES_ZERO_TITLE = "Group work into modules";
export const MODULES_ZERO_DESCRIPTION =
  "A module is a named container for related work items, with its own dates, lead and progress. Create the first one to start organizing this project.";
export const MODULES_ZERO_CREATE_LABEL = "Set your first module";

export interface ModulesZeroStateProps {
  /** Admin or member standing: guests get the same view with a disabled action. */
  canCreate: boolean;
  /** Opens the create dialog. */
  onCreate: () => void;
}

export function ModulesZeroState({ canCreate, onCreate }: ModulesZeroStateProps): React.ReactElement {
  return (
    <div data-testid="modules-zero-state">
      <EmptyState
        variant="detailed"
        art={{ category: "general", type: "empty" }}
        title={MODULES_ZERO_TITLE}
        description={MODULES_ZERO_DESCRIPTION}
        primaryAction={{
          label: MODULES_ZERO_CREATE_LABEL,
          onSelect: () => {
            onCreate();
          },
          disabled: !canCreate,
        }}
      />
    </div>
  );
}

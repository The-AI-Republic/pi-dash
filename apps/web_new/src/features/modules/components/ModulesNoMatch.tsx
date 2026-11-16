// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// No-match container for the modules list (parity/specs/modules.md §3.3,
// MOD-010). Renders when modules exist but the search and filters leave
// none visible. It sits inside the content slot, below the header, so the
// search and filter controls that produced it stay available. Clearing
// them restores the untouched module set in its prior order. This is never
// the empty-project state (slice F): that one offers a create action.

import { Search } from "lucide-react";
import * as React from "react";

export const MODULES_NO_MATCH_HEADING = "No modules match";
export const MODULES_NO_MATCH_HINT =
  "No modules match the current search or filters. Adjust the search or filters to see more.";

export function ModulesNoMatch(): React.ReactElement {
  return (
    <div
      data-testid="modules-no-match"
      className="flex flex-col items-center gap-(--space-4) px-(--space-6) py-(--space-16) text-center"
    >
      <span
        aria-hidden="true"
        className="grid size-12 place-items-center rounded-full border border-(--border) bg-(--subtle) text-(--text-muted)"
      >
        <Search size={20} strokeWidth={1.5} />
      </span>
      <h3 data-testid="modules-no-match-heading" className="text-h3 text-(--text)">
        {MODULES_NO_MATCH_HEADING}
      </h3>
      <p data-testid="modules-no-match-hint" className="text-body max-w-96 text-(--text-muted)">
        {MODULES_NO_MATCH_HINT}
      </p>
    </div>
  );
}

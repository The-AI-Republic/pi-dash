// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// No-match state (parity/specs/views.md §3, VIEW-010). Shows when views
// exist but the name search and list filters exclude every one; the
// underlying list is untouched. The search and filters that produce the
// empty set land with NEWFRONT-720.

import { EmptyState } from "@pidash/kit";
import { Search } from "lucide-react";
import * as React from "react";

export function ProjectViewsNoMatch(): React.ReactElement {
  return (
    <div data-testid="project-views-no-match">
      <EmptyState
        variant="section"
        icon={<Search size={16} strokeWidth={1.5} aria-hidden="true" />}
        title="No matching results."
        description="No views match the current search. Try adjusting the search terms."
      />
    </div>
  );
}

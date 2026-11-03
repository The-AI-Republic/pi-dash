// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline list-read failure (parity/specs/views.md §1). Every list read
// renders an error with a retry; the views list is no exception.

import { EmptyState } from "@pidash/kit";
import { CloudOff } from "lucide-react";
import * as React from "react";

export interface ProjectViewsErrorProps {
  onRetry: () => void;
}

export function ProjectViewsError({ onRetry }: ProjectViewsErrorProps): React.ReactElement {
  return (
    <div data-testid="project-views-error">
      <EmptyState
        variant="section"
        icon={<CloudOff size={16} strokeWidth={1.5} aria-hidden="true" />}
        title="Views didn't load"
        description="Check your connection and try again."
        action={{ label: "Retry", onSelect: () => onRetry() }}
      />
    </div>
  );
}

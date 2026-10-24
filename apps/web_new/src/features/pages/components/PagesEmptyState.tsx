// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Empty pages collection (parity/specs/pages.md §3.7, PAGE-019). The
// archived tab cannot be created into, so it carries no action. The
// ordinary empty state's action is disabled for a guest by its caller.

import { EmptyState } from "@pidash/kit";
import * as React from "react";

export interface PagesEmptyStateProps {
  archived: boolean;
  canCreate: boolean;
  onCreate: () => void;
  /** Failed reads keep the create action enabled (§2.7). */
  readFailed?: boolean;
}

export function PagesEmptyState({
  archived,
  canCreate,
  onCreate,
  readFailed = false,
}: PagesEmptyStateProps): React.ReactElement {
  return (
    <div data-testid="pages-empty-state">
      <EmptyState
        variant="detailed"
        art={{ category: "general", type: "empty" }}
        title={archived ? "No archived pages yet" : "No pages in this project yet"}
        description={
          archived
            ? "Pages you archive will be kept here."
            : "Write a page to capture context, plans and decisions beside your work."
        }
        {...(archived
          ? {}
          : {
              primaryAction: {
                label: "Create your first Page",
                onSelect: onCreate,
                disabled: readFailed ? false : !canCreate,
              },
            })}
      />
    </div>
  );
}

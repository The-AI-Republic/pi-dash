// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List empty states (parity/specs/intake.md INT-008, bug NEWFRONT-262).
// One presentation per situation: filters that match nothing, an
// untouched Open tab, and an untouched Closed tab. Only the Open
// guidance carries a create action; the informational Closed state has
// none. Every state renders a heading so the reason is readable without
// the surrounding chrome.

import { Button } from "@pidash/kit";
import * as React from "react";

import type { IntakeEmptyState as IntakeEmptyKind } from "../lib/emptyStates.js";

export interface IntakeEmptyStateProps {
  kind: IntakeEmptyKind;
  /** Opens the create dialog (INT-013, slice C). */
  onCreate?: (() => void) | undefined;
}

export function IntakeEmptyState({ kind, onCreate }: IntakeEmptyStateProps): React.ReactElement {
  if (kind === "filtered") {
    return (
      <div data-testid="intake-empty-filtered" className="flex flex-col gap-(--space-2) p-(--space-6)">
        <h3 className="text-emphasis text-(--text)">No matching results.</h3>
        <p className="text-body text-(--text-muted)">
          No request matches the filters applied to this list. Clear a filter above the list to see more requests.
        </p>
      </div>
    );
  }
  if (kind === "open-guidance") {
    return (
      <div data-testid="intake-empty-open" className="flex flex-col gap-(--space-3) p-(--space-6)">
        <h3 className="text-emphasis text-(--text)">No requests waiting for triage.</h3>
        <p className="text-body text-(--text-muted)">
          Requests that arrive for this project land here until someone accepts, declines or snoozes them.
        </p>
        <div>
          <Button
            variant="primary"
            size="small"
            data-testid="intake-empty-create"
            disabled={onCreate === undefined}
            onClick={() => {
              onCreate?.();
            }}
          >
            Create request
          </Button>
        </div>
      </div>
    );
  }
  return (
    <div data-testid="intake-empty-closed" className="flex flex-col gap-(--space-2) p-(--space-6)">
      <h3 className="text-emphasis text-(--text)">Nothing has been resolved yet.</h3>
      <p className="text-body text-(--text-muted)">
        Requests appear here once they are accepted, declined or marked as a duplicate.
      </p>
    </div>
  );
}

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Missing-view state (parity/specs/views.md §7, VIEW-022). One state for a
// view that is absent, owner-only to someone else, refused by the server or
// returned blank: the record's identity is what makes a view renderable, so
// the detail never tries to guess which failure it saw. The header chrome
// is absent here; the only exit is back to the project's views list.

import { Button, emptyStateImagePath } from "@pidash/kit";
import * as React from "react";

import { MISSING_VIEW_BACK_LABEL, MISSING_VIEW_DESCRIPTION, MISSING_VIEW_TITLE } from "./model.js";

export interface SavedViewMissingStateProps {
  onBack: () => void;
  /** Way-back label; the workspace detail names the default view (VIEW-038). */
  backLabel?: string | undefined;
}

export function SavedViewMissingState({
  onBack,
  backLabel = MISSING_VIEW_BACK_LABEL,
}: SavedViewMissingStateProps): React.ReactElement {
  return (
    <section data-testid="saved-view-missing" className="flex min-h-0 flex-1 items-center justify-center p-(--space-8)">
      <div className="flex w-full max-w-96 flex-col items-center gap-(--space-6) px-(--space-6) py-(--space-12)">
        <h3 className="text-h3 text-center text-(--text)">{MISSING_VIEW_TITLE}</h3>
        <img
          src={emptyStateImagePath({ category: "general", type: "empty" })}
          alt=""
          aria-hidden="true"
          draggable={false}
          className="h-36 w-36"
        />
        <p className="text-body text-center text-(--text-muted)">{MISSING_VIEW_DESCRIPTION}</p>
        <Button variant="primary" onClick={() => onBack()}>
          {backLabel}
        </Button>
      </div>
    </section>
  );
}

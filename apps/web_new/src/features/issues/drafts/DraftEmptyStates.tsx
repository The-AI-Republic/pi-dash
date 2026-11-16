// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Drafts empty states (parity/specs/issues.md §12, ISS-141). Two distinct
// first-run screens: with no drafts the page invites half-written work
// items; with no project joined it instead sends the viewer to create one.
// Both actions carry the same member-or-better gate, and a guest sees the
// action disabled rather than missing.

import { Button, EmptyState } from "@pidash/kit";
import { FilePen } from "lucide-react";
import * as React from "react";

export const NO_DRAFTS_TITLE = "Half-written work items";
export const NO_DRAFTS_BODY =
  "To try this out, start adding a work item and leave it mid-way or create your first draft below.";
export const NO_DRAFTS_ACTION = "Create draft work item";
export const NO_PROJECT_TITLE = "No project";
export const NO_PROJECT_BODY =
  "To create work items or manage your work, you need to create a project or be a part of one.";
export const NO_PROJECT_ACTION = "Start your first project";

/** Artwork: a sheet with a pen, drawn inline so no asset is copied. */
function DraftArt(): React.ReactElement {
  return (
    <svg width="160" height="120" viewBox="0 0 160 120" aria-hidden="true" role="presentation" data-artwork="draft">
      <rect x="34" y="18" width="72" height="88" rx="8" fill="var(--surface)" stroke="var(--border)" strokeWidth="2" />
      <line x1="46" y1="38" x2="94" y2="38" stroke="var(--subtle)" strokeWidth="6" strokeLinecap="round" />
      <line x1="46" y1="54" x2="86" y2="54" stroke="var(--subtle)" strokeWidth="6" strokeLinecap="round" />
      <line x1="46" y1="70" x2="76" y2="70" stroke="var(--accent-subtle)" strokeWidth="6" strokeLinecap="round" />
      <g transform="rotate(-35 116 74)">
        <rect x="108" y="48" width="14" height="46" rx="3" fill="var(--accent)" />
        <rect x="108" y="48" width="14" height="10" rx="3" fill="var(--accent-subtle)" />
      </g>
      <circle cx="122" cy="30" r="8" fill="var(--accent-subtle)" stroke="var(--accent)" strokeWidth="2" />
    </svg>
  );
}

export interface DraftsEmptyStateProps {
  /** Member or admin: the action stays visible but disabled for guests. */
  canCreate: boolean;
  onCreate: () => void;
}

export function DraftsEmptyState({ canCreate, onCreate }: DraftsEmptyStateProps): React.ReactElement {
  return (
    <div data-testid="drafts-empty-state">
      <EmptyState
        variant="detailed"
        art={<DraftArt />}
        title={NO_DRAFTS_TITLE}
        description={NO_DRAFTS_BODY}
        primaryAction={{ label: NO_DRAFTS_ACTION, onSelect: onCreate, disabled: !canCreate }}
      />
    </div>
  );
}

export interface NoProjectStateProps {
  canCreate: boolean;
  onCreateProject: () => void;
}

export function DraftsNoProjectState({ canCreate, onCreateProject }: NoProjectStateProps): React.ReactElement {
  return (
    <div
      data-testid="drafts-no-project-state"
      className="flex flex-col items-center gap-(--space-3) px-(--space-6) py-(--space-12) text-center"
    >
      <span
        aria-hidden="true"
        className="grid size-16 place-items-center rounded-(--radius-dialog) bg-(--accent-subtle)"
      >
        <FilePen size={28} strokeWidth={1.5} className="text-(--accent)" />
      </span>
      <h2 className="text-h3 text-(--text)">{NO_PROJECT_TITLE}</h2>
      <p className="text-body max-w-md text-(--text-muted)">{NO_PROJECT_BODY}</p>
      <Button variant="primary" disabled={!canCreate} onClick={onCreateProject}>
        {NO_PROJECT_ACTION}
      </Button>
    </div>
  );
}

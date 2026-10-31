// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Projects-list empty states (SHELL-026, SHELL-027). Three variants with
// distinct headings and artwork: the first-run state carries the
// role-gated creation action, the no-match and empty-archive states are
// informational. The creation dialog itself arrives with NEWFRONT-284;
// this component only raises the request through onCreate.
import { Button } from "@pidash/kit";
import * as React from "react";

export type ProjectsEmptyVariant = "first-run" | "no-match" | "empty-archive";

export interface ProjectsEmptyStateProps {
  variant: ProjectsEmptyVariant;
  /** Member or admin. Guests see the action disabled. */
  canCreate: boolean;
  onCreate: () => void;
}

const HEADINGS: Record<ProjectsEmptyVariant, string> = {
  "first-run": "No active projects",
  "no-match": "No matching results.",
  "empty-archive": "No projects archived",
};

const BLURBS: Record<ProjectsEmptyVariant, string> = {
  "first-run": "Projects gather a team's issues, cycles and documents in one place. Create one to get started.",
  "no-match": "Nothing here matches the current search. Try different words or clear the search.",
  "empty-archive": "Nothing is shelved here. Archived projects rest in this view until they are restored.",
};

export function ProjectsEmptyState({ variant, canCreate, onCreate }: ProjectsEmptyStateProps): React.ReactElement {
  return (
    <div
      data-testid="projects-empty-state"
      data-variant={variant}
      className="flex flex-col items-center gap-(--space-3) px-(--space-6) py-(--space-12) text-center"
    >
      <EmptyArtwork variant={variant} />
      <h2 className="text-h3 text-(--text)">{HEADINGS[variant]}</h2>
      <p className="text-body max-w-md text-(--text-muted)">{BLURBS[variant]}</p>
      {variant === "first-run" ? (
        <Button variant="primary" disabled={!canCreate} onClick={onCreate}>
          Start your first project
        </Button>
      ) : null}
    </div>
  );
}

function EmptyArtwork({ variant }: { variant: ProjectsEmptyVariant }): React.ReactElement {
  return (
    <svg
      width="160"
      height="120"
      viewBox="0 0 160 120"
      aria-hidden="true"
      focusable="false"
      data-artwork={variant}
      role="presentation"
    >
      {variant === "first-run" ? (
        <g>
          <rect x="28" y="34" width="72" height="52" rx="6" fill="var(--accent-subtle)" />
          <rect x="44" y="24" width="72" height="52" rx="6" fill="var(--subtle)" stroke="var(--border)" />
          <rect x="60" y="14" width="72" height="52" rx="6" fill="var(--surface)" stroke="var(--accent)" />
          <line x1="96" y1="30" x2="96" y2="50" stroke="var(--accent)" strokeWidth="4" strokeLinecap="round" />
          <line x1="86" y1="40" x2="106" y2="40" stroke="var(--accent)" strokeWidth="4" strokeLinecap="round" />
          <rect x="28" y="94" width="104" height="8" rx="4" fill="var(--subtle)" />
        </g>
      ) : null}
      {variant === "no-match" ? (
        <g>
          <circle cx="70" cy="52" r="30" fill="none" stroke="var(--border)" strokeWidth="6" />
          <line x1="92" y1="74" x2="116" y2="98" stroke="var(--border)" strokeWidth="8" strokeLinecap="round" />
          <line x1="52" y1="34" x2="88" y2="70" stroke="var(--accent)" strokeWidth="5" strokeLinecap="round" />
          <circle cx="118" cy="30" r="9" fill="var(--accent-subtle)" stroke="var(--accent)" strokeWidth="3" />
          <rect x="28" y="100" width="64" height="8" rx="4" fill="var(--subtle)" />
        </g>
      ) : null}
      {variant === "empty-archive" ? (
        <g>
          <rect x="40" y="18" width="80" height="12" rx="4" fill="var(--accent)" />
          <path d="M48 34 h64 l-8 52 h-48 Z" fill="var(--accent-subtle)" stroke="var(--accent)" strokeWidth="3" />
          <path
            d="M68 58 l12 12 l24 -26"
            fill="none"
            stroke="var(--accent)"
            strokeWidth="5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <rect x="40" y="94" width="80" height="8" rx="4" fill="var(--subtle)" />
        </g>
      ) : null}
    </svg>
  );
}

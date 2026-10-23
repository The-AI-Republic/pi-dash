// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The minimal public row both layouts render until the card slice lands:
// the public key and the title, plus the state's own marker context from the
// group it sits in. Deliberately not interactive — opening an issue is the
// peek slice's capability, not this one's.

import type { SpaceBoardIssue } from "@pidash/api-client";
import * as React from "react";

import { issueKey } from "../components/BoardShell.js";

export interface SpaceIssueRowProps {
  issue: SpaceBoardIssue;
  projectIdentifier: string;
  /** Rendered as a board card instead of a list row. */
  card?: boolean;
}

export function SpaceIssueRow({ issue, projectIdentifier, card = false }: SpaceIssueRowProps): React.ReactElement {
  const key = issueKey(projectIdentifier, issue);
  return (
    <div
      data-testid={card ? "space-board-card" : "space-board-issue"}
      data-issue-id={issue.id}
      title={issue.name}
      className="flex flex-col gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) py-(--space-2)"
    >
      <span data-testid="space-issue-key" className="text-meta text-(--text-muted)">
        {key}
      </span>
      <span className="text-body truncate text-(--text)">{issue.name}</span>
    </div>
  );
}

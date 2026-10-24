// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Duplicate-detection popover on the issue detail (parity/specs/issues.md
// §20/§23, ISS-231). Cloud-only: the community build asks this component
// and gets nothing — no button, no panel — so the detail shows no trace
// of the feature (the oracle asserts the word never appears). The panel
// reuses the create modal's near-duplicate comparison against the saved
// title, minus the row itself, so viewing an item never flags itself.

import { Button, Popover } from "@pidash/kit";
import { CopyCheck } from "lucide-react";
import * as React from "react";

import { useDuplicateMatches } from "../create/dedupe.js";
import { isIssueDedupeEnabled, ISSUE_DEDUPE_FLAG } from "./flags.js";
import { duplicateHref, duplicateRows } from "./dedupeModel.js";

export interface DetailDuplicatePopoverProps {
  workspaceSlug: string;
  projectId: string;
  title: string;
  /** The viewed item, never offered as its own duplicate. */
  issueId: string;
}

/** The button label for a match count. */
export function duplicateCountLabel(count: number): string {
  return count === 1 ? "1 possible duplicate" : `${count} possible duplicates`;
}

export function DetailDuplicatePopover({
  workspaceSlug,
  projectId,
  title,
  issueId,
}: DetailDuplicatePopoverProps): React.ReactElement | null {
  const enabled = isIssueDedupeEnabled();
  const search = useDuplicateMatches({ workspaceSlug, projectId, title, enabled });
  const matches = React.useMemo(() => duplicateRows(search.matches, issueId), [search.matches, issueId]);
  if (!enabled || matches.length === 0) return null;
  return (
    <span data-testid="detail-duplicate-popover" className="inline-flex">
      <Popover
        trigger={
          <Button variant="ghost" size="small" data-dedupe-flag={ISSUE_DEDUPE_FLAG}>
            <CopyCheck size={14} strokeWidth={1.5} aria-hidden="true" />
            {duplicateCountLabel(matches.length)}
          </Button>
        }
      >
        <div className="flex flex-col gap-(--space-2)">
          <p className="text-caption font-medium text-(--text-muted)">Possible duplicates</p>
          <ul className="flex flex-col gap-(--space-1)">
            {matches.map((match) => (
              <li key={match.id}>
                <a
                  href={duplicateHref(workspaceSlug, match)}
                  target="_blank"
                  rel="noreferrer"
                  className="text-body block truncate rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text) hover:bg-(--subtle)"
                >
                  {match.name}
                </a>
              </li>
            ))}
          </ul>
        </div>
      </Popover>
    </span>
  );
}

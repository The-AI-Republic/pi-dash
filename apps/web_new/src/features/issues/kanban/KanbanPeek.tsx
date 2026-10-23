// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board peek (parity/specs/issues.md §5, ISS-034). A plain click on a
// card opens the shared peek panel beside the board: the issue key above
// its editable title, a copy-link action and a link into the full work
// item. Renaming writes through the shared issue PATCH and is offered
// only for confirmed rows the viewer may edit. The shared panel owns the
// Escape and close handling that clears the selection.

import type { IssueListItem } from "@pidash/api-client";
import { IconButton } from "@pidash/kit";
import { Link } from "@tanstack/react-router";
import { Link2 } from "lucide-react";
import * as React from "react";

import { PeekPanel } from "../../../shared/shell/PeekPanel.js";
import { copyWorkItemLink, workItemPath } from "../../../shared/shell/shareLink.js";
import { useListIssuePatch } from "../list/mutations.js";

/** The two copy-link notices the card action reports. */
export const CARD_COPY_COPIED = "Copied the work item link.";
export const CARD_COPY_FAILED = "Couldn't copy the work item link.";

export interface KanbanPeekProps {
  workspaceSlug: string;
  projectId: string;
  projectIdentifier: string;
  issue: Pick<IssueListItem, "id" | "name" | "sequence_id">;
  /** Renaming is offered only for confirmed rows the viewer may edit. */
  canRename: boolean;
  onClose: () => void;
}

/** Key printed above the peek title, e.g. "WEB-12". */
export function peekKeyOf(projectIdentifier: string, issue: Pick<IssueListItem, "sequence_id">): string {
  return `${projectIdentifier}-${issue.sequence_id}`;
}

/** Copies one work item's address and reports through the shell toasts. */
export function copyCardLink(workspaceSlug: string, issueKey: string): Promise<boolean> {
  return copyWorkItemLink(workItemPath(workspaceSlug, issueKey), {
    copied: CARD_COPY_COPIED,
    failed: CARD_COPY_FAILED,
    failedBody: "Check the clipboard permission and try again.",
  });
}

export function KanbanPeek({
  workspaceSlug,
  projectId,
  projectIdentifier,
  issue,
  canRename,
  onClose,
}: KanbanPeekProps): React.ReactElement {
  const patch = useListIssuePatch(workspaceSlug, projectId, issue.id);
  // Escape belongs to the shared panel, which registers its own listener.
  const key = peekKeyOf(projectIdentifier, issue);
  return (
    <PeekPanel
      eyebrow={<span data-testid="kanban-peek-key">{key}</span>}
      title={issue.name}
      titleLabel="Work item title"
      onClose={onClose}
      {...(canRename
        ? {
            onCommitTitle: (next: string) => {
              patch.patchIssue({ name: next });
            },
          }
        : {})}
      actions={<CopyLinkButton workspaceSlug={workspaceSlug} issueKey={key} />}
    >
      <Link
        data-testid="kanban-peek-open"
        to="/$ws/browse/$issueKey"
        params={{ ws: workspaceSlug, issueKey: key }}
        className="text-body font-medium text-(--accent) hover:underline"
      >
        Open the full work item
      </Link>
    </PeekPanel>
  );
}

/** The copy-link action inside the peek header and the card menu. */
export function CopyLinkButton({
  workspaceSlug,
  issueKey,
}: {
  workspaceSlug: string;
  issueKey: string;
}): React.ReactElement {
  return (
    <IconButton label="Copy work item link" size="small" onClick={() => void copyCardLink(workspaceSlug, issueKey)}>
      <Link2 size={16} strokeWidth={1.5} aria-hidden="true" />
    </IconButton>
  );
}

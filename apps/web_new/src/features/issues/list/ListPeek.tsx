// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Minimal list peek (parity/specs/issues.md §2, ISS-013; §7, ISS-066). A
// title click opens the row beside the list without leaving the page;
// Escape or the close control clears the peek param. The header carries
// the destructive quick actions and its own copy-link button — the menu
// itself hides copy-link because that button already owns it. Full peek
// content (description, relations, activity) is NEWFRONT-646, which
// replaces this body.

import { IconButton } from "@pidash/kit";
import type { IssueListItem } from "@pidash/api-client";
import { X } from "lucide-react";
import * as React from "react";

import { CopyWorkItemLinkButton } from "../../../shared/shell/CopyWorkItemLinkButton.js";
import { DetailQuickActions } from "../menus/DetailQuickActions.js";
import { quickActionTarget, useQuickActions } from "../menus/useQuickActions.js";

export interface ListPeekProps {
  issueKey: string;
  title: string;
  onClose: () => void;
  /** The row behind the key; absent while the list still resolves it. */
  issue?: IssueListItem | null;
  /** Workspace prefix for the links and the menu's target. */
  workspaceSlug?: string | undefined;
  /** The row's project, for the menu's target and standing. */
  projectId?: string | undefined;
  projectIdentifier?: string | undefined;
  canWrite?: boolean | undefined;
}

export function ListPeek({
  issueKey,
  title,
  onClose,
  issue = null,
  workspaceSlug,
  projectId,
  projectIdentifier,
  canWrite,
}: ListPeekProps): React.ReactElement {
  React.useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [onClose]);

  // The menu only mounts once the row resolves: an unresolved key has no
  // target to act on, and the peek body must not read the project list
  // for a row the list has not handed over yet.
  const menu =
    issue === null ||
    workspaceSlug === undefined ||
    projectId === undefined ||
    projectIdentifier === undefined ? null : (
      <PeekMenu
        workspaceSlug={workspaceSlug}
        issue={issue}
        projectIdentifier={projectIdentifier}
        {...(canWrite === undefined ? {} : { canWrite })}
        label={`Actions for ${issueKey}`}
      />
    );

  return (
    <div
      aria-label="Work item preview"
      className="absolute top-0 right-0 bottom-0 z-40 flex w-[min(30rem,92vw)] flex-col gap-(--space-4) overflow-y-auto border-l border-(--border) bg-(--surface) p-(--space-6)"
    >
      <div className="flex items-start justify-between gap-(--space-4)">
        <p className="text-caption min-w-0 flex-1 truncate pt-(--space-1) text-(--text-muted)">{issueKey}</p>
        <div className="flex shrink-0 items-center gap-(--space-1)">
          {workspaceSlug === undefined ? null : (
            <CopyWorkItemLinkButton workspaceSlug={workspaceSlug} issueKey={issueKey} />
          )}
          {menu}
          <IconButton label="Close preview" size="small" onClick={onClose}>
            <X size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        </div>
      </div>
      <textarea
        aria-label="Work item title"
        readOnly
        rows={2}
        value={title}
        className="text-h3 w-full resize-none rounded-(--radius-control) border border-transparent bg-transparent px-(--space-2) py-(--space-1) text-(--text)"
      />
    </div>
  );
}

function PeekMenu({
  workspaceSlug,
  issue,
  projectIdentifier,
  canWrite,
  label,
}: {
  workspaceSlug: string;
  issue: IssueListItem;
  projectIdentifier: string;
  canWrite?: boolean | undefined;
  label: string;
}): React.ReactElement {
  const menu = useQuickActions({
    workspaceSlug,
    target: quickActionTarget(issue, projectIdentifier),
    context: "detail",
    ...(canWrite === undefined ? {} : { canWrite }),
  });
  return <DetailQuickActions state={menu} label={label} />;
}

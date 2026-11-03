// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item identifier for the detail header (parity/specs/issues.md §13,
// ISS-163). Renders the short code plus sequence as one click-to-copy
// control with a tooltip and a toast. The copyable id is all the community
// build shows; a cloud build mounts its type badge beside it (ISS-235).
import { Tooltip } from "@pidash/kit";
import * as React from "react";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { DetailTypeBadge } from "../edition/DetailTypeSlots.js";

export interface IssueIdentifierProps {
  /** Short project code, e.g. "WEB". */
  projectIdentifier: string;
  sequenceId: number;
  /** Workspace, project and item the cloud type badge reads, when that edition mounts one. */
  workspaceSlug?: string | undefined;
  projectId?: string | undefined;
  issueId?: string | undefined;
  editable?: boolean | undefined;
}

/** Short key as people read it, e.g. "WEB-42". */
export function issueKeyOf(projectIdentifier: string, sequenceId: number): string {
  return `${projectIdentifier}-${sequenceId}`;
}

export const IDENTIFIER_COPIED_TITLE = "Copied the work item ID.";
export const IDENTIFIER_COPY_FAILED_TITLE = "Couldn't copy the work item ID.";

export function IssueIdentifier({
  projectIdentifier,
  sequenceId,
  workspaceSlug,
  projectId,
  issueId,
  editable = false,
}: IssueIdentifierProps): React.ReactElement {
  const key = issueKeyOf(projectIdentifier, sequenceId);
  const copy = async (): Promise<void> => {
    try {
      await navigator.clipboard.writeText(key);
      notifySuccess(IDENTIFIER_COPIED_TITLE);
    } catch {
      notifyError(IDENTIFIER_COPY_FAILED_TITLE);
    }
  };
  return (
    <span className="inline-flex items-center gap-(--space-2)">
      <Tooltip content="Copy the work item ID">
        <button
          type="button"
          onClick={() => void copy()}
          className="text-meta font-medium whitespace-nowrap text-(--text-muted) hover:text-(--text)"
        >
          {key}
        </button>
      </Tooltip>
      {workspaceSlug === undefined || projectId === undefined || issueId === undefined ? null : (
        <DetailTypeBadge workspaceSlug={workspaceSlug} projectId={projectId} issueId={issueId} editable={editable} />
      )}
    </span>
  );
}

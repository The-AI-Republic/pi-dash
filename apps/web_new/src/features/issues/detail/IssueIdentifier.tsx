// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item identifier for the detail header (parity/specs/issues.md §13,
// ISS-163). Renders the short code plus sequence as one click-to-copy
// control with a tooltip and a toast; this build shows only the copyable
// id, and the cloud type switcher arrives with ISS-235 (NEWFRONT-657).
import { Tooltip } from "@pidash/kit";
import * as React from "react";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

export interface IssueIdentifierProps {
  /** Short project code, e.g. "WEB". */
  projectIdentifier: string;
  sequenceId: number;
}

/** Short key as people read it, e.g. "WEB-42". */
export function issueKeyOf(projectIdentifier: string, sequenceId: number): string {
  return `${projectIdentifier}-${sequenceId}`;
}

export const IDENTIFIER_COPIED_TITLE = "Copied the work item ID.";
export const IDENTIFIER_COPY_FAILED_TITLE = "Couldn't copy the work item ID.";

export function IssueIdentifier({ projectIdentifier, sequenceId }: IssueIdentifierProps): React.ReactElement {
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
    <Tooltip content="Copy the work item ID">
      <button
        type="button"
        onClick={() => void copy()}
        className="text-meta font-medium whitespace-nowrap text-(--text-muted) hover:text-(--text)"
      >
        {key}
      </button>
    </Tooltip>
  );
}

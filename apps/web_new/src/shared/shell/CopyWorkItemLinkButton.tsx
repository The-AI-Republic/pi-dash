// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Copy-link control the detail header and the peek panels share (DESK-019).
// It copies the canonical work-item address through the shared link helper,
// so the desktop bundle hands out the hosted app's address and never its
// own private origin.

import { IconButton } from "@pidash/kit";
import { Link2 } from "lucide-react";
import * as React from "react";

import { copyLinkWithToast, workItemPath } from "./shareLink.js";
import { notifyError, notifySuccess } from "./toaster.js";

export interface CopyWorkItemLinkButtonProps {
  workspaceSlug: string;
  /** Route key shown to people, e.g. "WEB-123". */
  issueKey: string;
}

export const COPY_WORK_ITEM_LINK_LABEL = "Copy work item link";

/**
 * Copy a work item's canonical address with the shared reporting: the
 * button below and menu entries that cannot host the button (the intake
 * overflow menu) run the same copy. Resolves true when written.
 */
export function copyWorkItemLinkAddress(workspaceSlug: string, issueKey: string): Promise<boolean> {
  return copyLinkWithToast(
    workItemPath(workspaceSlug, issueKey),
    { copied: "Copied the work item link.", failed: "Couldn't copy the work item link." },
    { success: notifySuccess, error: notifyError }
  );
}

export function CopyWorkItemLinkButton({ workspaceSlug, issueKey }: CopyWorkItemLinkButtonProps): React.ReactElement {
  return (
    <IconButton
      label={COPY_WORK_ITEM_LINK_LABEL}
      size="small"
      onClick={() => void copyWorkItemLinkAddress(workspaceSlug, issueKey)}
    >
      <Link2 size={16} strokeWidth={1.5} aria-hidden="true" />
    </IconButton>
  );
}

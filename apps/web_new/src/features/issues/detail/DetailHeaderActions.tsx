// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail header action row (parity/specs/issues.md §13, ISS-160–163):
// the click-to-copy identifier, the shared copy-link control, the
// subscribe toggle, and the detail quick-action menu the peek reuses.
// The menu state arrives from the mounting route, which owns the
// quick-action side effects and their navigation.
import type { Issue } from "@pidash/api-client";
import * as React from "react";

import { CopyWorkItemLinkButton } from "../../../shared/shell/CopyWorkItemLinkButton.js";
import { DetailQuickActions } from "../menus/DetailQuickActions.js";
import type { QuickActionsState } from "../menus/useQuickActions.js";
import { IssueIdentifier } from "./IssueIdentifier.js";
import { IssueSubscribeToggle } from "./IssueSubscribeToggle.js";

export interface DetailHeaderActionsProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  /** Route key shown to people, e.g. "WEB-123". */
  issueKey: string;
  /** Short code of the owning project, e.g. "WEB". */
  projectIdentifier: string;
  /** Project member standing on a live project; guests read. */
  editable: boolean;
  /** The header menu; null on surfaces without one (synced rows). */
  menu: QuickActionsState | null;
  /** After a landed subscription change (the detail re-reads). */
  onSubscriptionChanged?: (() => void) | undefined;
}

export function DetailHeaderActions({
  workspaceSlug,
  projectId,
  issue,
  issueKey,
  projectIdentifier,
  editable,
  menu,
  onSubscriptionChanged,
}: DetailHeaderActionsProps): React.ReactElement {
  return (
    <div className="flex items-center gap-(--space-2)">
      <IssueIdentifier
        projectIdentifier={projectIdentifier}
        sequenceId={issue.sequence_id}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        issueId={issue.id}
        editable={editable}
      />
      <CopyWorkItemLinkButton workspaceSlug={workspaceSlug} issueKey={issueKey} />
      <IssueSubscribeToggle
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        issueId={issue.id}
        archived={issue.archived_at !== null}
        canWrite={editable}
        onChanged={onSubscriptionChanged}
      />
      {menu === null ? null : <DetailQuickActions state={menu} />}
    </div>
  );
}

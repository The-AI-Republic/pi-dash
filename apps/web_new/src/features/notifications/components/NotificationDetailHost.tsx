// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Thin notification-to-issue preview seam (parity/specs/notifications.md
// §5 and §12, NTF-007-009). Notifications own selection and the state
// switch; access and issue data/content stay behind their public features.
import { useQuery } from "@tanstack/react-query";
import { FileSearch, X } from "lucide-react";
import * as React from "react";

import { IconButton, Spinner } from "@pidash/kit";
import { getAppClient } from "../../../core/api/client.js";
import { ROLE_MEMBER } from "../../../core/session/index.js";
import { PeekPanel } from "../../../shared/shell/index.js";
import { projectRolesQueryOptions } from "../../auth/index.js";
import { IssueDetailRoot, issueDetailQueryOptions } from "../../issues/index.js";
import type { NotificationCardData } from "../lib/card.js";

export interface NotificationDetailHostProps {
  workspaceSlug: string;
  selection: NotificationCardData | null;
  onClose: () => void;
}

export function NotificationDetailHost({
  workspaceSlug,
  selection,
  onClose,
}: NotificationDetailHostProps): React.ReactElement {
  const access = useQuery({
    ...projectRolesQueryOptions(getAppClient(), workspaceSlug),
    enabled: selection !== null,
  });
  const issue = useQuery({
    ...issueDetailQueryOptions(getAppClient(), workspaceSlug, selection?.projectId ?? "", selection?.issueId ?? ""),
    enabled: selection !== null && !access.isPending,
  });

  if (selection === null) {
    return (
      <div data-testid="notifications-detail-pane" className="grid h-full place-items-center p-(--space-8) text-center">
        <div
          data-testid="notifications-detail-placeholder"
          className="flex max-w-sm flex-col items-center gap-(--space-3)"
        >
          <span
            aria-hidden="true"
            className="grid size-12 place-items-center rounded-(--radius-full) bg-(--subtle) text-(--text-muted)"
          >
            <FileSearch size={22} strokeWidth={1.5} />
          </span>
          <p className="text-emphasis text-(--text)">Choose an update</p>
          <p className="text-body text-(--text-muted)">Its work item overview will open here.</p>
        </div>
      </div>
    );
  }

  if (access.isPending || issue.isPending) {
    return (
      <div data-testid="notifications-detail-pane" className="grid h-full place-items-center p-(--space-8)">
        <div data-testid="notifications-detail-loading">
          <Spinner label="Loading work item access" />
        </div>
      </div>
    );
  }

  const role = access.data?.[selection.projectId] ?? null;
  const variant = selection.triage ? "triage" : "peek";
  const content =
    issue.data === undefined ? (
      <div className="p-(--space-8)">
        <p className="text-emphasis text-(--text)">{selection.title}</p>
        <p className="text-body text-(--text-muted)">{selection.summary}</p>
      </div>
    ) : (
      <IssueDetailRoot
        workspaceSlug={workspaceSlug}
        projectId={selection.projectId}
        issue={issue.data}
        editable={!selection.triage && role !== null && role >= ROLE_MEMBER}
        topSlot={<span className="text-meta text-(--text-muted)">{selection.reference}</span>}
      />
    );

  if (!selection.triage) {
    return (
      <div data-testid="notifications-detail-pane" data-detail-variant={variant} className="h-full">
        <PeekPanel eyebrow={selection.reference} title={selection.title} titleLabel="Work item title" onClose={onClose}>
          {content}
        </PeekPanel>
      </div>
    );
  }

  return (
    <div data-testid="notifications-detail-pane" data-detail-variant={variant} className="h-full overflow-y-auto">
      <header className="sticky top-0 z-10 flex items-center justify-between gap-(--space-3) border-b border-(--border) bg-(--surface) px-(--space-4) py-(--space-3)">
        <div className="min-w-0">
          <p className="text-meta font-medium text-(--text)">{selection.reference}</p>
          <p className="text-caption text-(--text-muted)">Triage work item</p>
        </div>
        <IconButton
          label="Close notification details"
          size="small"
          onClick={onClose}
          data-testid="notifications-detail-close"
        >
          <X size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      </header>
      {content}
    </div>
  );
}

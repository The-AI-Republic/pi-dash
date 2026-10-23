// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Thin notification-to-issue preview seam (parity/specs/notifications.md
// §5, §6 and §12, NTF-007-009). Notifications own selection and the state
// switch; access and issue data/content stay behind their public features.
// The preview renders inline in the inbox detail pane — never through the
// shared portal overlay — so the pane carries the selected content, the
// notification's own change and the close control (NTF-008, NTF-009).
import { useQuery } from "@tanstack/react-query";
import type { Issue } from "@pidash/api-client";
import { FileSearch, X } from "lucide-react";
import * as React from "react";

import { Avatar, IconButton, Spinner } from "@pidash/kit";
import { getAppClient } from "../../../core/api/client.js";
import { ROLE_MEMBER, useWorkspace } from "../../../core/session/index.js";
import { CopyWorkItemLinkButton } from "../../../shared/shell/CopyWorkItemLinkButton.js";
import { projectMembershipQueryOptions } from "../../auth/index.js";
import {
  IssueDetailRoot,
  IssuePropertiesSidebar,
  TriageDetailRoot,
  issueDetailQueryOptions,
} from "../../issues/index.js";
import type { NotificationCardData } from "../lib/card.js";
import { formatNotificationAge } from "../lib/relativeAge.js";

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
  // The access read is per project and per selection, never the warmed
  // workspace role map: its own fetch runs while the pane shows the
  // loading state, and a triage item then embeds the issues-owned
  // triage content (spec §5, §12).
  const access = useQuery({
    ...projectMembershipQueryOptions(getAppClient(), workspaceSlug, selection?.projectId ?? ""),
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

  const role = access.data?.role ?? null;
  const variant = selection.triage ? "triage" : "peek";
  const editable = !selection.triage && role !== null && role >= ROLE_MEMBER;

  return (
    <div
      data-testid="notifications-detail-pane"
      data-detail-variant={variant}
      className="flex h-full min-h-0 flex-col overflow-y-auto"
    >
      <header className="sticky top-0 z-10 flex items-center justify-between gap-(--space-3) border-b border-(--border) bg-(--surface) px-(--space-5) py-(--space-3)">
        <div className="min-w-0">
          <p className="text-meta font-medium text-(--text)">{selection.reference}</p>
          <p className="text-caption text-(--text-muted)">
            {selection.triage ? "Triage work item" : "Work item preview"}
          </p>
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

      <section
        data-testid="notifications-detail-change"
        aria-label="Notification change"
        className="flex gap-(--space-3) border-b border-(--border) px-(--space-5) py-(--space-4)"
      >
        <Avatar
          name={selection.actorName}
          {...(selection.actorAvatarUrl === null ? {} : { src: selection.actorAvatarUrl })}
          size="small"
        />
        <div className="min-w-0 flex-1">
          <p data-testid="notifications-detail-summary" className="text-body text-(--text)">
            {selection.summary}
          </p>
          <p className="text-caption text-(--text-muted)">
            <span data-testid="notifications-detail-actor">{selection.actorName}</span>
            {" · "}
            <time dateTime={selection.createdAt}>{formatNotificationAge(selection.createdAt)}</time>
          </p>
        </div>
      </section>

      {issue.data === undefined ? (
        <div className="p-(--space-5)">
          <p className="text-emphasis text-(--text)">{selection.title}</p>
          <p className="text-body text-(--text-muted)">The work item details could not be loaded.</p>
        </div>
      ) : selection.triage ? (
        <TriageDetailRoot
          workspaceSlug={workspaceSlug}
          projectId={selection.projectId}
          issue={issue.data}
          reference={selection.reference}
        />
      ) : (
        <IssuePreviewDetail
          workspaceSlug={workspaceSlug}
          projectId={selection.projectId}
          issue={issue.data}
          editable={editable}
          topSlot={
            <div className="flex items-center gap-(--space-2)">
              <span className="text-meta text-(--text-muted)">{selection.reference}</span>
              <CopyWorkItemLinkButton workspaceSlug={workspaceSlug} issueKey={selection.reference} />
            </div>
          }
          sidebar={
            <IssuePropertiesSidebar
              workspaceSlug={workspaceSlug}
              projectId={selection.projectId}
              issue={issue.data}
              editable={editable}
            />
          }
        />
      )}
    </div>
  );
}

/** The peek mounts the workspace read only once it actually renders. */
function IssuePreviewDetail(props: {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  editable: boolean;
  topSlot: React.ReactNode;
  sidebar: React.ReactNode;
}): React.ReactElement {
  const { workspace } = useWorkspace(props.workspaceSlug);
  return (
    <IssueDetailRoot
      workspaceSlug={props.workspaceSlug}
      workspaceId={workspace?.id ?? null}
      projectId={props.projectId}
      issue={props.issue}
      editable={props.editable}
      topSlot={props.topSlot}
      sidebar={props.sidebar}
    />
  );
}

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Description version history (parity/specs/issues.md §13, ISS-147).
// The "Edited by" line under the description opens the stored versions
// (avatar, author, age); picking one previews its HTML with newer/older
// navigation and offers a restore that writes the version back through
// the update. Restoring hides for archived, synced, and guest views.

import { useQuery } from "@tanstack/react-query";
import { Avatar, Button, Dialog, IconButton, Menu, Skeleton, type MenuEntry } from "@pidash/kit";
import type { WorkspaceMemberList } from "@pidash/api-client";
import { ChevronLeft, ChevronRight, History } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { descriptionAssetSrc, RichTextView } from "../../../shared/editor/index.js";
import { timeAgo } from "../../../shared/lib/timeAgo.js";
import { usePaletteWorkspaceMembers } from "../../../shared/shell/contextual/api.js";
import { descriptionVersionQueryOptions, descriptionVersionsQueryOptions } from "../api/descriptionVersions.js";

export interface DescriptionHistoryProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Issue creation timestamp, dating the line when unsaved. */
  createdAt: string;
  /** Issue creator id, naming the line when unsaved. */
  createdBy: string | null;
  /** True for synced issues: the line says why editing is off. */
  synced: boolean;
  /** True for archived, synced, or guest views: no restore entry. */
  restoreDisabled: boolean;
  /** Write version HTML back through the update. */
  onRestore: (html: string) => void;
}

/** Display name for an author the workspace no longer lists. */
export const HISTORY_UNKNOWN_AUTHOR = "Unknown member";

export interface VersionAuthor {
  name: string;
  avatarUrl: string | undefined;
}

/** Author row for a user id, falling back when unlisted. */
export function selectVersionAuthor(members: WorkspaceMemberList | undefined, userId: string | null): VersionAuthor {
  const found = userId === null ? undefined : members?.find((entry) => entry.member.id === userId);
  if (found === undefined) return { name: HISTORY_UNKNOWN_AUTHOR, avatarUrl: undefined };
  return { name: found.member.display_name, avatarUrl: found.member.avatar_url ?? undefined };
}

export function DescriptionHistory({
  workspaceSlug,
  projectId,
  issueId,
  createdAt,
  createdBy,
  synced,
  restoreDisabled,
  onRestore,
}: DescriptionHistoryProps): React.ReactElement {
  const [activeId, setActiveId] = React.useState<string | null>(null);
  const client = getAppClient();
  const versionsQuery = useQuery(descriptionVersionsQueryOptions(client, workspaceSlug, projectId, issueId));
  const membersQuery = usePaletteWorkspaceMembers(workspaceSlug);
  const detailQuery = useQuery({
    ...descriptionVersionQueryOptions(client, workspaceSlug, projectId, issueId, activeId ?? ""),
    enabled: activeId !== null,
  });
  // A stored version carries the same legacy image sources as the detail;
  // resolve them through this issue's project so the preview shows the
  // picture (ISS-127).
  const resolveAsset = React.useCallback(
    (src: string) => descriptionAssetSrc(src, { workspaceSlug, projectId, apiBaseUrl: getAppClient().baseUrl }),
    [workspaceSlug, projectId]
  );

  const versions = versionsQuery.data?.results ?? [];
  const latest = versions[0];
  const lineAuthor =
    latest === undefined
      ? selectVersionAuthor(membersQuery.data, createdBy)
      : selectVersionAuthor(membersQuery.data, latest.owned_by);
  const lineAge = latest === undefined ? timeAgo(createdAt) : timeAgo(latest.created_at);
  const active = versions.find((row) => row.id === activeId) ?? null;
  const activeIndex = active === null ? -1 : versions.findIndex((row) => row.id === active.id);
  const activeHeader =
    active === null
      ? undefined
      : `${selectVersionAuthor(membersQuery.data, active.owned_by).name} · ${timeAgo(active.last_saved_at)}`;
  const detail = detailQuery.data;

  const items: MenuEntry[] = versions.map((row) => {
    const author = selectVersionAuthor(membersQuery.data, row.owned_by);
    return {
      label: `${author.name} ${timeAgo(row.last_saved_at)}`,
      itemKey: row.id,
      icon: (
        <Avatar
          name={author.name}
          {...(author.avatarUrl === undefined ? {} : { src: author.avatarUrl })}
          size="small"
        />
      ),
      onSelect: () => setActiveId(row.id),
    };
  });

  return (
    <>
      <Dialog
        title="Description history"
        description={activeHeader}
        open={activeId !== null}
        // No opener lives inside the dialog, so every openness request is
        // a close intent (Escape, backdrop, close button).
        onOpenChange={() => setActiveId(null)}
        actions={
          <>
            <Button variant="secondary" size="medium" onClick={() => setActiveId(null)}>
              Cancel
            </Button>
            {restoreDisabled || detail === undefined ? null : (
              <Button
                variant="primary"
                size="medium"
                onClick={() => {
                  onRestore(detail.description_html);
                  setActiveId(null);
                }}
              >
                Restore
              </Button>
            )}
          </>
        }
      >
        <div className="flex items-center justify-between gap-(--space-4)">
          <p className="text-meta text-(--text-muted)">
            {active === null ? null : `Version ${activeIndex + 1} of ${versions.length}`}
          </p>
          <div className="flex items-center gap-(--space-2)">
            <IconButton
              label="Newer version"
              variant="ghost"
              size="small"
              disabled={activeIndex <= 0}
              onClick={() => {
                const newer = versions[activeIndex - 1];
                if (newer !== undefined) setActiveId(newer.id);
              }}
            >
              <ChevronLeft size={16} strokeWidth={1.5} />
            </IconButton>
            <IconButton
              label="Older version"
              variant="ghost"
              size="small"
              disabled={activeIndex === -1 || activeIndex >= versions.length - 1}
              onClick={() => {
                const older = versions[activeIndex + 1];
                if (older !== undefined) setActiveId(older.id);
              }}
            >
              <ChevronRight size={16} strokeWidth={1.5} />
            </IconButton>
          </div>
        </div>
        <div className="mt-(--space-4)">
          {detailQuery.isError ? (
            <p className="text-body text-(--danger)">Couldn&apos;t load this version.</p>
          ) : detail === undefined ? (
            <div className="flex flex-col gap-(--space-4)">
              <Skeleton height="14px" width="80%" />
              <Skeleton height="14px" width="60%" />
              <Skeleton height="14px" width="70%" />
            </div>
          ) : (
            <RichTextView html={detail.description_html} label="Version preview" resolveAsset={resolveAsset} />
          )}
        </div>
      </Dialog>
      <Menu
        trigger={
          <Button variant="ghost" size="small" disabled={versions.length === 0}>
            <History size={16} strokeWidth={1.5} aria-hidden="true" />
            <span>
              Edited by <span className="font-medium">{lineAuthor.name}</span> {lineAge}
              {synced ? <span className="text-(--text-muted)"> (synced, read-only)</span> : null}
            </span>
          </Button>
        }
        items={items}
      />
    </>
  );
}

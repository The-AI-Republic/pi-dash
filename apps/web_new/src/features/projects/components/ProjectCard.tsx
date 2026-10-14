// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project card (SHELL-032 through SHELL-035). Identity, visibility,
// description fallback, member stack and favorite star on a header row
// with quick actions — deliberately no cover slot (target per main#523).
// Presentational: the list view feeds the member directory, the star
// entitlement and the favorite toggle through props, so this renders
// without providers. Clicks route members into the work-items view,
// intercept outsiders toward joining, and never fire on archived cards.
// The restore/delete dialogs arrive with NEWFRONT-284; until their
// handlers are passed, those menu entries stay hidden.
import { Link } from "@tanstack/react-router";
import type { ProjectDetails, WorkspaceMember } from "@pidash/api-client";
import { ContextMenu, IconButton, Menu } from "@pidash/kit";
import { Check, Ellipsis, Lock, Settings, Star } from "lucide-react";
import * as React from "react";

import { platform } from "../../../core/platform/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { isProjectAdmin, isProjectAuthorized } from "../tabs.js";
import { projectIssuesPath, projectSettingsPath, selectCardMenuEntries } from "./cardMenu.js";
import { JoinProjectDialog } from "./JoinProjectDialog.js";
import { ProjectLogo } from "./ProjectLogo.js";
import { ProjectMemberStack, type MemberStackEntry } from "./ProjectMemberStack.js";

/** `network` value marking a private project (public reads 2). */
export const PROJECT_NETWORK_PRIVATE = 0;

const COPIED_TITLE = "Copied the project link";
const COPIED_BODY = "Paste it anywhere to share this project.";
const COPY_FAILED_TITLE = "Couldn't copy the link";
const COPY_FAILED_BODY = "The clipboard refused the write — copy the address manually.";

const UNKNOWN_MEMBER_NAME = "Project member";

export interface ProjectCardProps {
  workspaceSlug: string;
  project: ProjectDetails;
  /** Member directory; names resolve once it lands. */
  members?: WorkspaceMember[] | undefined;
  /** Workspace admin or member: guests never see the star. */
  canStar?: boolean | undefined;
  /** Favorite write in flight for this card. */
  starPending?: boolean | undefined;
  onToggleFavorite?: ((project: ProjectDetails) => void) | undefined;
  /** Restore dialog wired (NEWFRONT-284). Absent, the entry hides. */
  onRestore?: ((project: ProjectDetails) => void) | undefined;
  /** Delete dialog wired (NEWFRONT-284). Absent, the entry hides. */
  onDelete?: ((project: ProjectDetails) => void) | undefined;
  /** Post-join landing; defaults to a full-page load of the address. */
  onNavigate?: ((href: string) => void) | undefined;
}

/** "Jan 2, 2024" in UTC, so the fallback never shifts with timezone. */
export function formatCreationDate(iso: string): string {
  return new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
    timeZone: "UTC",
  }).format(new Date(iso));
}

function halt(event: React.SyntheticEvent): void {
  event.preventDefault();
  event.stopPropagation();
}

export function ProjectCard({
  workspaceSlug,
  project,
  members,
  canStar = false,
  starPending = false,
  onToggleFavorite,
  onRestore,
  onDelete,
  onNavigate,
}: ProjectCardProps): React.ReactElement {
  const [joinOpen, setJoinOpen] = React.useState(false);
  const archived = project.archived_at !== null;
  const isMember = project.member_role !== null;
  const isAuthorized = isProjectAuthorized(project.member_role);
  const navigate = onNavigate ?? ((href: string) => globalThis.window.location.assign(href));
  const issuesPath = projectIssuesPath(workspaceSlug, project.id);
  const settingsPath = projectSettingsPath(workspaceSlug, project.id);

  const directory = React.useMemo(() => {
    const rows = new Map<string, WorkspaceMember["member"]>();
    for (const row of members ?? []) rows.set(row.member.id, row.member);
    return rows;
  }, [members]);
  const stack: MemberStackEntry[] = project.members.map((id) => {
    const found = directory.get(id);
    return { id, name: found?.display_name ?? UNKNOWN_MEMBER_NAME, avatarUrl: found?.avatar_url ?? null };
  });

  const description = project.description.trim();
  const subText = description === "" ? `Created on ${formatCreationDate(project.created_at)}` : project.description;
  const showStar = !archived && canStar && onToggleFavorite !== undefined;

  async function copyLink(): Promise<void> {
    try {
      const clipboard = globalThis.navigator.clipboard;
      if (clipboard === undefined) throw new Error("clipboard unavailable");
      await clipboard.writeText(`${globalThis.window.location.origin}${issuesPath}`);
      notifySuccess(COPIED_TITLE, COPIED_BODY);
    } catch {
      notifyError(COPY_FAILED_TITLE, COPY_FAILED_BODY);
    }
  }

  const restoreAction = onRestore === undefined ? null : () => onRestore(project);
  const deleteAction = onDelete === undefined ? null : () => onDelete(project);
  const entries = selectCardMenuEntries(
    {
      archived,
      isMember,
      isAuthorized,
      isAdmin: isProjectAdmin(project.member_role),
    },
    {
      onSettings: () => globalThis.window.location.assign(settingsPath),
      onJoin: () => setJoinOpen(true),
      onOpenInNewTab: () => void platform.openExternal(`${globalThis.window.location.origin}${issuesPath}`),
      onCopyLink: () => void copyLink(),
      onRestore: restoreAction,
      onDelete: deleteAction,
    }
  );

  function onCardClick(event: React.MouseEvent): void {
    if (archived) {
      halt(event);
      return;
    }
    if (!isMember) {
      halt(event);
      setJoinOpen(true);
    }
  }

  const card = (
    <Link
      to="/$ws/projects/$projectId/issues"
      params={{ ws: workspaceSlug, projectId: project.id }}
      search={{}}
      data-testid="project-card"
      onClick={onCardClick}
      className="flex w-full flex-col gap-(--space-3) rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-4) hover:border-(--accent)"
    >
      <div className="flex items-center gap-(--space-3)">
        <ProjectLogo logoProps={project.logo_props} identifier={project.identifier} />
        <span className="flex min-w-0 flex-1 flex-col gap-(--space-1)">
          <h2 className="text-emphasis truncate text-(--text)">{project.name}</h2>
          <span className="text-caption flex items-center gap-(--space-1) text-(--text-muted)">
            <span className="truncate font-[family-name:var(--font-mono)]">{project.identifier}</span>
            {project.network === PROJECT_NETWORK_PRIVATE ? (
              <>
                <Lock size={12} strokeWidth={1.5} aria-hidden="true" />
                <span className="sr-only">Private project</span>
              </>
            ) : null}
          </span>
        </span>
        {showStar ? (
          <IconButton
            label={project.is_favorite ? `Remove ${project.name} from favorites` : `Add ${project.name} to favorites`}
            size="small"
            data-testid="project-card-star"
            disabled={starPending}
            aria-busy={starPending || undefined}
            onClick={(event) => {
              halt(event);
              onToggleFavorite?.(project);
            }}
          >
            <Star size={16} strokeWidth={1.5} aria-hidden="true" fill={project.is_favorite ? "currentColor" : "none"} />
          </IconButton>
        ) : null}
        {entries.length > 0 ? (
          <Menu
            trigger={
              <IconButton
                label={`Actions for ${project.name}`}
                size="small"
                data-testid="project-card-actions"
                onClick={halt}
              >
                <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
              </IconButton>
            }
            items={entries}
          />
        ) : null}
      </div>
      <p data-testid="project-card-subtext" className="text-body line-clamp-2 break-words text-(--text-muted)">
        {subText}
      </p>
      <div className="flex items-center justify-between gap-(--space-2)">
        <span className="flex items-center gap-(--space-2)">
          <ProjectMemberStack members={stack} />
          {archived ? <span className="text-caption font-medium text-(--text-muted)">Archived</span> : null}
        </span>
        {archived ? null : isAuthorized ? (
          <IconButton
            label={`${project.name} settings`}
            size="small"
            onClick={(event) => {
              halt(event);
              globalThis.window.location.assign(settingsPath);
            }}
          >
            <Settings size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        ) : isMember ? (
          <span className="text-body flex items-center gap-(--space-1) text-(--text-muted)">
            <Check size={16} strokeWidth={1.5} aria-hidden="true" />
            Joined
          </span>
        ) : (
          <button
            type="button"
            onClick={(event) => {
              halt(event);
              setJoinOpen(true);
            }}
            className="text-body cursor-pointer font-medium text-(--accent) hover:underline"
          >
            Join
          </button>
        )}
      </div>
    </Link>
  );

  return (
    <>
      {entries.length > 0 ? <ContextMenu trigger={card} items={entries} /> : card}
      {joinOpen ? (
        <JoinProjectDialog
          workspaceSlug={workspaceSlug}
          projectId={project.id}
          projectName={project.name}
          open={joinOpen}
          onOpenChange={setJoinOpen}
          onNavigate={navigate}
        />
      ) : null}
    </>
  );
}

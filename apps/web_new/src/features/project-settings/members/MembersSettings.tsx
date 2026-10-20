// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members tab body (parity/specs/project-settings-members.md §"Member
// table", §"Search, filter and sort", §"Loading and failure states";
// PSET-018–021, PSET-027). Reads the memberships and the workspace
// directory, shows a skeleton until both settle, then the search box, the
// role filter, and the table. Search is local; the role filter and sort
// persist per project for the session. Zero matches show the centered
// notice; a failed read toasts the server message and keeps the surface.

import { cn } from "@pidash/kit";
import { Search } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { resolveFileUrl } from "../../../shared/shell/index.js";
import { notifyMembersLoadFailure, useProjectMemberships, useWorkspaceDirectory } from "./api.js";
import { useMemberFilters } from "./filters.js";
import { MemberFilters } from "./MemberFilters.js";
import { MemberSkeleton } from "./MemberSkeleton.js";
import { MemberTable } from "./MemberTable.js";
import { applyMemberRosterFilters, buildMemberRoster, type MemberRosterFilters, type MemberRoleKey } from "./roster.js";

/** Centered note when search and role filters exclude every row. */
export const MEMBERS_NO_MATCH_TEXT = "No matching members";

export interface MembersSettingsProps {
  workspaceSlug: string;
  projectId: string;
  /** Resolves a stored avatar path; defaults to the app client's origin. */
  resolveAvatar?: (path: string | null) => string | null;
}

/**
 * Fire the failure toast once per failed resolution: the two reads are
 * independent, and the surface keeps its skeleton or table either way.
 */
function useReadFailureNotice(errors: readonly (unknown | null)[]): void {
  const notified = React.useRef(new Set<unknown>());
  React.useEffect(() => {
    for (const error of errors) {
      if (error === null || notified.current.has(error)) continue;
      notified.current.add(error);
      notifyMembersLoadFailure(error);
    }
  }, [errors]);
}

export function MembersSettings({ workspaceSlug, projectId, resolveAvatar }: MembersSettingsProps): React.ReactElement {
  const memberships = useProjectMemberships(workspaceSlug, projectId);
  const directory = useWorkspaceDirectory(workspaceSlug);
  const [persisted, setPersisted] = useMemberFilters(projectId);
  const [search, setSearch] = React.useState("");

  useReadFailureNotice([memberships.error ?? null, directory.error ?? null]);

  const loading = memberships.isPending || directory.isPending;
  // A refused read keeps the skeleton rather than claiming "no matches":
  // the toast explains the failure and the surface stays put (PSET-027).
  const unavailable =
    (memberships.isError && memberships.data === undefined) || (directory.isError && directory.data === undefined);
  const roster = React.useMemo(
    () =>
      memberships.data === undefined || directory.data === undefined
        ? []
        : buildMemberRoster(memberships.data, directory.data),
    [memberships.data, directory.data]
  );
  const filters: MemberRosterFilters = React.useMemo(() => ({ ...persisted, search }), [persisted, search]);
  const visible = React.useMemo(() => applyMemberRosterFilters(roster, filters), [roster, filters]);

  const resolveAvatarFile = React.useCallback(
    (path: string | null) =>
      resolveAvatar === undefined ? resolveFileUrl(path, getAppClient().baseUrl) : resolveAvatar(path),
    [resolveAvatar]
  );

  const setRoles = (roles: MemberRoleKey[]): void => {
    setPersisted({ ...persisted, roles });
  };

  return (
    <section
      aria-label="Members settings"
      data-testid="project-settings-members"
      data-slice="NEWFRONT-574"
      className="flex flex-col gap-(--space-4)"
    >
      <div className="flex flex-wrap items-center justify-between gap-(--space-3)">
        <span className="flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) px-(--space-3) py-(--space-1)">
          <Search size={14} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
          <input
            aria-label="Search members"
            placeholder="Search members"
            value={search}
            onChange={(event) => setSearch(event.currentTarget.value)}
            className="text-body w-56 border-none bg-transparent text-(--text) placeholder:text-(--text-muted) focus:outline-none"
          />
        </span>
        <MemberFilters roles={persisted.roles} onChange={setRoles} />
      </div>
      {loading || unavailable ? (
        <MemberSkeleton />
      ) : visible.length === 0 ? (
        <p data-testid="members-no-match" className={cn("text-body py-(--space-12) text-center text-(--text-muted)")}>
          {MEMBERS_NO_MATCH_TEXT}
        </p>
      ) : (
        <MemberTable
          workspaceSlug={workspaceSlug}
          rows={visible}
          sort={persisted.sort}
          onSortChange={(sort) => setPersisted({ ...persisted, sort })}
          resolveAvatar={resolveAvatarFile}
        />
      )}
    </section>
  );
}

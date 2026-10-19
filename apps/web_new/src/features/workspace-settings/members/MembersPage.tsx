// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members roster page body (parity/specs/workspace-settings.md §7–§9,
// WSET-008/011/014). The settings shell (NEWFRONT-534) supplies the frame,
// gate and tab title; this page renders the section header (count chip,
// persistent search, role filter), the roster table, the skeleton until
// both reads resolve, and the inline no-match note. Invitations, the Add
// control and row actions belong to NEWFRONT-538.

import { Badge, Button, cn } from "@pidash/kit";
import { Search } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { resolveFileUrl } from "../../../shared/shell/index.js";
import { usePendingInvitations, useWorkspaceRoster } from "./api.js";
import { MembersFilters } from "./MembersFilters.js";
import { MembersSkeleton } from "./MembersSkeleton.js";
import { MembersTable } from "./MembersTable.js";
import {
  applyRosterFilters,
  EMPTY_ROSTER_FILTERS,
  rosterCount,
  type RosterFilters,
  type RosterRoleFilter,
} from "./roster.js";

/** Inline note when the filters exclude every member (§9). */
export const MEMBERS_NO_MATCH_TEXT = "No matching members";
/** Read failure notice for the two roster reads. */
export const MEMBERS_LOAD_FAILED_TEXT = "Couldn't load the members.";
export const MEMBERS_RETRY_LABEL = "Try again";

export interface MembersPageProps {
  workspaceSlug: string;
}

export function MembersPage({ workspaceSlug }: MembersPageProps): React.ReactElement {
  const members = useWorkspaceRoster(workspaceSlug);
  const invitations = usePendingInvitations(workspaceSlug);
  const [filters, setFilters] = React.useState<RosterFilters>(EMPTY_ROSTER_FILTERS);

  const baseUrl = getAppClient().baseUrl;
  const loading = members.isPending || invitations.isPending;
  const failed = members.isError || invitations.isError;

  const allMembers = members.data ?? [];
  const visible = React.useMemo(() => applyRosterFilters(allMembers, filters), [allMembers, filters]);
  const noMatch = !loading && !failed && visible.length === 0;

  const search = (value: string): void => {
    setFilters((current) => ({ ...current, search: value }));
  };
  const setRoles = (roles: RosterRoleFilter[]): void => {
    setFilters((current) => ({ ...current, roles }));
  };
  const setOrder = (order: RosterFilters["order"]): void => {
    setFilters((current) => ({ ...current, order }));
  };
  const retry = (): void => {
    void members.refetch();
    void invitations.refetch();
  };

  // Tests pin the count rule without a browser; the page never recomputes it.
  const count = rosterCount(allMembers);

  return (
    <section aria-label="Members" data-testid="workspace-settings-members" className="flex flex-col gap-(--space-4)">
      <div className="flex flex-wrap items-center justify-between gap-(--space-3)">
        <h2 className="text-h3 flex items-center gap-(--space-2) text-(--text)">
          Members
          {count > 0 ? (
            <span data-testid="members-count" aria-label={`${count} members`}>
              <Badge>{count}</Badge>
            </span>
          ) : null}
        </h2>
        <div className="flex flex-wrap items-center gap-(--space-2)">
          <label className="relative inline-flex items-center">
            <span className="sr-only">Search members</span>
            <Search
              size={14}
              strokeWidth={1.5}
              aria-hidden="true"
              className="pointer-events-none absolute left-(--space-3) text-(--text-muted)"
            />
            <input
              type="text"
              autoFocus
              value={filters.search}
              placeholder="Search..."
              onChange={(event) => search(event.target.value)}
              className={cn(
                "text-body h-(--control-height) w-56 rounded-(--radius-control) border border-(--border) bg-(--bg) pr-(--space-3) pl-(--space-8) text-(--text) placeholder:text-(--text-muted)",
                "focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-1 focus-visible:ring-offset-(--bg) focus-visible:outline-none"
              )}
            />
          </label>
          <MembersFilters roles={filters.roles} onChange={setRoles} />
        </div>
      </div>

      {loading ? <MembersSkeleton /> : null}

      {!loading && failed ? (
        <div
          role="alert"
          data-testid="members-load-failed"
          className="text-body flex items-center gap-(--space-3) rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-3) text-(--text-muted)"
        >
          {MEMBERS_LOAD_FAILED_TEXT}
          <Button variant="secondary" size="small" onClick={retry}>
            {MEMBERS_RETRY_LABEL}
          </Button>
        </div>
      ) : null}

      {noMatch ? (
        <p data-testid="members-no-match" className="text-body py-(--space-12) text-center text-(--text-muted)">
          {MEMBERS_NO_MATCH_TEXT}
        </p>
      ) : null}

      {!loading && !failed && !noMatch ? (
        <MembersTable
          workspaceSlug={workspaceSlug}
          rows={visible}
          order={filters.order}
          onOrderChange={setOrder}
          avatarUrl={(path) => resolveFileUrl(path, baseUrl)}
        />
      ) : null}
    </section>
  );
}

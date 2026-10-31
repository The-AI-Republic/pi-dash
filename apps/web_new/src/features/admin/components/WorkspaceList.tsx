// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Instance workspace list (ADM-038–042, from parity/specs/admin.md
// §5.2–§5.6): a titled section with a count badge over every workspace
// on the instance. Each row deep-links into the main web app at the
// workspace slug in a new tab through the F-09 coexistence helper;
// there is no in-console detail and no delete or edit action. Further
// pages append through the load-more control; the section is a
// skeleton on first load, a spinner rides beside the count during
// pagination or refresh, and an empty instance shows a zero count
// with no rows and no illustration.
import { Link } from "@tanstack/react-router";
import type { AdminWorkspace } from "@pidash/api-client";
import { Badge, Button, Skeleton, Spinner, Tooltip } from "@pidash/kit";
import * as React from "react";

import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { useInstanceWorkspaces } from "../hooks.js";

/** Section-header shortcut target: the create-workspace page (NEWFRONT-513). */
export const WORKSPACE_CREATE_PATH = "/god-mode/workspace/create/";

/** Skeleton rows standing in for the list on first load (§5.5). */
export const WORKSPACE_LIST_SKELETON_ROWS = 4;

/** Explanatory hint anchored to each row's slug. */
export const WORKSPACE_SLUG_TOOLTIP = "The short name used in this workspace's web address.";

type RouterLinkProps = Parameters<typeof Link>[0];

// The create page is not in the route tree yet, so the typed `to`
// cannot name it; the cast carries the literal through the same way
// CrossAppLink reaches out-of-tree paths.
const createLinkProps = { to: WORKSPACE_CREATE_PATH } as RouterLinkProps;

function countLabel(count: number, singular: string, plural: string): string {
  return count === 1 ? `1 ${singular}` : `${count} ${plural}`;
}

/** Row logo: the workspace image, or the name's first letter when unset. */
function WorkspaceLogo({ name, logoUrl }: { name: string; logoUrl: string | null }): React.ReactElement {
  if (logoUrl !== null && logoUrl !== "") {
    return (
      <img
        src={logoUrl}
        alt=""
        data-testid="admin-workspace-row-logo"
        className="size-10 shrink-0 rounded-(--radius-card) object-cover"
      />
    );
  }
  return (
    <span
      aria-hidden="true"
      data-testid="admin-workspace-row-letter"
      className="text-emphasis flex size-10 shrink-0 items-center justify-center rounded-(--radius-card) bg-(--subtle) font-medium text-(--text-muted)"
    >
      {name.slice(0, 1)}
    </span>
  );
}

/** One workspace row: an external link into the main web app (§5.3). */
function WorkspaceListRow({ workspace }: { workspace: AdminWorkspace }): React.ReactElement {
  const ownerEmail = workspace.owner?.email ?? null;
  const totalProjects = workspace.total_projects ?? null;
  const totalMembers = workspace.total_members ?? null;
  const counts: string[] = [];
  if (totalProjects !== null) counts.push(countLabel(totalProjects, "project", "projects"));
  if (totalMembers !== null) counts.push(countLabel(totalMembers, "member", "members"));
  return (
    <li>
      <CrossAppLink
        to={`/${workspace.slug}`}
        target="_blank"
        rel="noreferrer"
        data-testid="admin-workspace-row"
        className="flex items-center gap-(--space-4) rounded-(--radius-card) border border-(--border) bg-(--bg) p-(--space-4)"
      >
        <WorkspaceLogo name={workspace.name} logoUrl={workspace.logo_url} />
        <span className="flex min-w-0 flex-1 flex-col gap-(--space-1)">
          <span data-testid="admin-workspace-row-name" className="text-emphasis font-medium text-(--text)">
            {workspace.name}
          </span>
          <Tooltip content={WORKSPACE_SLUG_TOOLTIP}>
            <span data-testid="admin-workspace-row-slug" className="text-body w-fit text-(--text-muted)">
              {workspace.slug}
            </span>
          </Tooltip>
        </span>
        <span className="text-body flex shrink-0 flex-col items-end gap-(--space-1) text-(--text-muted)">
          {ownerEmail === null ? null : <span data-testid="admin-workspace-row-owner">Owner {ownerEmail}</span>}
          {counts.length === 0 ? null : <span data-testid="admin-workspace-row-counts">{counts.join(" · ")}</span>}
        </span>
      </CrossAppLink>
    </li>
  );
}

/** Section chrome shared by the loading, error, and loaded bodies. */
function WorkspaceListFrame({
  count,
  spinning,
  children,
}: {
  count: number | null;
  spinning: boolean;
  children: React.ReactNode;
}): React.ReactElement {
  return (
    <section
      aria-labelledby="admin-workspace-list-heading"
      data-testid="admin-workspace-list"
      className="flex flex-col gap-(--space-4)"
    >
      <div className="flex items-center gap-(--space-3)">
        <h2 id="admin-workspace-list-heading" className="text-emphasis font-medium text-(--text)">
          Workspaces
        </h2>
        {count === null ? null : (
          <span data-testid="admin-workspace-count">
            <Badge>{count}</Badge>
          </span>
        )}
        {spinning ? (
          <span data-testid="admin-workspace-count-spinner">
            <Spinner size="small" label="Refreshing the workspace list" />
          </span>
        ) : null}
        <span aria-hidden="true" className="flex-1" />
        <Link
          {...createLinkProps}
          data-testid="admin-workspace-create"
          className="text-body shrink-0 font-medium text-(--accent) hover:underline"
        >
          Create workspace
        </Link>
      </div>
      <p data-testid="admin-workspace-helper" className="text-body text-(--text-muted)">
        Workspaces can&apos;t be deleted here. You can open a workspace only if you&apos;re a member of it.
      </p>
      {children}
    </section>
  );
}

export function WorkspaceList(): React.ReactElement {
  const list = useInstanceWorkspaces();
  if (list.isPending) {
    return (
      <WorkspaceListFrame count={null} spinning={false}>
        <span
          role="status"
          aria-label="Loading workspaces"
          data-testid="admin-workspace-list-skeleton"
          className="flex flex-col gap-(--space-3)"
        >
          {Array.from({ length: WORKSPACE_LIST_SKELETON_ROWS }, (_, index) => (
            <Skeleton key={index} height="74px" />
          ))}
        </span>
      </WorkspaceListFrame>
    );
  }
  if (list.isError) {
    return (
      <WorkspaceListFrame count={null} spinning={false}>
        <p role="alert" data-testid="admin-workspace-list-error" className="text-body text-(--danger)">
          The workspace list could not be loaded.
        </p>
        <Button
          type="button"
          variant="secondary"
          size="small"
          onClick={() => void list.refetch()}
          data-testid="admin-workspace-list-retry"
          className="self-start"
        >
          Try again
        </Button>
      </WorkspaceListFrame>
    );
  }
  const rows = list.data.pages.flatMap((page) => page.results);
  return (
    <WorkspaceListFrame count={rows.length} spinning={list.isFetching}>
      {rows.length === 0 ? null : (
        <ul data-testid="admin-workspace-rows" className="flex list-none flex-col gap-(--space-3) p-0">
          {rows.map((workspace) => (
            <WorkspaceListRow key={workspace.id} workspace={workspace} />
          ))}
        </ul>
      )}
      {list.hasNextPage ? (
        <Button
          type="button"
          variant="secondary"
          loading={list.isFetchingNextPage}
          onClick={() => void list.fetchNextPage()}
          data-testid="admin-workspace-load-more"
          className="self-start"
        >
          Load more
        </Button>
      ) : null}
    </WorkspaceListFrame>
  );
}

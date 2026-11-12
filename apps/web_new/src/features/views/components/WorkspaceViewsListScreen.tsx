// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace views list screen (parity/specs/views.md §9, VIEW-029-032/039).
// Reads the workspace's custom views once on mount, keeps the four static
// defaults visible at all times, and filters both groups by the name typed
// in the search field. The tab title is the workspace name plus "All Views".
// Guests reach this page: the server narrows the custom read to the rows
// they own, and the screen renders whatever it receives.

import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import type { SavedView } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { useWorkspace } from "../../../core/session/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { workspaceViewsQueryOptions } from "../api/queries.js";
import {
  resolveWorkspaceCustomsState,
  selectWorkspaceViewGroups,
  sortWorkspaceViewsNewestFirst,
  workspaceViewsTitle,
} from "../lib/workspaceViews.js";
import { WorkspaceDefaultViewRow } from "./WorkspaceDefaultViewRow.js";
import { WorkspaceViewsCustoms } from "./WorkspaceViewsCustoms.js";
import { WorkspaceViewsSearch } from "./WorkspaceViewsSearch.js";

export interface WorkspaceViewsListScreenProps {
  workspaceSlug: string;
  /** Opens the edit dialog for a custom view. Slot owner: NEWFRONT-726. */
  onEditView?: ((view: SavedView) => void) | undefined;
  /** Opens the delete confirmation for a custom view. Slot owner: NEWFRONT-726. */
  onDeleteView?: ((view: SavedView) => void) | undefined;
}

export function WorkspaceViewsListScreen({
  workspaceSlug,
  onEditView,
  onDeleteView,
}: WorkspaceViewsListScreenProps): React.ReactElement {
  const [query, setQuery] = React.useState("");
  const customs = useQuery(workspaceViewsQueryOptions(getAppClient(), workspaceSlug));
  const { workspace } = useWorkspace(workspaceSlug);

  useDocumentHead({ title: workspaceViewsTitle(workspace?.name), robots: "noindex, nofollow" });

  const fetched = React.useMemo(() => sortWorkspaceViewsNewestFirst(customs.data ?? []), [customs.data]);
  const groups = React.useMemo(() => selectWorkspaceViewGroups(fetched, query), [fetched, query]);
  const state = resolveWorkspaceCustomsState(customs.isPending, customs.isError);

  return (
    <section aria-label="Workspace views" data-testid="workspace-views-screen" className="flex min-h-0 flex-1 flex-col">
      <WorkspaceViewsSearch query={query} onQueryChange={setQuery} />
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        {groups.defaults.length === 0 ? null : (
          <ul data-testid="workspace-views-defaults" className="flex flex-col">
            {groups.defaults.map((view) => (
              <WorkspaceDefaultViewRow key={view.key} workspaceSlug={workspaceSlug} view={view} />
            ))}
          </ul>
        )}
        <WorkspaceViewsCustoms
          state={state}
          views={groups.customs}
          searchHidesRows={fetched.length > 0}
          workspaceSlug={workspaceSlug}
          onEdit={onEditView}
          onDelete={onDeleteView}
          onRetry={() => {
            void customs.refetch();
          }}
        />
      </div>
    </section>
  );
}

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module quick look (parity/specs/modules.md §5, MOD-019). One panel
// over the list or the archived tab, driven by the `peekModule` key. It
// mounts the shared work-item peek chrome — the same frame the issues area
// uses — and fills it with the module sidebar in the viewer's mode, so the
// detail page and the peek never drift into two different sidebars. A
// short work-item summary sits above the sidebar, read from the same detail
// record the sidebar uses.
//
// The panel does not own its address: the screen hands it the close writer
// and hides it when the key leaves. A live mount follows the viewer's
// project role, so an editor can change the module from the panel exactly
// as on the detail page (archives ARCH-025 compares the archived panel with
// this live twin); an archived mount reads the archived detail and stays
// read-only in every role. A module that no longer resolves shows the
// missing state here (never a blank panel), while the address stays
// reachable so the reader can close it.

import { Skeleton } from "@pidash/kit";
import type { ApiClient, ModuleDetail, ArchivedModuleDetail } from "@pidash/api-client";
import { useQuery } from "@tanstack/react-query";
import { AlertCircle } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useProjectRoles } from "../../auth/index.js";
import { PeekChrome, type PeekMode } from "../../issues/index.js";
import { isProjectAuthorized } from "../../projects/index.js";
import { ModuleAnalyticsSidebar, ModuleProgressSection } from "./sidebar/index.js";
import { moduleSidebarDetailQueryOptions } from "./sidebar/moduleSidebarQueries.js";
import { moduleWorkItemCountText } from "../lib/moduleIdentity.js";

export const MODULE_PEEK_LABEL = "Module preview";
export const MODULE_PEEK_MISSING_TITLE = "Module does not exist";
export const MODULE_PEEK_MISSING_BODY =
  "It may have been deleted or archived, or you may not have access to it. Close this panel to go back.";

type ModulePeekRecord = ModuleDetail | ArchivedModuleDetail;

export interface ModulePeekPanelProps {
  workspaceSlug: string;
  projectId: string;
  /** The module the peek names. */
  moduleId: string;
  /** A name the list already knows, shown while the detail read lands. */
  fallbackName?: string | undefined;
  /** Read the archived record instead of the live one. */
  archived?: boolean | undefined;
  /** Remove the peek key from the address and hide the panel. */
  onClose: () => void;
  /** Test seam; the app client by default. */
  client?: ApiClient | undefined;
}

/** The done-over-total line under the panel's title. */
export function modulePeekSummary(record: Pick<ModuleDetail, "completed_issues" | "total_issues">): string {
  return moduleWorkItemCountText(record);
}

function PeekSummary({ record }: { record: ModulePeekRecord }): React.ReactElement {
  return (
    <p data-testid="modules-peek-summary" className="text-meta text-(--text-muted)">
      {modulePeekSummary(record)}
    </p>
  );
}

function PeekMissing({ onClose }: { onClose: () => void }): React.ReactElement {
  return (
    <div data-testid="modules-peek-missing" role="alert" className="flex flex-col items-start gap-(--space-2)">
      <p className="text-body flex items-center gap-(--space-2) font-medium text-(--text)">
        <AlertCircle size={16} strokeWidth={1.5} aria-hidden="true" className="text-(--warning)" />
        {MODULE_PEEK_MISSING_TITLE}
      </p>
      <p className="text-body text-(--text-muted)">{MODULE_PEEK_MISSING_BODY}</p>
      <button
        type="button"
        onClick={onClose}
        className="text-body rounded-(--radius-control) font-medium text-(--accent) hover:underline"
      >
        Close preview
      </button>
    </div>
  );
}

function PeekSkeleton(): React.ReactElement {
  return (
    <div
      role="status"
      aria-label="Loading module"
      data-testid="modules-peek-skeleton"
      className="flex flex-col gap-(--space-3)"
    >
      <Skeleton width="40%" height="16px" />
      <Skeleton width="70%" height="20px" />
      <Skeleton height="20px" />
      <Skeleton height="72px" />
    </div>
  );
}

export function ModulePeekPanel({
  workspaceSlug,
  projectId,
  moduleId,
  fallbackName,
  archived = false,
  onClose,
  client = getAppClient(),
}: ModulePeekPanelProps): React.ReactElement {
  const [mode, setMode] = React.useState<PeekMode>("side");
  const roles = useProjectRoles(workspaceSlug);
  const role = roles.data?.[projectId] ?? null;
  const canEdit = isProjectAuthorized(role);

  const detail = useQuery(moduleSidebarDetailQueryOptions(client, workspaceSlug, projectId, moduleId, archived));
  const record = detail.data;

  // A missing module (a stale shared link, or one archived from a live
  // address) is a settled answer, not a retry loop: the panel keeps its
  // close control and renders the missing state.
  const missing = detail.isError || (detail.isSuccess && record === undefined);

  return (
    <PeekChrome
      mode={mode}
      onModeChange={setMode}
      title={record?.name ?? fallbackName ?? ""}
      titleAsHeading
      eyebrow={archived ? "Archived module" : "Module"}
      onClose={onClose}
      testId="modules-peek"
    >
      {missing ? (
        <PeekMissing onClose={onClose} />
      ) : record === undefined ? (
        <PeekSkeleton />
      ) : (
        <div data-testid="modules-peek-body" className="flex flex-col gap-(--space-4)">
          <PeekSummary record={record} />
          <ModuleAnalyticsSidebar
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            moduleId={moduleId}
            readOnly={archived}
            archived={archived}
            canEdit={canEdit}
            client={client}
            progress={
              <ModuleProgressSection
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                moduleId={moduleId}
                readOnly={archived}
                archived={archived}
                canEdit={canEdit}
                client={client}
              />
            }
          />
        </div>
      )}
    </PeekChrome>
  );
}

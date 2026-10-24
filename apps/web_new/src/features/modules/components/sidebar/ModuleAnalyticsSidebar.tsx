// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The single module analytics sidebar (parity/specs/modules.md §1.3,
// §9.1, MOD-033). The detail page mounts it read-write for admins and
// members; the peek and archived peek mount the same tree with
// `readOnly` and/or `archived`, so no surface gets a second variant. It
// shows the module's status, name, description, date range, lead,
// members, completed-over-total work items and, only for a points
// estimate system, completed-over-total points. Every edit is a partial
// PATCH through the shared mutation hook; the `progress` slot lets slice L
// insert the progress section below these fields, and slice M's links
// section closes the tree below it.

import { Button, IconButton, Skeleton } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import type { ApiClient, ModuleStatus } from "@pidash/api-client";
import { X } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import { useProjectRoles } from "../../../auth/index.js";
import {
  DateRangePicker,
  SidebarRow,
  SIDEBAR_ROW_CLASS,
  SIDEBAR_ROW_LABEL_CLASS,
  useWeekStart,
  type RangeValue,
} from "../../../issues/index.js";
import { isProjectAuthorized } from "../../../projects/index.js";
import { MemberPicker } from "../../../../shared/pickers/index.js";
import { formatModuleDay, parseModuleDay } from "../../lib/moduleDates.js";
import { ModuleDescriptionField, ModuleNameField } from "./ModuleIdentityFields.js";
import { ModuleLinksSection } from "./ModuleLinksSection.js";
import { ModuleStatusPicker } from "./ModuleStatusPicker.js";
import {
  MODULE_SAVE_FAILED_BODY,
  moduleCountText,
  moduleSidebarEditable,
  MODULE_STATUS_LABELS,
  showsModulePoints,
} from "./moduleSidebarModel.js";
import {
  moduleSidebarDetailQueryOptions,
  moduleSidebarEstimatesQueryOptions,
  type ModuleSidebarRecord,
} from "./moduleSidebarQueries.js";
import { useModuleSidebarWrite } from "./useModuleSidebarWrite.js";

export const MODULE_SIDEBAR_LOAD_FAILED = "Couldn't load the module.";
const ROW_VARIANT = "ghost";

/** One end of the sidebar's range: a padded day, or the pinned placeholder. */
function DateEnd({ iso, placeholder }: { iso: string | null; placeholder: string }): React.ReactElement {
  const day = parseModuleDay(iso);
  return (
    <span className={day === null ? "text-(--text-muted)" : undefined}>
      {day === null ? placeholder : formatModuleDay(day)}
    </span>
  );
}

export interface ModuleAnalyticsSidebarProps {
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
  /** The peek's read-only mount; the detail page leaves it false. */
  readOnly?: boolean;
  /** The archived peek: every field is disabled even for an editor. */
  archived?: boolean;
  /**
   * Explicit editor standing. Omit to read the project role map; tests
   * and hosts that already resolved access may pass it directly.
   */
  canEdit?: boolean;
  /** Slice L's progress section, mounted below the count rows. */
  progress?: React.ReactNode;
  /**
   * Detail-page close control (NEWFRONT-802): the button renders only
   * when this is set, so the peek keeps its shell close. It writes
   * through the same persisted collapse flag as the header toggle.
   */
  onClose?: (() => void) | undefined;
  /** Test seam; the app client by default. */
  client?: ApiClient;
}

function sidebarRecordStatus(record: ModuleSidebarRecord): ModuleStatus {
  return record.status as ModuleStatus;
}

/** True when the record itself is an archived read, flag or not. */
export function isArchivedRecord(record: ModuleSidebarRecord): boolean {
  return "archived_at" in record && record.archived_at !== null;
}

export function ModuleAnalyticsSidebar({
  workspaceSlug,
  projectId,
  moduleId,
  readOnly = false,
  archived = false,
  canEdit,
  progress,
  onClose,
  client = getAppClient(),
}: ModuleAnalyticsSidebarProps): React.ReactElement {
  const roles = useProjectRoles(workspaceSlug);
  const role = roles.data?.[projectId] ?? null;
  const editor = canEdit ?? isProjectAuthorized(role);

  const detail = useQuery(moduleSidebarDetailQueryOptions(client, workspaceSlug, projectId, moduleId, archived));
  const estimates = useQuery(moduleSidebarEstimatesQueryOptions(client, workspaceSlug, projectId));

  if (detail.data === undefined) {
    if (detail.isError) {
      return (
        <div className="flex flex-col items-start gap-(--space-2)" role="alert">
          <p className="text-meta text-(--text-muted)">{MODULE_SIDEBAR_LOAD_FAILED}</p>
          <Button variant="secondary" size="small" onClick={() => void detail.refetch()}>
            Try again
          </Button>
        </div>
      );
    }
    return <ModuleAnalyticsSidebarSkeleton />;
  }

  return (
    <ModuleAnalyticsSidebarContent
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      record={detail.data}
      readOnly={readOnly}
      archived={archived || isArchivedRecord(detail.data)}
      editor={editor}
      showPoints={showsModulePoints(estimates.data)}
      progress={progress}
      onClose={onClose}
      client={client}
    />
  );
}

interface ModuleAnalyticsSidebarContentProps {
  workspaceSlug: string;
  projectId: string;
  record: ModuleSidebarRecord;
  readOnly: boolean;
  archived: boolean;
  editor: boolean;
  showPoints: boolean;
  progress: React.ReactNode;
  onClose?: (() => void) | undefined;
  client: ApiClient;
}

export function ModuleAnalyticsSidebarContent({
  workspaceSlug,
  projectId,
  record,
  readOnly,
  archived,
  editor,
  showPoints,
  progress,
  onClose,
  client,
}: ModuleAnalyticsSidebarContentProps): React.ReactElement {
  const weekStart = useWeekStart();
  const editable = moduleSidebarEditable({ readOnly, archived, canEdit: editor });
  const write = useModuleSidebarWrite({ client, workspaceSlug, projectId, moduleId: record.id });
  const status = sidebarRecordStatus(record);
  const range: RangeValue = { start: record.start_date, end: record.target_date };

  return (
    <div className="flex flex-col gap-(--space-4)" data-testid="modules-analytics-sidebar">
      {onClose === undefined ? null : (
        <div className="flex items-center justify-end">
          <IconButton
            label="Close sidebar"
            variant="ghost"
            size="small"
            data-testid="modules-sidebar-close"
            onClick={onClose}
          >
            <X size={14} strokeWidth={2} aria-hidden="true" />
          </IconButton>
        </div>
      )}
      <div className="flex flex-col gap-(--space-2)">
        <ModuleStatusPicker value={status} disabled={!editable} onChange={(next) => write.mutate({ status: next })} />
        <h4 className="text-h3 min-w-0 font-semibold break-words text-(--text)">
          <ModuleNameField value={record.name} disabled={!editable} onSave={(name) => write.mutate({ name })} />
        </h4>
        <ModuleDescriptionField
          value={record.description}
          disabled={!editable}
          onSave={(description) => write.mutate({ description })}
        />
      </div>

      <div className="flex flex-col gap-(--space-1)">
        <SidebarRow label="Date range">
          <DateRangePicker
            value={range}
            mode="merged"
            weekStart={weekStart}
            label="Date range"
            fromPlaceholder="Start date"
            toPlaceholder="End date"
            disabled={!editable}
            testId="modules-sidebar-dates"
            trigger={
              <button
                type="button"
                disabled={!editable}
                data-testid="modules-sidebar-dates-trigger"
                className="text-body inline-flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:text-(--text-muted) disabled:hover:bg-transparent"
              >
                <DateEnd iso={record.start_date} placeholder="Start date" />{" "}
                <DateEnd iso={record.target_date} placeholder="End date" />
              </button>
            }
            onChange={(next) => {
              const patch: { start_date: string | null; target_date: string | null } = {
                start_date: next.start,
                target_date: next.end,
              };
              write.mutate(patch);
            }}
          />
        </SidebarRow>
        <SidebarRow label="Lead">
          <MemberPicker
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            value={record.lead_id}
            disabled={!editable}
            variant={ROW_VARIANT}
            placeholder="Lead"
            preload={record.lead_id !== null}
            onChange={(leadId) => write.mutate({ lead_id: leadId })}
          />
        </SidebarRow>
        <SidebarRow label="Members">
          <MemberPicker
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            multiple
            value={record.member_ids}
            disabled={!editable}
            variant={ROW_VARIANT}
            placeholder="Members"
            preload={record.member_ids.length > 0}
            onChange={(memberIds) => write.mutate({ member_ids: memberIds })}
          />
        </SidebarRow>
        <SidebarRow label="Work items">
          <span className="text-body text-(--text-muted)">
            {moduleCountText(record.completed_issues, record.total_issues)}
          </span>
        </SidebarRow>
        {showPoints ? (
          <SidebarRow label="Points">
            <span className="text-body text-(--text-muted)">
              {moduleCountText(record.completed_estimate_points, record.total_estimate_points)}
            </span>
          </SidebarRow>
        ) : null}
      </div>

      {progress}
      <ModuleLinksSection
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        moduleId={record.id}
        links={record.link_module}
        canEdit={editable}
        client={client}
      />
    </div>
  );
}

/** Placeholder rows while the detail and the points gate hydrate. */
export function ModuleAnalyticsSidebarSkeleton(): React.ReactElement {
  return (
    <div
      className="flex flex-col gap-(--space-2)"
      role="status"
      aria-label="Loading module"
      data-testid="modules-analytics-sidebar-skeleton"
    >
      <Skeleton width="80px" height="20px" />
      <Skeleton width="70%" height="20px" />
      <Skeleton width="100%" height="56px" />
      {[0, 1, 2, 3, 4].map((index) => (
        <div key={index} className={SIDEBAR_ROW_CLASS}>
          <span className={SIDEBAR_ROW_LABEL_CLASS} />
          <Skeleton height="20px" />
        </div>
      ))}
    </div>
  );
}

export { MODULE_SAVE_FAILED_BODY, MODULE_STATUS_LABELS };

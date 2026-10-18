// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List layout shell (parity/specs/issues.md §2, ISS-007–014). Renders
// one section for the flat list or one per group value; sections collapse
// through the shared stored set, empty sections follow the display flag,
// grouped sections page with an explicit row while the flat list loads
// the next page from a scroll sentinel, and each section ends with its
// quick-add. Rows own peek and expansion.

import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import { Skeleton, VirtualList } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { usePermissions } from "../../../core/session/index.js";
import { projectDetailsQueryOptions, projectsQueryOptions } from "../../projects/index.js";
import { isCollapsed, readCollapsedGroups, useToggleCollapsedGroup } from "./collapse.js";

import { creationDisabledForGroup, sectionPrefill } from "./grouping.js";
import { GroupHeader } from "./GroupHeader.js";

import { ListRow } from "./ListRow.js";
import { useListPages } from "./pages.js";

import { useListReferences } from "./reference.js";
import { prefsQueryOptions } from "../prefs/prefsQuery.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { addIssuesToModule, addIssueToCycle } from "@pidash/api-client";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import type { RichExpression } from "../filters/rich/expression.js";
import type { DisplayFeatureFlags } from "../display/matrix.js";
import { useCreateWorkItem } from "../api/create.js";

// Overlays load on first use: the list route itself must stay inside the
// initial-JS budget (Quality gates).
const LazyAddExistingModal = React.lazy(() =>
  import("./AddExistingModal.js").then((module) => ({ default: module.AddExistingModal }))
);
const LazyListPeek = React.lazy(() => import("./ListPeek.js").then((module) => ({ default: module.ListPeek })));
const LazyCreateDialog = React.lazy(() =>
  import("./CreateDialog.js").then((module) => ({ default: module.CreateDialog }))
);
const LazyQuickAdd = React.lazy(() => import("./QuickAdd.js").then((module) => ({ default: module.QuickAdd })));

/** Flat-list sentinel that loads the next page when scrolled into view. */
function useSentinel(onIntersect: () => void, enabled: boolean): React.RefObject<HTMLDivElement | null> {
  const ref = React.useRef<HTMLDivElement | null>(null);
  React.useEffect(() => {
    if (!enabled || ref.current === null) return;
    const node = ref.current;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) onIntersect();
      },
      { rootMargin: "200px" }
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [enabled, onIntersect]);
  return ref;
}

/** Grouping title maps keyed by the display dimension. */
/** One-line diagnostic used by the route tests to inspect paging state. */
export function groupLabelsFor(prefs: DisplayPrefs, refs: ReturnType<typeof useListReferences>) {
  return refs.labels[prefs.groupBy ?? ""] ?? new Map<string, string>();
}

export interface ListLayoutProps {
  workspaceSlug: string;
  entity: IssuesEntity;
  projectId: string;
  /** Project identifier prefix for row keys. */
  projectIdentifier: string;
  prefs: DisplayPrefs;
  /** Cycle target for creation inside a cycle page. */
  cycleId?: string | undefined;
  /** Module target for creation inside a module page. */
  moduleId?: string | undefined;
  /** Completed cycle pages render read-only. */
  completedCycle?: boolean | undefined;
  /** Controlled peek key (the route owns the URL param). */
  peekKey?: string | null | undefined;
  onPeekChange?: ((key: string | null) => void) | undefined;
  /** Compact density from the layout search param. */
  compact?: boolean | undefined;
  /** Stored rich filters scoping every page (ISS-095–099). */
  richFilters?: RichExpression | undefined;
}

/**
 * Virtualized flat rows (ISS-010). The perf budget pins the issue list at
 * 2,000 rows with every frame under 50 ms, so only the viewport window
 * mounts; grouped sections stay unvirtualized because each bucket is
 * paged to fifty. The kit's VirtualList owns the scroll region.
 */
function FlatRows({
  rows,
  renderRow,
  rowHeight,
}: {
  rows: IssueListItem[];
  renderRow: (issue: IssueListItem) => React.ReactNode;
  rowHeight: number;
}): React.ReactElement {
  return (
    <VirtualList
      label="Issues"
      items={rows}
      estimateSize={rowHeight}
      getKey={(issue) => issue.id}
      overscan={4}
      className="h-full"
      renderRow={(issue) => renderRow(issue)}
    />
  );
}

/** Create-modal state owned by the layout. */
interface CreateModalState {
  open: boolean;
  prefill: Record<string, string | string[]>;
}

export function ListLayout({
  workspaceSlug,
  entity,
  projectId,
  projectIdentifier,
  prefs,
  cycleId,
  moduleId,
  completedCycle = false,
  peekKey: controlledPeekKey,
  onPeekChange,
  compact = false,
  richFilters = {},
}: ListLayoutProps): React.ReactElement {
  const client = getAppClient();
  const permissions = usePermissions(workspaceSlug);
  const { data: prefsRecord } = useSuspenseQuery(prefsQueryOptions(client, workspaceSlug, entity));
  const collapsedState = readCollapsedGroups(prefsRecord.filters);
  const collapse = useToggleCollapsedGroup(workspaceSlug, entity);
  const refs = useListReferences({ workspaceSlug, projectId, dimension: prefs.groupBy });
  const labels = groupLabelsFor(prefs, refs);
  const pages = useListPages(
    workspaceSlug,
    entity,
    prefs.groupBy,
    prefs.orderBy,
    labels,
    prefs.showEmptyGroups,
    richFilters
  );
  const { data: projects } = useQuery(projectsQueryOptions(client, workspaceSlug));
  const { data: details } = useQuery(projectDetailsQueryOptions(client, workspaceSlug));
  const project = projects?.find((row) => row.id === projectId) ?? null;
  const projectDetails = details?.find((row) => row.id === projectId) ?? null;
  const estimateEnabled = projectDetails !== null && projectDetails.estimate !== null;
  const [uncontrolledPeekKey, setUncontrolledPeekKey] = React.useState<string | null>(null);
  const peekKey = controlledPeekKey === undefined ? uncontrolledPeekKey : controlledPeekKey;
  const [createState, setCreateState] = React.useState<CreateModalState | null>(null);
  const [addExistingOpen, setAddExistingOpen] = React.useState(false);
  const create = useCreateWorkItem(workspaceSlug);

  const readOnly = !permissions.canEdit || completedCycle;
  const flags: DisplayFeatureFlags = {
    hideCycle: !project?.cycle_view,
    hideModule: !project?.module_view,
  };

  const allRows = pages.sections.flatMap((section) => section.rows);
  const peekIssue =
    peekKey === null ? null : (allRows.find((row) => `${projectIdentifier}-${row.sequence_id}` === peekKey) ?? null);

  function setPeek(key: string | null): void {
    if (controlledPeekKey === undefined) setUncontrolledPeekKey(key);
    onPeekChange?.(key);
  }

  function openPeek(key: string): void {
    setPeek(key);
  }

  function closePeek(): void {
    setPeek(null);
  }

  const quickAddDisabled = !permissions.canEdit || completedCycle || creationDisabledForGroup(prefs.groupBy);
  const canAdd = permissions.canEdit && !completedCycle && !creationDisabledForGroup(prefs.groupBy);
  const menu = cycleId !== undefined || moduleId !== undefined;

  function openCreate(prefill: Record<string, string | string[]>): void {
    setCreateState({ open: true, prefill });
  }

  async function submitCreate(name: string, prefill: Record<string, string | string[]>): Promise<void> {
    try {
      const created = await create.mutateAsync({
        projectId,
        payload: {
          name,
          ...prefill,
          ...(prefill["state_id"] === undefined &&
          projectDetails?.default_state !== null &&
          projectDetails?.default_state !== undefined
            ? { state_id: projectDetails.default_state }
            : {}),
          ...(cycleId === undefined ? {} : { cycle_id: cycleId }),
          ...(moduleId === undefined ? {} : { module_ids: [moduleId] }),
        },
      });
      if (cycleId !== undefined) {
        await addIssueToCycle(client, workspaceSlug, projectId, cycleId, created.id).catch(() => undefined);
      }
      if (moduleId !== undefined) {
        await addIssuesToModule(client, workspaceSlug, projectId, moduleId, [created.id]).catch(() => undefined);
      }
      setCreateState(null);
    } catch {
      // The create path already toasted the failure.
    }
  }

  async function attachExisting(ids: string[]): Promise<void> {
    try {
      if (cycleId !== undefined) {
        for (const id of ids) await addIssueToCycle(client, workspaceSlug, projectId, cycleId, id);
      } else if (moduleId !== undefined) {
        await addIssuesToModule(client, workspaceSlug, projectId, moduleId, ids);
      }
      notifySuccess(ids.length === 1 ? "Work item added" : "Work items added");
      setAddExistingOpen(false);
    } catch {
      notifyError("Couldn't add the work items", "Check your connection and try again.");
    }
  }

  function hrefFor(issue: IssueListItem): string {
    return `/${workspaceSlug}/browse/${projectIdentifier}-${issue.sequence_id}`;
  }

  const sentinelRef = useSentinel(pages.loadMore, !pages.grouped && pages.hasMore);

  if (pages.isPending) {
    return (
      <div data-testid="list-loading" className="flex flex-col gap-(--space-2) p-(--space-3)">
        {Array.from({ length: 6 }, (_, index) => (
          <Skeleton key={index} height="32px" />
        ))}
      </div>
    );
  }

  const sectionGroups = pages.sections;

  function renderListRow(issue: IssueListItem): React.ReactElement {
    return (
      <ListRow
        key={issue.id}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectIdentifier={projectIdentifier}
        issue={issue}
        depth={1}
        properties={prefs.properties}
        flags={flags}
        readOnly={readOnly}
        states={refs.stateRows}
        stateNames={refs.stateNames}
        assigneeNames={refs.assigneeNames}
        labelNames={refs.labelNames}
        cycleNames={refs.cycleNames}
        moduleNames={refs.moduleNames}
        openCycleIds={refs.openCycleIds}
        estimateEnabled={estimateEnabled}
        highlightedId={null}
        onOpenPeek={openPeek}
        hrefFor={hrefFor}
        compact={compact}
        deferPickers={!pages.grouped}
      />
    );
  }

  return (
    <div
      {...(pages.grouped ? { role: "region", "aria-label": "Issues" } : { "data-testid": "list-scroll" })}
      className="relative flex h-full flex-col overflow-auto pb-16"
    >
      {peekIssue === null ? null : (
        <React.Suspense fallback={null}>
          <LazyListPeek issueKey={peekKey ?? ""} title={peekIssue.name} onClose={closePeek} />
        </React.Suspense>
      )}
      {sectionGroups.map((section) => {
        const sectionCollapsed = isCollapsed(collapsedState, "group_by", section.key);
        return (
          <section
            key={section.key}
            data-drop-target-for-element="true"
            className={pages.grouped ? undefined : "flex min-h-0 flex-1 flex-col"}
          >
            <GroupHeader
              section={section}
              dimension={prefs.groupBy}
              collapsed={sectionCollapsed}
              onToggle={() => collapse.toggleCollapsedGroup("group_by", section.key)}
              canAdd={canAdd}
              menu={menu}
              onCreate={() => openCreate({ ...sectionPrefill(prefs.groupBy, section) })}
              onAddExisting={() => setAddExistingOpen(true)}
            />
            {sectionCollapsed ? null : (
              <>
                {pages.grouped ? (
                  section.rows.map((issue) => renderListRow(issue))
                ) : (
                  <div className="min-h-0 flex-1">
                    <FlatRows rows={section.rows} renderRow={renderListRow} rowHeight={compact ? 32 : 44} />
                  </div>
                )}
                {pages.grouped && pages.sectionMore(section) ? (
                  <button
                    type="button"
                    onClick={() => pages.loadMoreSection(section)}
                    className="text-body w-full border-b border-(--border) px-(--space-3) py-(--space-2) text-left text-(--accent) hover:bg-(--subtle)"
                  >
                    Load more
                  </button>
                ) : null}
                <React.Suspense fallback={null}>
                  <LazyQuickAdd
                    workspaceSlug={workspaceSlug}
                    projectId={projectId}
                    cycleId={cycleId}
                    moduleId={moduleId}
                    prefill={sectionPrefill(prefs.groupBy, section)}
                    {...(pages.grouped ? { groupKey: section.key } : {})}
                    defaultStateId={projectDetails?.default_state ?? null}
                    disabled={quickAddDisabled}
                    disabledReason={
                      !permissions.canEdit
                        ? "New work items are read-only for you"
                        : completedCycle
                          ? "This cycle is complete"
                          : creationDisabledForGroup(prefs.groupBy)
                            ? "Creation is off in this grouping"
                            : undefined
                    }
                  />
                </React.Suspense>
              </>
            )}
          </section>
        );
      })}
      {!pages.grouped && pages.hasMore ? (
        <div ref={sentinelRef} data-testid="list-sentinel" className="flex flex-col gap-(--space-2) p-(--space-3)">
          {Array.from({ length: 3 }, (_, index) => (
            <Skeleton key={index} height="32px" />
          ))}
        </div>
      ) : null}
      {addExistingOpen ? (
        <React.Suspense fallback={null}>
          <LazyAddExistingModal
            open
            onOpenChange={(open) => {
              if (!open) setAddExistingOpen(false);
            }}
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            excludeCycleMembers={cycleId !== undefined}
            {...(moduleId !== undefined ? { excludeModuleId: moduleId } : {})}
            onAttach={(ids) => attachExisting(ids)}
          />
        </React.Suspense>
      ) : null}
      {createState === null ? null : (
        <React.Suspense fallback={null}>
          <LazyCreateDialog
            open={createState.open}
            onOpenChange={(open) => {
              if (!open) setCreateState(null);
            }}
            onSave={(name) => void submitCreate(name, createState.prefill)}
          />
        </React.Suspense>
      )}
    </div>
  );
}

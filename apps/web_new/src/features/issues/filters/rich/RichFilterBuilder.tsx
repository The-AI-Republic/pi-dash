// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rich-filter builder (parity/specs/issues.md §9, ISS-095–105). One
// component owns the stored expression, the header entry and the
// condition row it toggles. Empty expressions render an "Add filter"
// button that opens the field picker; non-empty ones render an accent
// filter icon toggling the row. Every edit persists through the entity's
// user-properties record, except in temporary mode (ISS-105), which keeps
// the expression in memory and registers its field configs while
// mounted, and in saved-view mode, where edits stay local until "Update
// view" PATCHes the record. The row's right side carries clear-all and
// the view actions (ISS-104). ISS-095 resolves to per-condition removal:
// no chip bar renders on issues surfaces (NEWFRONT-148).

import type { SavedView } from "@pidash/api-client";
import { Button, IconButton, Tooltip } from "@pidash/kit";
import { Filter, Plus } from "lucide-react";
import * as React from "react";

import { ROLE_MEMBER } from "../../../../core/session/index.js";
import { useMe, usePermissions } from "../../../../core/session/index.js";
import { useProjectRoles } from "../../../auth/index.js";
import type { IssuesEntity } from "../../prefs/entities.js";
import { selectDisplayPrefs, useEntityPrefs, type DisplayPrefs } from "../../prefs/displayPrefs.js";
import {
  addCondition,
  hasRichConditions,
  isConditionComplete,
  normalizeRichExpression,
  parseRichExpression,
  removeCondition,
  richFilterFields,
  serializeRichExpression,
  updateCondition,
  usedProperties,
  type RichCondition,
  type RichFilterField,
} from "./expression.js";
// Submit-time serializer for lazy hosts (the views dialog reads its
// temporary registry through the builder-module loader in the barrel).
export { serializeRichExpression } from "./expression.js";
// Stored-expression parser for the same hosts (the edit dialog seeds its
// temporary instance from the record through the same loader).
export { parseRichExpression } from "./expression.js";
import { ConditionPill, conditionField } from "./ConditionPill.js";
import { FieldPicker } from "./FieldPicker.js";
import { temporaryInstanceId, useTemporaryFieldConfigs, type FieldConfigRegistry } from "./temporary.js";
import { useRichFilters, useUpdateRichFilters } from "./useRichFilters.js";
import { ViewActions } from "./ViewActions.js";
import type { ViewContext, ViewSnapshot } from "./views.js";
import { entityViewContext, isViewDirty, viewDisplaySnapshot } from "./views.js";

export interface RichFilterBuilderProps {
  workspaceSlug: string;
  projectId: string;
  entity: IssuesEntity;
  /** Cycle/module properties hide while their project features are off. */
  hideCycle: boolean;
  hideModule: boolean;
  /** In-memory instance: no reads or writes, TEMP- id, configs on mount. */
  temporary?: boolean | undefined;
  /** Custom config registry for the temporary instance; session-wide by default. */
  temporaryRegistry?: FieldConfigRegistry | undefined;
  /**
   * Expression a temporary instance opens with (ISS-071). The archived
   * page restores its own locally stored filters and never reads the
   * server record, so the caller hands the stored tree in here; later
   * edits stay in memory.
   */
  temporarySeed?: readonly RichCondition[] | undefined;
  /** Saved-view record the row edits; absent on bare entity lists. */
  view?: { view: SavedView; isDefault: boolean } | undefined;
  /** Display settings the saved view currently shows (local edits included). */
  viewDisplay?: DisplayPrefs | undefined;
  /** Project view saving waits for the project's views feature. */
  viewsEnabled?: boolean | undefined;
  /** Fired after a view create/update so the host can adopt the record. */
  onViewCreated?: ((view: SavedView) => void) | undefined;
  onViewUpdated?: ((view: SavedView) => void) | undefined;
  /**
   * Views-owned save dialog (parity/specs/views.md §8, VIEW-048). When the
   * host provides it, the row's Save view/Save as entry hands it the live
   * snapshot instead of opening the minimal form.
   */
  onSavePress?: ((snapshot: ViewSnapshot) => void) | undefined;
}

export interface RichFilterBuilderParts {
  /** Header trigger, mounted in the header action slot. */
  control: React.ReactNode;
  /**
   * Conditions the row holds right now. Hosts that query with the
   * expression read it here instead of keeping a second copy that would
   * drift from the row (ISS-071).
   */
  conditions: readonly RichCondition[];
  /** Builder row, mounted beneath the header. */
  row: React.ReactNode;
  /** Client id of this instance: the entity's, or a TEMP- id when ephemeral. */
  instanceId: string;
  /**
   * A condition was introduced through this row during the current visit
   * (ISS-068/070/071). The first-run empty card holds until then, so a
   * stored expression that happens to empty the list reads as its own
   * state rather than as a search the user just made.
   */
  addedThisVisit: boolean;
  /** Empty the expression in one step: the filtered-empty card's entry. */
  clearFilters: () => void;
  /**
   * At least one condition is complete. The empty-state rule needs only
   * this answer, so the host passes it on instead of re-deriving it from
   * the expression (Quality gates: the parser stays in this chunk).
   */
  hasCompleteCondition: boolean;
}

/**
 * Build the header control and row from one shared state instance so the
 * toggle and the row cannot diverge. The caller places `control` in the
 * header and `row` below it.
 */
export function useRichFilterBuilder(props: RichFilterBuilderProps): RichFilterBuilderParts {
  const { workspaceSlug, projectId, entity, hideCycle, hideModule } = props;
  const temporary = props.temporary ?? false;
  const isView = props.view !== undefined;
  const entityPrefs = useEntityPrefs(workspaceSlug, entity, !temporary && !isView);
  const { conditions: stored } = useRichFilters(workspaceSlug, entity, !temporary && !isView);
  const { updateRichFilters, failureCount } = useUpdateRichFilters(workspaceSlug, entity, !isView);
  const permissions = usePermissions(workspaceSlug);
  // View writes are a project power: the old row asks for ADMIN or MEMBER
  // standing on this project, so a workspace member who has not joined it
  // sees no Save view rather than a 403. Workspace admins keep their
  // standing everywhere.
  const projectRoles = useProjectRoles(workspaceSlug);
  const projectRole = projectRoles.data?.[projectId];
  const canWriteViews = permissions.isAdmin || (projectRole !== undefined && projectRole >= ROLE_MEMBER);
  const me = useMe();
  // A persisted instance is named after its entity; a temporary one gets
  // a client-only TEMP- id that dies with the mount.
  const [instanceId] = React.useState(() =>
    temporary ? temporaryInstanceId() : `${entity.scope}:${entityIdOf(entity)}`
  );
  // A temporary row starts from whatever the caller seeded and keeps its
  // own copy for the life of the mount; a saved-view row starts from the
  // view's stored expression and keeps edits local until Update view; a
  // persisted entity row follows the server.
  const [local, setLocal] = React.useState<readonly RichCondition[]>(props.temporarySeed ?? []);
  const [viewConditions, setViewConditions] = React.useState<readonly RichCondition[]>(() =>
    isView ? parseRichExpression(props.view?.view.rich_filters ?? {}) : []
  );
  const [viewRecord, setViewRecord] = React.useState<SavedView | null>(props.view?.view ?? null);
  const viewSource = props.view?.view;
  const viewDefault = props.view?.isDefault ?? false;
  React.useEffect(() => {
    if (viewSource === undefined) return;
    setViewRecord(viewSource);
    setViewConditions(parseRichExpression(viewSource.rich_filters));
  }, [viewSource]);
  const conditions = temporary ? local : isView ? viewConditions : stored;
  const [open, setOpen] = React.useState(true);
  // Set when a condition is added through either add control, and dropped
  // again once the expression is empty (clearing restores the list).
  const [addedThisVisit, setAddedThisVisit] = React.useState(false);
  const fields = React.useMemo(() => richFilterFields({ hideCycle, hideModule }), [hideCycle, hideModule]);
  const [draft, setDraft] = React.useState<readonly RichCondition[] | null>(null);
  const rows = draft ?? conditions;

  const viewContext = React.useMemo<ViewContext>(() => {
    if (viewRecord === null) return entityViewContext();
    return {
      isView: true,
      viewScope: viewRecord.project === null ? "workspace" : "project",
      viewId: viewRecord.id,
      ownerId: viewRecord.owned_by,
      locked: viewRecord.is_locked,
      isDefault: viewDefault,
      storedExpression: normalizeRichExpression(viewRecord.rich_filters),
      storedDisplayFilters: viewRecord.display_filters,
      storedDisplayProperties: viewRecord.display_properties,
    };
  }, [viewDefault, viewRecord]);
  const snapshot = React.useMemo<ViewSnapshot>(() => {
    if (viewRecord !== null) {
      // Filters come from the row and display settings from the view's
      // live display state, so an untouched view never reads as changed
      // and a layout or display switch makes Update view available.
      const display = props.viewDisplay;
      return {
        expression: serializeRichExpression(rows),
        ...(display === undefined
          ? { displayFilters: viewRecord.display_filters, displayProperties: viewRecord.display_properties }
          : viewDisplaySnapshot(viewRecord, display)),
      };
    }
    const record = entityPrefs.data;
    if (record === undefined) {
      return { expression: serializeRichExpression(rows), displayFilters: {}, displayProperties: {} };
    }
    const display = selectDisplayPrefs(record);
    return {
      expression: serializeRichExpression(rows),
      displayFilters: {
        ...record.display_filters,
        layout: display.layout,
        group_by: display.groupBy,
        sub_group_by: display.subGroupBy,
        order_by: display.orderBy,
      },
      displayProperties: record.display_properties,
    };
  }, [entityPrefs.data, props.viewDisplay, rows, viewRecord]);

  const serverFingerprint = React.useMemo(
    () => JSON.stringify(conditions.map((row) => [row.key, row.property, row.operator, row.negated, row.value])),
    [conditions]
  );
  // Adopt server changes (a completed PATCH, a background refetch), but
  // never drop a draft that still holds the user's pending edits: a second
  // half-built condition must survive the echo of the first one's PATCH.
  const persistedExpression = React.useMemo(() => JSON.stringify(serializeRichExpression(conditions)), [conditions]);
  React.useEffect(() => {
    setDraft((current) =>
      current !== null &&
      current.every(isConditionComplete) &&
      JSON.stringify(serializeRichExpression(current)) === persistedExpression
        ? null
        : current
    );
  }, [serverFingerprint, persistedExpression]);
  // A failed write rolled the cache back; drop the draft so the row shows
  // the server's stored expression instead of an edit that never landed.
  const failedWrites = React.useRef(failureCount);
  React.useEffect(() => {
    if (failureCount !== failedWrites.current) {
      failedWrites.current = failureCount;
      setDraft(null);
    }
  }, [failureCount]);
  // Temporary instances report their field configs while mounted.
  useTemporaryFieldConfigs(temporary ? rows : [], props.temporaryRegistry);
  const used = React.useMemo(() => usedProperties(rows), [rows]);
  const hasConditions = hasRichConditions(rows);
  // A saved view whose row no longer matches the record still needs its
  // right side: Clear all empties the expression, and the only way that
  // empty expression reaches the record is through this row's Update view.
  // So the row (and its actions) outlives an empty condition list whenever
  // the view carries unsaved changes.
  const viewDirty = viewRecord !== null && isViewDirty(viewContext, snapshot);
  const showRowActions = hasConditions || viewDirty;

  function commit(next: readonly RichCondition[]): void {
    setDraft(next);
    if (next.length === 0) setAddedThisVisit(false);
    else if (next.some((row) => !conditions.some((current) => current.key === row.key))) setAddedThisVisit(true);
    if (temporary || isView) {
      if (temporary) setLocal(next);
      else setViewConditions(next);
      return;
    }
    // A condition stays a draft until it is complete: adding a field picks
    // its property and operator first and the value second, so persisting
    // here would write an unchanged expression and the server echo would
    // drop the half-built row. Only an actual expression change reaches
    // the preference record.
    const current = JSON.stringify(serializeRichExpression(conditions));
    const nextExpression = JSON.stringify(serializeRichExpression(next));
    if (nextExpression !== current) updateRichFilters(next);
  }

  const control = showRowActions ? (
    <Tooltip content={open ? "Hide filters" : "Show filters"}>
      <IconButton
        label="Filters"
        variant="secondary"
        size="small"
        aria-pressed={open}
        data-testid="rich-filter-toggle"
        data-applied="true"
        onClick={() => {
          setOpen(!open);
        }}
        className={open ? "text-(--accent)" : "text-(--accent)"}
      >
        <Filter size={14} strokeWidth={2} aria-hidden="true" />
      </IconButton>
    </Tooltip>
  ) : (
    <FieldPicker
      fields={fields}
      used={used}
      trigger={
        <Button variant="secondary" size="small" data-testid="rich-filter-add">
          <Filter size={14} strokeWidth={2} aria-hidden="true" />
          <span>Add filter</span>
        </Button>
      }
      onPick={(field) => {
        commit(addCondition(rows, field));
        setOpen(true);
      }}
    />
  );

  const row =
    showRowActions && open ? (
      <div
        data-testid="rich-filter-row"
        className="flex flex-wrap items-center gap-(--space-2) border-b border-(--border) px-(--space-4) py-(--space-2)"
      >
        {rows.map((condition) => (
          <ConditionPill
            key={condition.key}
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            fields={fields}
            field={conditionField(condition, fields)}
            condition={condition}
            conditions={rows}
            used={used}
            onChange={(next) => {
              commit(updateCondition(rows, condition.key, next));
            }}
            onRemove={() => {
              commit(removeCondition(rows, condition.key));
            }}
          />
        ))}
        <FieldPicker
          fields={fields}
          used={used}
          trigger={
            <IconButton label="Add filter" size="small">
              <Plus size={14} strokeWidth={2} aria-hidden="true" />
            </IconButton>
          }
          onPick={(field) => {
            commit(addCondition(rows, field));
          }}
        />
        <ViewActions
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          snapshot={snapshot}
          context={viewContext}
          canEdit={canWriteViews && !temporary}
          viewsEnabled={props.viewsEnabled ?? true}
          meId={me.data?.id ?? null}
          hasConditions={hasConditions}
          onClearAll={() => {
            commit([]);
          }}
          onCreated={props.onViewCreated}
          onSavePress={props.onSavePress}
          onUpdated={(stored) => {
            setViewRecord(stored);
            setViewConditions(parseRichExpression(stored.rich_filters));
            setDraft(null);
            props.onViewUpdated?.(stored);
          }}
        />
      </div>
    ) : null;

  return {
    control,
    row,
    instanceId,
    conditions: rows,
    addedThisVisit,
    clearFilters: () => {
      commit([]);
    },
    hasCompleteCondition: rows.some(isConditionComplete),
  };
}

/** Stable entity id segment for a persisted instance. */
function entityIdOf(entity: IssuesEntity): string {
  switch (entity.scope) {
    case "project":
      return entity.projectId;
    case "cycle":
      return `${entity.projectId}/${entity.cycleId}`;
    case "module":
      return `${entity.projectId}/${entity.moduleId}`;
    case "epics":
      return `epics/${entity.projectId}`;
    case "workspace":
      return "workspace";
  }
}

export type { RichFilterField };

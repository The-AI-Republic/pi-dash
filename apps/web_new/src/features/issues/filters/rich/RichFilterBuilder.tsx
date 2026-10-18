// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rich-filter builder (parity/specs/issues.md §9, ISS-095–099). One
// component owns the stored expression, the header entry and the
// condition row it toggles. Empty expressions render an "Add filter"
// button that opens the field picker; non-empty ones render an accent
// filter icon toggling the row. Every edit persists through the entity's
// user-properties record. ISS-095 resolves to per-condition removal: no
// chip bar renders on issues surfaces (NEWFRONT-148).

import { Button, IconButton, Tooltip } from "@pidash/kit";
import { Filter, Plus } from "lucide-react";
import * as React from "react";

import type { IssuesEntity } from "../../prefs/entities.js";
import {
  addCondition,
  hasRichConditions,
  isConditionComplete,
  removeCondition,
  richFilterFields,
  serializeRichExpression,
  updateCondition,
  usedProperties,
  type RichCondition,
} from "./expression.js";
import { ConditionPill, conditionField } from "./ConditionPill.js";
import { FieldPicker } from "./FieldPicker.js";
import { useRichFilters, useUpdateRichFilters } from "./useRichFilters.js";

export interface RichFilterBuilderProps {
  workspaceSlug: string;
  projectId: string;
  entity: IssuesEntity;
  /** Cycle/module properties hide while their project features are off. */
  hideCycle: boolean;
  hideModule: boolean;
}

export interface RichFilterBuilderParts {
  /** Header trigger, mounted in the header action slot. */
  control: React.ReactNode;
  /** Builder row, mounted beneath the header. */
  row: React.ReactNode;
}

/**
 * Build the header control and row from one shared state instance so the
 * toggle and the row cannot diverge. The caller places `control` in the
 * header and `row` below it.
 */
export function useRichFilterBuilder(props: RichFilterBuilderProps): RichFilterBuilderParts {
  const { workspaceSlug, projectId, entity, hideCycle, hideModule } = props;
  const { conditions } = useRichFilters(workspaceSlug, entity);
  const { updateRichFilters, failureCount } = useUpdateRichFilters(workspaceSlug, entity);
  const [open, setOpen] = React.useState(true);
  const fields = React.useMemo(() => richFilterFields({ hideCycle, hideModule }), [hideCycle, hideModule]);
  const [draft, setDraft] = React.useState<readonly RichCondition[] | null>(null);
  const rows = draft ?? conditions;
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
  const used = React.useMemo(() => usedProperties(rows), [rows]);
  const hasConditions = hasRichConditions(rows);

  function commit(next: readonly RichCondition[]): void {
    setDraft(next);
    // A condition stays a draft until it is complete: adding a field picks
    // its property and operator first and the value second, so persisting
    // here would write an unchanged expression and the server echo would
    // drop the half-built row. Only an actual expression change reaches
    // the preference record.
    const current = JSON.stringify(serializeRichExpression(conditions));
    const nextExpression = JSON.stringify(serializeRichExpression(next));
    if (nextExpression !== current) updateRichFilters(next);
  }

  const control = hasConditions ? (
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
    hasConditions && open ? (
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
      </div>
    ) : null;

  return { control, row };
}

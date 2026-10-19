// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Display options panel (parity/specs/issues.md §9, ISS-075–081, ISS-094).
// The five sections in fixed order with only the ones the active layout
// enables. Every section reuses the shared filter-section chrome: title,
// applied count, collapsible body, search box and a five-row cap. Grouping
// and ordering are single-choice sections and mark their rows as radios;
// the property pills and switches stay buttons with aria-pressed so the
// parity gate can probe them by visible label.

import { Check } from "lucide-react";
import * as React from "react";

import type { LayoutKind } from "../layouts/kinds.js";
import { FilterSection, type FilterSectionOption } from "../filters/shared/FilterSection.js";
import {
  DISPLAY_SECTION_TITLES,
  DISPLAY_SWITCH_LABELS,
  groupDimensionLabel,
  isPropertyShown,
  layoutDisplayMatrix,
  ORDER_OPTION_LABELS,
  propertyPillLabel,
  visibleGroupDimensions,
  visiblePropertyKeys,
  type DisplaySection,
  type DisplaySwitchKey,
} from "./matrix.js";

/** The persisted display state the panel edits. */
export interface DisplayPanelState {
  groupBy: string | null;
  subGroupBy: string | null;
  orderBy: string;
  showEmptyGroups: boolean;
  showSubIssues: boolean;
  properties: Record<string, boolean>;
}

export interface DisplayPanelProps {
  layout: LayoutKind;
  state: DisplayPanelState;
  /** Cycle/module pills and dimensions hide while their features are off. */
  hideCycle: boolean;
  hideModule: boolean;
  isEpic: boolean;
  onGroupBy: (dimension: string | null) => void;
  onSubGroupBy: (dimension: string | null) => void;
  onOrderBy: (order: string) => void;
  /** Switch toggle with the next state; the panel owns the absent-key read. */
  onToggleSwitch: (key: DisplaySwitchKey, on: boolean) => void;
  /** Pill toggle with the next visibility; the panel owns the absent-key read. */
  onToggleProperty: (key: string, shown: boolean) => void;
}

/** The stored grouping id for a dimension row; `null` is the ungrouped row. */
const NONE_ROW = "\u0000none";

function SwitchOption({
  switchKey,
  on,
  onToggle,
}: {
  switchKey: DisplaySwitchKey;
  on: boolean;
  onToggle: (key: DisplaySwitchKey, on: boolean) => void;
}): React.ReactElement {
  return (
    <button
      type="button"
      aria-pressed={on}
      onClick={() => onToggle(switchKey, !on)}
      className="flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) hover:bg-(--subtle)"
    >
      <span
        aria-hidden="true"
        className={`grid size-3.5 shrink-0 place-items-center rounded-(--radius-control) border ${
          on ? "border-(--accent) bg-(--accent) text-(--on-accent)" : "border-(--border)"
        }`}
      >
        {on ? <Check size={10} strokeWidth={3} /> : null}
      </span>
      <span className="text-body truncate text-(--text)">{DISPLAY_SWITCH_LABELS[switchKey]}</span>
    </button>
  );
}

function switchState(state: DisplayPanelState, key: DisplaySwitchKey): boolean {
  return key === "sub_issue" ? state.showSubIssues : state.showEmptyGroups;
}

/**
 * A single-choice section's rows; the panel syncs the stored dimension and
 * the shared section's radio state reads from that one applied value.
 */
function SingleChoiceSection({
  section,
  labels,
  checked,
  count,
  onPick,
}: {
  section: DisplaySection;
  labels: readonly { id: string; label: string }[];
  checked: readonly string[];
  count: number;
  onPick: (id: string) => void;
}): React.ReactElement {
  const options: FilterSectionOption[] = labels.map((row) => ({ id: row.id, label: row.label }));
  return (
    <FilterSection
      title={DISPLAY_SECTION_TITLES[section]}
      kind="single"
      selectionRole="pressed"
      applied={[]}
      checked={checked}
      count={count}
      options={options}
      onToggle={onPick}
      groupLabel={`${DISPLAY_SECTION_TITLES[section]} options`}
      testId={`display-section-${section}`}
      className="py-(--space-4)"
      cap={Number.POSITIVE_INFINITY}
    />
  );
}

function dimensionRows(dimensions: readonly (string | null)[]): { id: string; label: string }[] {
  return dimensions.map((dimension) => ({
    id: dimension ?? NONE_ROW,
    label: groupDimensionLabel(dimension),
  }));
}

export function DisplayPanel({
  layout,
  state,
  hideCycle,
  hideModule,
  isEpic,
  onGroupBy,
  onSubGroupBy,
  onOrderBy,
  onToggleSwitch,
  onToggleProperty,
}: DisplayPanelProps): React.ReactElement {
  const matrix = layoutDisplayMatrix(layout);
  const flags = { hideCycle, hideModule };
  const showSubGroup = matrix.subGroupBy !== null && layout === "kanban" && state.groupBy !== null;
  return (
    <div data-testid="display-options-panel" className="divide-y divide-(--border) px-(--space-2)">
      {matrix.properties.length > 0 ? (
        <FilterSection
          key={`properties:${layout}`}
          title={DISPLAY_SECTION_TITLES.properties}
          kind="multi"
          applied={[]}
          options={[]}
          onToggle={() => {}}
          testId="display-section-properties"
          className="py-(--space-4)"
        >
          <div className="mt-(--space-1) flex flex-wrap items-center gap-(--space-2)">
            {visiblePropertyKeys(matrix.properties, flags).map((key) => {
              const shown = isPropertyShown(state.properties, key);
              return (
                <button
                  key={key}
                  type="button"
                  aria-pressed={shown}
                  onClick={() => onToggleProperty(key, !shown)}
                  className={`text-caption rounded-full border px-(--space-4) py-(--space-1) ${
                    shown
                      ? "border-(--accent) bg-(--accent) text-(--on-accent)"
                      : "border-(--border) text-(--text-muted)"
                  }`}
                >
                  {propertyPillLabel(key, isEpic)}
                </button>
              );
            })}
          </div>
        </FilterSection>
      ) : null}
      {matrix.groupBy !== null ? (
        <SingleChoiceSection
          key={`group:${layout}`}
          section="group"
          labels={dimensionRows(
            visibleGroupDimensions(matrix.groupBy, layout === "kanban" ? state.subGroupBy : null, flags)
          )}
          checked={[state.groupBy ?? NONE_ROW]}
          count={state.groupBy === null ? 0 : 1}
          onPick={(id) => onGroupBy(id === NONE_ROW ? null : id)}
        />
      ) : null}
      {showSubGroup && matrix.subGroupBy !== null ? (
        <SingleChoiceSection
          key={`subGroup:${layout}`}
          section="subGroup"
          labels={dimensionRows(visibleGroupDimensions(matrix.subGroupBy, state.groupBy, flags))}
          checked={[state.subGroupBy ?? NONE_ROW]}
          count={state.subGroupBy === null ? 0 : 1}
          onPick={(id) => onSubGroupBy(id === NONE_ROW ? null : id)}
        />
      ) : null}
      {matrix.orderBy !== null && matrix.orderBy.length > 0 ? (
        <SingleChoiceSection
          key={`order:${layout}`}
          section="order"
          labels={matrix.orderBy.map((order) => ({ id: order, label: ORDER_OPTION_LABELS[order] ?? order }))}
          // The order always holds a value, so it is not an applied filter;
          // the active row marks itself, the title carries no count.
          checked={[state.orderBy]}
          count={0}
          onPick={onOrderBy}
        />
      ) : null}
      {matrix.switches.length > 0 ? (
        <FilterSection
          key={`options:${layout}`}
          title={DISPLAY_SECTION_TITLES.options}
          kind="multi"
          applied={[]}
          options={[]}
          onToggle={() => {}}
          testId="display-section-options"
          className="py-(--space-4)"
        >
          <div className="mt-(--space-1)">
            {matrix.switches.map((switchKey) => (
              <SwitchOption
                key={switchKey}
                switchKey={switchKey}
                on={switchState(state, switchKey)}
                onToggle={onToggleSwitch}
              />
            ))}
          </div>
        </FilterSection>
      ) : null}
    </div>
  );
}

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Display options panel (parity/specs/issues.md §9, ISS-075–081).
// Presentational: the five sections in fixed order with only the ones
// the active layout enables, each collapsible. Single-select options
// carry a check marker, property pills paint active, and the switches
// read as toggle buttons; every control exposes its state through
// aria-pressed so the parity gate can probe it by visible label.

import { Check, ChevronDown, ChevronUp } from "lucide-react";
import * as React from "react";

import type { LayoutKind } from "../layouts/kinds.js";
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
  onToggleSwitch: (key: DisplaySwitchKey) => void;
  /** Pill toggle with the next visibility; the panel owns the absent-key read. */
  onToggleProperty: (key: string, shown: boolean) => void;
}

function DisplaySectionShell({
  section,
  children,
}: {
  section: DisplaySection;
  children: React.ReactNode;
}): React.ReactElement {
  const [open, setOpen] = React.useState(true);
  const title = DISPLAY_SECTION_TITLES[section];
  return (
    <div data-testid={`display-section-${section}`} className="py-(--space-4)">
      <div className="flex items-center justify-between gap-(--space-2)">
        <h3 className="text-caption text-(--text-muted)">{title}</h3>
        <button
          type="button"
          aria-label={`${open ? "Collapse" : "Expand"} ${title} section`}
          aria-expanded={open}
          onClick={() => setOpen(!open)}
          className="grid size-5 shrink-0 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle)"
        >
          {open ? <ChevronUp size={14} aria-hidden="true" /> : <ChevronDown size={14} aria-hidden="true" />}
        </button>
      </div>
      {open ? <div className="mt-(--space-1)">{children}</div> : null}
    </div>
  );
}

function CheckOption({
  label,
  checked,
  onSelect,
}: {
  label: string;
  checked: boolean;
  onSelect: () => void;
}): React.ReactElement {
  return (
    <button
      type="button"
      aria-pressed={checked}
      onClick={onSelect}
      className="flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) hover:bg-(--subtle)"
    >
      <span
        aria-hidden="true"
        className={`grid size-3.5 shrink-0 place-items-center rounded-full border ${
          checked ? "border-(--accent) bg-(--accent) text-(--on-accent)" : "border-(--border)"
        }`}
      >
        {checked ? <Check size={10} strokeWidth={3} /> : null}
      </span>
      <span className="text-body truncate text-(--text)">{label}</span>
    </button>
  );
}

function SwitchOption({
  switchKey,
  on,
  onToggle,
}: {
  switchKey: DisplaySwitchKey;
  on: boolean;
  onToggle: (key: DisplaySwitchKey) => void;
}): React.ReactElement {
  return (
    <button
      type="button"
      aria-pressed={on}
      onClick={() => onToggle(switchKey)}
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
        <DisplaySectionShell key={`properties:${layout}`} section="properties">
          <div className="flex flex-wrap items-center gap-(--space-2)">
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
        </DisplaySectionShell>
      ) : null}
      {matrix.groupBy !== null ? (
        <DisplaySectionShell key={`group:${layout}`} section="group">
          <div>
            {visibleGroupDimensions(matrix.groupBy, layout === "kanban" ? state.subGroupBy : null, flags).map(
              (dimension) => (
                <CheckOption
                  key={dimension ?? "none"}
                  label={groupDimensionLabel(dimension)}
                  checked={state.groupBy === dimension}
                  onSelect={() => onGroupBy(dimension)}
                />
              )
            )}
          </div>
        </DisplaySectionShell>
      ) : null}
      {showSubGroup && matrix.subGroupBy !== null ? (
        <DisplaySectionShell key={`subGroup:${layout}`} section="subGroup">
          <div>
            {visibleGroupDimensions(matrix.subGroupBy, state.groupBy, flags).map((dimension) => (
              <CheckOption
                key={dimension ?? "none"}
                label={groupDimensionLabel(dimension)}
                checked={state.subGroupBy === dimension}
                onSelect={() => onSubGroupBy(dimension)}
              />
            ))}
          </div>
        </DisplaySectionShell>
      ) : null}
      {matrix.orderBy !== null && matrix.orderBy.length > 0 ? (
        <DisplaySectionShell key={`order:${layout}`} section="order">
          <div>
            {matrix.orderBy.map((order) => (
              <CheckOption
                key={order}
                label={ORDER_OPTION_LABELS[order] ?? order}
                checked={state.orderBy === order}
                onSelect={() => onOrderBy(order)}
              />
            ))}
          </div>
        </DisplaySectionShell>
      ) : null}
      {matrix.switches.length > 0 ? (
        <DisplaySectionShell key={`options:${layout}`} section="options">
          <div>
            {matrix.switches.map((switchKey) => (
              <SwitchOption
                key={switchKey}
                switchKey={switchKey}
                on={switchState(state, switchKey)}
                onToggle={onToggleSwitch}
              />
            ))}
          </div>
        </DisplaySectionShell>
      ) : null}
    </div>
  );
}

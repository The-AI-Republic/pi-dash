// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sub-issue display dropdown (parity/specs/issues.md §15, ISS-178, on the
// shared frame of §22 ISS-094). Three sections on the same chrome as the
// filter menu: grouping (radio, with an ungrouped row), ordering (radio)
// and the six property columns (pressed pills). Choices stay per parent;
// the trigger marks how many settings differ from the defaults.

import { Button, Popover } from "@pidash/kit";
import { SlidersHorizontal } from "lucide-react";
import * as React from "react";

import { FilterSection, FilterSectionFrame, type FilterSectionOption } from "../filters/shared/FilterSection.js";
import {
  displayChangeCount,
  SUB_ISSUE_GROUP_OPTIONS,
  SUB_ISSUE_ORDER_OPTIONS,
  SUB_ISSUE_PROPERTY_KEYS,
  SUB_ISSUE_PROPERTY_LABELS,
  type SubIssueDisplay,
  type SubIssueGroupBy,
  type SubIssueOrderBy,
  type SubIssuePropertyKey,
} from "./rules.js";

export interface SubIssueDisplayMenuProps {
  display: SubIssueDisplay;
  onGroupBy: (groupBy: SubIssueGroupBy) => void;
  onOrderBy: (orderBy: SubIssueOrderBy) => void;
  onToggleProperty: (key: SubIssuePropertyKey) => void;
}

const NONE_ROW = "\u0000none";

export function SubIssueDisplayMenu({
  display,
  onGroupBy,
  onOrderBy,
  onToggleProperty,
}: SubIssueDisplayMenuProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const count = displayChangeCount(display);
  const groupOptions: FilterSectionOption[] = SUB_ISSUE_GROUP_OPTIONS.map((option) => ({
    id: option.value ?? NONE_ROW,
    label: option.label,
  }));
  const orderOptions: FilterSectionOption[] = SUB_ISSUE_ORDER_OPTIONS.map((option) => ({
    id: option.id,
    label: option.label,
  }));
  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      className="max-h-[26rem] w-72 overflow-y-auto"
      trigger={
        <Button
          variant="ghost"
          size="small"
          data-testid="sub-issue-display-trigger"
          aria-label={count > 0 ? `Display, ${count} changed` : "Display"}
          aria-pressed={count > 0}
        >
          <SlidersHorizontal size={14} strokeWidth={2} aria-hidden="true" />
        </Button>
      }
    >
      <div className="divide-y divide-(--border)" data-testid="sub-issue-display-panel">
        <FilterSection
          title="Group by"
          kind="single"
          applied={[]}
          checked={[display.groupBy ?? NONE_ROW]}
          count={display.groupBy === null ? 0 : 1}
          options={groupOptions}
          onToggle={(id) => onGroupBy(id === NONE_ROW ? null : (id as Exclude<SubIssueGroupBy, null>))}
          groupLabel="Group by"
          testId="sub-issue-display-group"
          className="py-(--space-2)"
          cap={Number.POSITIVE_INFINITY}
        />
        <FilterSection
          title="Order by"
          kind="single"
          applied={[]}
          checked={[display.orderBy]}
          count={0}
          options={orderOptions}
          onToggle={(id) => onOrderBy(id as SubIssueOrderBy)}
          groupLabel="Order by"
          testId="sub-issue-display-order"
          className="py-(--space-2)"
          cap={Number.POSITIVE_INFINITY}
        />
        <FilterSectionFrame
          title="Display Properties"
          countLabel="Display Properties"
          testId="sub-issue-display-properties"
          className="py-(--space-2)"
        >
          <div className="mt-(--space-1) flex flex-wrap items-center gap-(--space-2)">
            {SUB_ISSUE_PROPERTY_KEYS.map((key) => {
              const shown = display.properties[key] !== false;
              return (
                <button
                  key={key}
                  type="button"
                  aria-pressed={shown}
                  onClick={() => onToggleProperty(key)}
                  className={`text-caption rounded-full border px-(--space-4) py-(--space-1) ${
                    shown
                      ? "border-(--accent) bg-(--accent) text-(--on-accent)"
                      : "border-(--border) text-(--text-muted)"
                  }`}
                >
                  {SUB_ISSUE_PROPERTY_LABELS[key]}
                </button>
              );
            })}
          </div>
        </FilterSectionFrame>
      </div>
    </Popover>
  );
}

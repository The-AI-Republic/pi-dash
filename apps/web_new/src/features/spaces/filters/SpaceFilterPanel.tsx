// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Filter panel (SPC-034/038/039): a navbar Filters control opening a
// dropdown with one collapsible section per facet the active layout
// supports. The search box narrows the option rows only — it never
// touches the address or the board. Picks stay in the panel (it does
// not close) and flow to the address through onToggle. Labels are never
// offered here; they arrive via the address only (SPC-037).

import { Button, Popover } from "@pidash/kit";
import { Check, ChevronDown, ListFilter } from "lucide-react";
import * as React from "react";

import type { SpaceBoardState } from "@pidash/api-client";

import { StateMarker } from "../layouts/StateMarker.js";
import { SPACE_PRIORITIES, type SpaceBoardFilters, type SpaceLayout } from "../lib/address.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import {
  facetValues,
  optionMatchesQuery,
  panelFacetsForLayout,
  previewStateOptions,
  SPACE_FACET_MESSAGE_KEYS,
  SPACE_PRIORITY_MESSAGE_KEYS,
  SPACE_STATE_OPTION_PREVIEW,
  type SpacePanelFacet,
} from "./facets.js";
import { PriorityMarker } from "./PriorityMarker.js";

export interface SpaceFilterPanelProps {
  layout: SpaceLayout;
  filters: SpaceBoardFilters;
  states: readonly SpaceBoardState[];
  onToggle: (facet: SpacePanelFacet, value: string) => void;
}

interface PanelOption {
  value: string;
  label: string;
  checked: boolean;
  marker: React.ReactNode;
}

function priorityPanelOptions(filters: SpaceBoardFilters, query: string): PanelOption[] {
  const applied = new Set(facetValues(filters, "priority"));
  return SPACE_PRIORITIES.filter((level) =>
    optionMatchesQuery([spaceMessage(SPACE_PRIORITY_MESSAGE_KEYS[level]), level], query)
  ).map((level) => ({
    value: level,
    label: spaceMessage(SPACE_PRIORITY_MESSAGE_KEYS[level]),
    checked: applied.has(level),
    marker: <PriorityMarker level={level} />,
  }));
}

function statePanelOptions(
  filters: SpaceBoardFilters,
  states: readonly SpaceBoardState[],
  query: string
): PanelOption[] {
  const applied = new Set(facetValues(filters, "state"));
  return states
    .filter((state) => optionMatchesQuery([state.name], query))
    .map((state) => ({
      value: state.id,
      label: state.name,
      checked: applied.has(state.id),
      marker: <StateMarker state={state} />,
    }));
}

function FilterOptionRow({
  facet,
  option,
  onToggle,
}: {
  facet: SpacePanelFacet;
  option: PanelOption;
  onToggle: (facet: SpacePanelFacet, value: string) => void;
}): React.ReactElement {
  return (
    <li>
      <button
        type="button"
        data-testid="space-filter-option"
        data-facet={facet}
        data-value={option.value}
        data-checked={option.checked ? "true" : "false"}
        aria-pressed={option.checked}
        onClick={() => onToggle(facet, option.value)}
        className="flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left hover:bg-(--subtle)"
      >
        <span
          aria-hidden="true"
          data-testid="space-filter-check"
          data-checked={option.checked ? "true" : "false"}
          className={
            option.checked
              ? "inline-flex size-4 shrink-0 items-center justify-center rounded-(--radius-control) border border-(--accent) bg-(--accent) text-(--on-accent)"
              : "inline-flex size-4 shrink-0 items-center justify-center rounded-(--radius-control) border border-(--border)"
          }
        >
          {option.checked ? <Check size={12} aria-hidden="true" /> : null}
        </span>
        {option.marker}
        <span title={option.label} className="text-body truncate text-(--text)">
          {option.label}
        </span>
      </button>
    </li>
  );
}

export function SpaceFilterPanel({ layout, filters, states, onToggle }: SpaceFilterPanelProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const [query, setQuery] = React.useState("");
  const [collapsed, setCollapsed] = React.useState<ReadonlySet<SpacePanelFacet>>(() => new Set());
  const [statesExpanded, setStatesExpanded] = React.useState(false);
  const facets = panelFacetsForLayout(layout);

  const handleOpenChange = (next: boolean): void => {
    setOpen(next);
    if (!next) {
      // Each opening starts fresh: no stale search, no stale expansion.
      setQuery("");
      setCollapsed(new Set());
      setStatesExpanded(false);
    }
  };

  const toggleSection = (facet: SpacePanelFacet): void => {
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(facet)) {
        next.delete(facet);
      } else {
        next.add(facet);
      }
      return next;
    });
  };

  return (
    <Popover
      trigger={
        <Button variant="secondary" size="small" data-testid="space-filters-button" aria-expanded={open}>
          <ListFilter size={14} aria-hidden="true" />
          {spaceMessage("filters.open")}
        </Button>
      }
      open={open}
      onOpenChange={handleOpenChange}
    >
      <div data-testid="space-filters-panel" className="flex flex-col gap-(--space-4)">
        <input
          data-testid="space-filter-search"
          type="text"
          aria-label={spaceMessage("filters.search")}
          placeholder={spaceMessage("filters.search")}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          className="text-body h-(--control-height) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) text-(--text)"
        />
        {facets.map((facet) => {
          const isCollapsed = collapsed.has(facet);
          const applied = facetValues(filters, facet).length;
          const matched =
            facet === "priority" ? priorityPanelOptions(filters, query) : statePanelOptions(filters, states, query);
          const shown = facet === "state" ? previewStateOptions(matched, statesExpanded) : matched;
          const canExpand = facet === "state" && matched.length > SPACE_STATE_OPTION_PREVIEW;
          return (
            <section
              key={facet}
              data-testid="space-filter-facet"
              data-facet={facet}
              className="flex flex-col gap-(--space-2)"
            >
              <button
                type="button"
                data-testid="space-filter-section"
                data-facet={facet}
                aria-expanded={!isCollapsed}
                onClick={() => toggleSection(facet)}
                className="flex w-full items-center justify-between rounded-(--radius-control) px-(--space-2) py-(--space-1) hover:bg-(--subtle)"
              >
                <span className="text-emphasis text-(--text)">
                  {spaceMessage(SPACE_FACET_MESSAGE_KEYS[facet])}
                  {applied === 0 ? null : ` (${applied})`}
                </span>
                <ChevronDown
                  size={14}
                  aria-hidden="true"
                  className={isCollapsed ? "-rotate-90 text-(--text-muted)" : "text-(--text-muted)"}
                />
              </button>
              {isCollapsed ? null : shown.length === 0 ? (
                <p
                  data-testid="space-filter-no-matches"
                  data-facet={facet}
                  className="text-body px-(--space-2) text-(--text-muted)"
                >
                  {spaceMessage("filters.noMatches")}
                </p>
              ) : (
                <>
                  <ul className="flex flex-col">
                    {shown.map((option) => (
                      <FilterOptionRow key={option.value} facet={facet} option={option} onToggle={onToggle} />
                    ))}
                  </ul>
                  {canExpand ? (
                    <button
                      type="button"
                      data-testid="space-filter-show-more"
                      data-expanded={statesExpanded ? "true" : "false"}
                      onClick={() => setStatesExpanded((expanded) => !expanded)}
                      className="text-body rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text-muted) hover:bg-(--subtle)"
                    >
                      {spaceMessage(statesExpanded ? "filters.showLess" : "filters.showMore")}
                    </button>
                  ) : null}
                </>
              )}
            </section>
          );
        })}
      </div>
    </Popover>
  );
}

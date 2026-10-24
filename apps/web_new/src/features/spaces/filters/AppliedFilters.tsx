// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Applied-filters strip (SPC-035/037): one shell per applied facet above
// the layouts — value pills with per-pill removal, a per-facet clear and
// one clear-all. Labels render as a shell only (no pills exist for them);
// state ids the states fetch does not know render no pill but keep their
// shell's clear. The strip renders nothing when no facet applies.

import { Button } from "@pidash/kit";
import { X } from "lucide-react";
import * as React from "react";

import type { SpaceBoardState } from "@pidash/api-client";

import { StateMarker } from "../layouts/StateMarker.js";
import type { SpaceBoardFilters } from "../lib/address.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import {
  facetValues,
  hasAppliedFilters,
  SPACE_FACET_MESSAGE_KEYS,
  SPACE_PRIORITY_MESSAGE_KEYS,
  SPACE_STRIP_FACET_ORDER,
  type SpacePriorityLevel,
  type SpaceStripFacet,
} from "./facets.js";
import { PriorityMarker } from "./PriorityMarker.js";

export interface AppliedFiltersProps {
  filters: SpaceBoardFilters;
  states: readonly SpaceBoardState[];
  onRemoveValue: (facet: SpaceStripFacet, value: string) => void;
  onClearFacet: (facet: SpaceStripFacet) => void;
  onClearAll: () => void;
}

function AppliedPill({
  facet,
  pillKey,
  label,
  marker,
  stateId,
  onRemove,
}: {
  facet: SpaceStripFacet;
  pillKey: string;
  label: string;
  marker: React.ReactNode;
  stateId?: string | undefined;
  onRemove: () => void;
}): React.ReactElement {
  return (
    <span
      data-testid="space-applied-pill"
      data-facet={facet}
      data-value={pillKey}
      {...(stateId === undefined ? {} : { "data-state-id": stateId })}
      className="inline-flex items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--bg) py-(--space-1) pr-(--space-1) pl-(--space-2)"
    >
      {marker}
      <span className="text-body text-(--text)">{label}</span>
      <button
        type="button"
        data-testid="space-applied-remove"
        data-facet={facet}
        data-value={pillKey}
        aria-label={spaceMessage("filters.removeValue")}
        onClick={onRemove}
        className="inline-flex items-center justify-center rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
      >
        <X size={12} aria-hidden="true" />
      </button>
    </span>
  );
}

function priorityPills(
  filters: SpaceBoardFilters,
  onRemoveValue: (facet: SpaceStripFacet, value: string) => void
): React.ReactNode {
  return facetValues(filters, "priority").map((level) => {
    const key = level as SpacePriorityLevel;
    return (
      <AppliedPill
        key={level}
        facet="priority"
        pillKey={level}
        label={spaceMessage(SPACE_PRIORITY_MESSAGE_KEYS[key])}
        marker={<PriorityMarker level={key} />}
        onRemove={() => onRemoveValue("priority", level)}
      />
    );
  });
}

function statePills(
  filters: SpaceBoardFilters,
  states: readonly SpaceBoardState[],
  onRemoveValue: (facet: SpaceStripFacet, value: string) => void
): React.ReactNode {
  return facetValues(filters, "state").map((id) => {
    const state = states.find((entry) => entry.id === id);
    if (state === undefined) return null;
    return (
      <AppliedPill
        key={id}
        facet="state"
        pillKey={state.name}
        label={state.name}
        marker={<StateMarker state={state} />}
        stateId={id}
        onRemove={() => onRemoveValue("state", id)}
      />
    );
  });
}

export function AppliedFilters({
  filters,
  states,
  onRemoveValue,
  onClearFacet,
  onClearAll,
}: AppliedFiltersProps): React.ReactElement | null {
  if (!hasAppliedFilters(filters)) return null;
  const shells = SPACE_STRIP_FACET_ORDER.filter((facet) => facetValues(filters, facet).length > 0);
  return (
    <div
      data-testid="space-applied-filters"
      className="flex flex-wrap items-center gap-(--space-2) px-(--space-4) pt-(--space-3)"
    >
      {shells.map((facet) => (
        <div
          key={facet}
          data-testid="space-applied-facet"
          data-facet={facet}
          className="flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--subtle) py-(--space-1) pr-(--space-1) pl-(--space-3)"
        >
          <span className="text-meta text-(--text-muted)">{spaceMessage(SPACE_FACET_MESSAGE_KEYS[facet])}</span>
          {facet === "priority"
            ? priorityPills(filters, onRemoveValue)
            : facet === "state"
              ? statePills(filters, states, onRemoveValue)
              : null}
          <button
            type="button"
            data-testid="space-applied-clear"
            data-facet={facet}
            aria-label={spaceMessage("filters.clearFacet")}
            onClick={() => onClearFacet(facet)}
            className="inline-flex items-center justify-center rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--bg) hover:text-(--text)"
          >
            <X size={12} aria-hidden="true" />
          </button>
        </div>
      ))}
      <Button variant="ghost" size="small" data-testid="space-applied-clear-all" onClick={onClearAll}>
        {spaceMessage("filters.clearAll")}
      </Button>
    </div>
  );
}

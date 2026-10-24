// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One published board (SPC-001, SPC-003, SPC-004, SPC-016, SPC-020–026,
// SPC-034–039). Settings are the gate: while they load the navbar
// placeholder and an empty body show; a 404 swaps to the anchor
// illustration; any other failure swaps to the board-error illustration.
// Once settings resolve, issues and states load under the real navbar,
// whose switcher offers only the publisher-enabled layouts and whose
// Filters control offers the active layout's facets; the settled layout
// and the filters live in the address, and the applied pills render above
// the layouts. Every branch mounts the board's document head (SPC-007) so
// the tags settle before content is announced.

import { useQuery } from "@tanstack/react-query";
import type { SpaceBoardIssuePage, SpaceBoardSettings } from "@pidash/api-client";
import * as React from "react";

import { spaceRuntime } from "../api/client.js";
import { spaceIssuesQueryOptions, spaceSettingsQueryOptions, spaceStatesQueryOptions } from "../api/queries.js";
import { AppliedFilters } from "../filters/AppliedFilters.js";
import { SpaceFilterPanel } from "../filters/SpaceFilterPanel.js";
import { useSpaceFilterNav } from "../filters/useSpaceFilterNav.js";
import { BoardLayouts } from "../layouts/BoardLayouts.js";
import { LayoutSwitcher } from "../layouts/LayoutSwitcher.js";
import { useSettledSpaceLayout } from "../layouts/useSettledSpaceLayout.js";
import { classifySpaceFailure } from "../lib/errors.js";
import {
  isValidAnchor,
  parseSpaceBoardSearch,
  type SpaceBoardFilters,
  type SpaceBoardSearch,
  type SpaceLayout,
} from "../lib/address.js";
import { BoardLoading, BoardNavbar } from "./BoardShell.js";
import { SpaceAccountSlot } from "./SpaceAccountSlot.js";
import { SpaceProductBadge } from "./SpaceProductBadge.js";
import { SpaceThemeSwitch } from "./SpaceThemeSwitch.js";
import { SpaceAnchorNotFound, SpaceBoardError } from "./FailureSurfaces.js";
import { SpaceBoardHead } from "./SpaceBoardHead.js";

export interface BoardScreenProps {
  anchor: string;
  search: SpaceBoardSearch;
}

/**
 * The address facets the fetch consumes (SPC-026/036; slice F owns their UI).
 * The router hands back the raw address values alongside the validated ones
 * for keys a search schema did not declare, so this reads the address the
 * same way the parser does rather than trusting the shape.
 */
export function boardFiltersOf(search: SpaceBoardSearch): SpaceBoardFilters {
  const parsed = parseSpaceBoardSearch({ ...search });
  return {
    ...(parsed.state === undefined ? {} : { state: parsed.state }),
    ...(parsed.priority === undefined ? {} : { priority: parsed.priority }),
    ...(parsed.labels === undefined ? {} : { labels: parsed.labels }),
  };
}

export function BoardScreen({ anchor, search }: BoardScreenProps): React.ReactElement {
  if (!isValidAnchor(anchor)) {
    return (
      <>
        <SpaceBoardHead anchor={anchor} />
        <SpaceAnchorNotFound />
      </>
    );
  }
  return <ValidAnchorBoard anchor={anchor} search={search} />;
}

function ValidAnchorBoard({ anchor, search }: BoardScreenProps): React.ReactElement {
  const settings = useQuery(spaceSettingsQueryOptions(spaceRuntime.client, anchor));
  if (settings.isError) {
    return (
      <>
        <SpaceBoardHead anchor={anchor} />
        {classifySpaceFailure(settings.error) === "not-found" ? <SpaceAnchorNotFound /> : <SpaceBoardError />}
      </>
    );
  }
  if (settings.data === undefined) {
    return (
      <>
        <SpaceBoardHead anchor={anchor} />
        <div data-testid="space-board">
          <BoardNavbar
            settings={undefined}
            anchor={anchor}
            search={search}
            accountSlot={<SpaceAccountSlot anchor={anchor} search={search} />}
          >
            <SpaceThemeSwitch />
          </BoardNavbar>
          <div data-testid="space-board-placeholder" className="min-h-[60vh]" />
        </div>
      </>
    );
  }
  return (
    <>
      <SpaceBoardHead anchor={anchor} />
      <ResolvedBoard anchor={anchor} search={search} settings={settings.data} />
    </>
  );
}

function ResolvedBoard({
  anchor,
  search,
  settings,
}: {
  anchor: string;
  search: SpaceBoardSearch;
  settings: SpaceBoardSettings;
}): React.ReactElement {
  const filters = boardFiltersOf(search);
  const issues = useQuery(spaceIssuesQueryOptions(spaceRuntime.client, anchor, filters));
  const states = useQuery(spaceStatesQueryOptions(spaceRuntime.client, anchor));
  const { offered, active, select } = useSettledSpaceLayout(anchor, search, settings.view_props);
  const filterNav = useSpaceFilterNav(anchor, search, filters);
  const identifier = settings.project_details?.identifier ?? "";
  return (
    <div data-testid="space-board">
      <BoardNavbar
        settings={settings}
        anchor={anchor}
        search={search}
        accountSlot={<SpaceAccountSlot anchor={anchor} search={search} />}
      >
        <LayoutSwitcher offered={offered} active={active} onSelect={select} />
        {states.data === undefined ? null : (
          <SpaceFilterPanel layout={active} filters={filters} states={states.data} onToggle={filterNav.toggleValue} />
        )}
        <SpaceThemeSwitch />
      </BoardNavbar>
      {issues.isError || states.isError ? (
        <SpaceBoardError />
      ) : issues.data === undefined || states.data === undefined ? (
        <BoardLoading />
      ) : (
        <>
          <AppliedFilters
            filters={filters}
            states={states.data}
            onRemoveValue={filterNav.removeValue}
            onClearFacet={filterNav.clearFacet}
            onClearAll={filterNav.clearAll}
          />
          <BoardContent
            anchor={anchor}
            layout={active}
            filters={filters}
            page={issues.data}
            states={states.data}
            projectIdentifier={identifier}
          />
          <SpaceProductBadge />
        </>
      )}
    </div>
  );
}

function BoardContent({
  anchor,
  layout,
  filters,
  page,
  states,
  projectIdentifier,
}: {
  anchor: string;
  layout: SpaceLayout;
  filters: SpaceBoardFilters;
  page: SpaceBoardIssuePage;
  states: React.ComponentProps<typeof BoardLayouts>["states"];
  projectIdentifier: string;
}): React.ReactElement {
  return (
    <BoardLayouts
      client={spaceRuntime.client}
      anchor={anchor}
      layout={layout}
      filters={filters}
      states={states}
      page={page}
      projectIdentifier={projectIdentifier}
    />
  );
}

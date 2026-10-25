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
import { useNavigate } from "@tanstack/react-router";
import type { SpaceBoardIssue, SpaceBoardIssuePage, SpaceBoardSettings } from "@pidash/api-client";
import * as React from "react";

import { spaceRuntime } from "../api/client.js";
import {
  spaceIssuesQueryOptions,
  spaceLabelsQueryOptions,
  spaceSettingsQueryOptions,
  spaceStatesQueryOptions,
} from "../api/queries.js";
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
  spaceIssueHref,
  type SpaceBoardFilters,
  type SpaceBoardSearch,
  type SpaceLayout,
} from "../lib/address.js";
import { SpaceIssuePeek } from "../peek/SpaceIssuePeek.js";
import { spacePeekCloseHref, spacePeekOpenHref } from "../lib/peek.js";
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
  const labels = useQuery(spaceLabelsQueryOptions(spaceRuntime.client, anchor));
  const { offered, active, select } = useSettledSpaceLayout(anchor, search, settings.view_props);
  const filterNav = useSpaceFilterNav(anchor, search, filters);
  const peekNav = useSpacePeekNav(anchor, search);
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
      {search.peekId === undefined ? null : (
        <SpaceIssuePeek
          anchor={anchor}
          issueId={search.peekId}
          projectIdentifier={identifier}
          states={states.data}
          seed={peekSeed(issues.data, search.peekId)}
          onClose={peekNav.close}
          onInvalid={peekNav.invalid}
        />
      )}
      {issues.isError || states.isError || labels.isError ? (
        <SpaceBoardError />
      ) : issues.data === undefined || states.data === undefined || labels.data === undefined ? (
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
            labels={labels.data}
            search={search}
            projectIdentifier={identifier}
            votesEnabled={settings.is_votes_enabled}
            reactionsEnabled={settings.is_reactions_enabled}
            openIssue={peekNav.open}
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
  labels,
  search,
  projectIdentifier,
  votesEnabled,
  reactionsEnabled,
  openIssue,
}: {
  anchor: string;
  layout: SpaceLayout;
  filters: SpaceBoardFilters;
  page: SpaceBoardIssuePage;
  states: React.ComponentProps<typeof BoardLayouts>["states"];
  labels: React.ComponentProps<typeof BoardLayouts>["labels"];
  search: SpaceBoardSearch;
  projectIdentifier: string;
  votesEnabled: boolean;
  reactionsEnabled: boolean;
  openIssue: (issueId: string) => void;
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
      labels={labels}
      openIssueId={search.peekId}
      onOpenIssue={(issue) => openIssue(issue.id)}
      issueHref={(issue) => spaceIssueHref(anchor, search, issue.id)}
      votesEnabled={votesEnabled}
      reactionsEnabled={reactionsEnabled}
    />
  );
}

/**
 * The list row already loaded for the open issue, if any (SPC-046). Its
 * key and title seed the overlay header while the detail fetch is still
 * resolving; nothing else is taken from it.
 */
export function peekSeed(
  page: SpaceBoardIssuePage | undefined,
  peekId: string | undefined
): Pick<SpaceBoardIssue, "name" | "sequence_id"> | undefined {
  if (page === undefined || peekId === undefined) return undefined;
  for (const bucket of Object.values(page.results)) {
    const row = bucket.results.find((issue) => issue.id === peekId);
    if (row !== undefined) return { name: row.name, sequence_id: row.sequence_id };
  }
  return undefined;
}

/**
 * The overlay's address round trip (SPC-030/040/042). Opening pushes one
 * entry naming the issue over the current board position; closing and an
 * unusable id both replace the address without peekId, so view and filters
 * survive and Back does not replay the overlay.
 */
export interface SpacePeekNav {
  open: (issueId: string) => void;
  close: () => void;
  invalid: () => void;
}

export function useSpacePeekNav(anchor: string, search: SpaceBoardSearch): SpacePeekNav {
  const navigate = useNavigate();
  const navigateRef = React.useRef(navigate);
  navigateRef.current = navigate;
  const open = React.useCallback(
    (issueId: string) => {
      void navigateRef.current({ href: spacePeekOpenHref(anchor, search, issueId) });
    },
    [anchor, search]
  );
  const dismiss = React.useCallback(() => {
    void navigateRef.current({ href: spacePeekCloseHref(anchor, search), replace: true });
  }, [anchor, search]);
  return { open, close: dismiss, invalid: dismiss };
}

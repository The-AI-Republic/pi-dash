// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// SPC-026: per-group paging. Groups come from the first page; asking one
// group for more appends only that group's next page, and the others keep
// their rows, cursor and expanded state. One request per state is in flight
// at a time, so a scroll burst or a double click cannot fetch a page twice.

import { useQueryClient } from "@tanstack/react-query";
import type { ApiClient, SpaceBoardIssuePage, SpaceBoardState } from "@pidash/api-client";
import * as React from "react";

import { spaceGroupQueryOptions } from "../api/queries.js";
import type { SpaceBoardFilters } from "../lib/address.js";
import { appendGroupPage, findGroup, firstPageGroups, type SpaceIssueGroup } from "./model.js";

export interface SpaceGroupsState {
  groups: SpaceIssueGroup[];
  /** State ids with a page request in flight (placeholder rows show here). */
  paging: ReadonlySet<string>;
  /** State ids whose last page request failed (the control offers a retry). */
  failed: ReadonlySet<string>;
  /** Ask one group for its next page; a no-op while it is in flight or done. */
  loadMore: (stateId: string) => void;
}

interface PagingState {
  /** The first page these later pages extend. */
  base: SpaceBoardIssuePage;
  loaded: ReadonlyMap<string, SpaceIssueGroup>;
  paging: ReadonlySet<string>;
  failed: ReadonlySet<string>;
}

function emptyState(base: SpaceBoardIssuePage): PagingState {
  return { base, loaded: new Map(), paging: new Set(), failed: new Set() };
}

/**
 * Fold the first page and any loaded later pages into the renderable groups.
 * Later pages are dropped whenever the first page changes (new filters, a
 * reload), so rows from two snapshots can never mix.
 */
export function useSpaceGroups(
  client: ApiClient,
  anchor: string,
  filters: SpaceBoardFilters,
  states: readonly SpaceBoardState[],
  page: SpaceBoardIssuePage
): SpaceGroupsState {
  const queryClient = useQueryClient();
  const firstPage = React.useMemo(() => firstPageGroups(states, page), [states, page]);
  const [state, setState] = React.useState<PagingState>(() => emptyState(page));
  // Adjusting state during render is the sanctioned way to reset derived
  // paging when the page under it changes; the render continues from the
  // reset value.
  const live = state.base === page ? state : emptyState(page);
  if (live !== state) setState(live);

  // The guard is a ref so two triggers in one commit cannot both pass it.
  const inFlight = React.useRef(new Set<string>());
  const lastBase = React.useRef(page);
  if (lastBase.current !== page) {
    lastBase.current = page;
    inFlight.current.clear();
  }

  const groups = React.useMemo(
    () => firstPage.map((group) => live.loaded.get(group.stateId) ?? group),
    [firstPage, live.loaded]
  );
  const groupsRef = React.useRef(groups);
  groupsRef.current = groups;
  const filtersRef = React.useRef(filters);
  filtersRef.current = filters;

  const loadMore = React.useCallback(
    (stateId: string) => {
      const group = findGroup(groupsRef.current, stateId);
      if (group === undefined || group.nextCursor === null) return;
      if (inFlight.current.has(stateId)) return;
      const cursor = group.nextCursor;
      inFlight.current.add(stateId);
      setState((current) => {
        const settled = current.base === page ? current : emptyState(page);
        return {
          ...settled,
          paging: new Set(settled.paging).add(stateId),
          failed: new Set([...settled.failed].filter((id) => id !== stateId)),
        };
      });

      const settle = (apply: (current: PagingState) => PagingState): void => {
        inFlight.current.delete(stateId);
        setState((current) => (current.base === page ? apply(current) : current));
      };

      void queryClient
        .fetchQuery(spaceGroupQueryOptions(client, anchor, filtersRef.current, stateId, cursor))
        .then((next) => {
          settle((current) => {
            const loaded = new Map(current.loaded);
            loaded.set(stateId, appendGroupPage(loaded.get(stateId) ?? group, next));
            const paging = new Set(current.paging);
            paging.delete(stateId);
            return { ...current, loaded, paging };
          });
        })
        .catch(() => {
          settle((current) => {
            const paging = new Set(current.paging);
            paging.delete(stateId);
            return { ...current, paging, failed: new Set(current.failed).add(stateId) };
          });
        });
    },
    [anchor, client, page, queryClient]
  );

  return { groups, paging: live.paging, failed: live.failed, loadMore };
}

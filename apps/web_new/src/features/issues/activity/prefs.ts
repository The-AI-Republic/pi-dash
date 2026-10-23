// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The feed's two browser-local choices (parity/specs/issues.md §16,
// ISS-195/196). A hook so the component stays declarative; every read is
// re-validated, so a hand-edited or half-written value falls back to the
// defaults instead of breaking the feed.

import * as React from "react";

import {
  CLOUD_FEED_FILTERS,
  DEFAULT_SORT,
  FEED_FILTERS,
  FILTERS_STORAGE_KEY,
  SORT_STORAGE_KEY,
  parseStoredFilters,
  parseStoredSort,
  serializeFilters,
  serializeSort,
  type FeedFilter,
  type FeedSortOrder,
} from "./model.js";

function readFilters(available: readonly FeedFilter[]): FeedFilter[] {
  try {
    return parseStoredFilters(window.localStorage.getItem(FILTERS_STORAGE_KEY), available);
  } catch {
    return [...available];
  }
}

function readSort(): FeedSortOrder {
  try {
    return parseStoredSort(window.localStorage.getItem(SORT_STORAGE_KEY));
  } catch {
    return DEFAULT_SORT;
  }
}

export interface FeedPrefs {
  filters: FeedFilter[];
  setFilters: (next: FeedFilter[]) => void;
  order: FeedSortOrder;
  setOrder: (next: FeedSortOrder) => void;
}

export function useFeedPrefs(worklogEnabled: boolean = false): FeedPrefs {
  const available = worklogEnabled ? CLOUD_FEED_FILTERS : FEED_FILTERS;
  const [filters, setFilterState] = React.useState<FeedFilter[]>(() => readFilters(available));
  const [order, setOrderState] = React.useState<FeedSortOrder>(readSort);

  const setFilters = React.useCallback(
    (next: FeedFilter[]) => {
      setFilterState(next);
      try {
        window.localStorage.setItem(FILTERS_STORAGE_KEY, serializeFilters(next, available));
      } catch {
        // Storage can be denied (private mode); the feed still narrows in
        // this tab, it just does not remember the choice.
      }
    },
    [available]
  );

  const setOrder = React.useCallback((next: FeedSortOrder) => {
    setOrderState(next);
    try {
      window.localStorage.setItem(SORT_STORAGE_KEY, serializeSort(next));
    } catch {
      // Same as above.
    }
  }, []);

  return { filters, setFilters, order, setOrder };
}

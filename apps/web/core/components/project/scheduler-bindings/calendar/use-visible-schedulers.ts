/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useMemo } from "react";
import { useLocalStorage } from "@pi-dash/hooks";

/**
 * Per-user per-project state for which schedulers' occurrences are visible
 * on the calendar. Backed by the shared ``useLocalStorage`` hook so the
 * choice survives reloads and broadcasts across tabs.
 *
 * Default: all known scheduler IDs are visible.
 */
export function useVisibleSchedulers(projectId: string, knownIds: string[]) {
  const storageKey = `scheduler-calendar:visible:${projectId}`;
  const { storedValue, setValue } = useLocalStorage<string[]>(storageKey, []);

  const hiddenIds = useMemo(() => new Set(storedValue ?? []), [storedValue]);

  const isVisible = useCallback((schedulerId: string) => !hiddenIds.has(schedulerId), [hiddenIds]);

  const toggle = useCallback(
    (schedulerId: string) => {
      const next = new Set(hiddenIds);
      if (next.has(schedulerId)) next.delete(schedulerId);
      else next.add(schedulerId);
      setValue([...next]);
    },
    [hiddenIds, setValue]
  );

  const showAll = useCallback(() => setValue([]), [setValue]);
  const hideAll = useCallback(() => setValue([...knownIds]), [setValue, knownIds]);

  return { isVisible, toggle, showAll, hideAll, hiddenIds };
}

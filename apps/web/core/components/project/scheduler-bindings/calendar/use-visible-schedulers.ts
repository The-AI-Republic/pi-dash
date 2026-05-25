/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useEffect, useState } from "react";

/**
 * Per-user per-project state for which schedulers' occurrences are visible
 * on the calendar. Persists to localStorage so the choice survives reloads.
 *
 * Default: all known scheduler IDs are visible. As new schedulers appear
 * (newly installed bindings between renders), they default to visible too.
 */
export function useVisibleSchedulers(projectId: string, knownIds: string[]) {
  const storageKey = `scheduler-calendar:visible:${projectId}`;

  // Lazily hydrate from localStorage so SSR doesn't break (the web app is
  // ssr:false, but the lazy form is the safer default).
  const [hiddenIds, setHiddenIds] = useState<Set<string>>(() => {
    if (typeof window === "undefined") return new Set();
    try {
      const raw = window.localStorage.getItem(storageKey);
      if (!raw) return new Set();
      const parsed = JSON.parse(raw) as string[];
      return new Set(parsed);
    } catch {
      return new Set();
    }
  });

  // Persist changes back to localStorage.
  useEffect(() => {
    if (typeof window === "undefined") return;
    try {
      window.localStorage.setItem(storageKey, JSON.stringify([...hiddenIds]));
    } catch {
      // Quota-exceeded or storage disabled; silently ignore.
    }
  }, [storageKey, hiddenIds]);

  const isVisible = useCallback((schedulerId: string) => !hiddenIds.has(schedulerId), [hiddenIds]);

  const toggle = useCallback((schedulerId: string) => {
    setHiddenIds((prev) => {
      const next = new Set(prev);
      if (next.has(schedulerId)) {
        next.delete(schedulerId);
      } else {
        next.add(schedulerId);
      }
      return next;
    });
  }, []);

  const showAll = useCallback(() => setHiddenIds(new Set()), []);
  const hideAll = useCallback(() => setHiddenIds(new Set(knownIds)), [knownIds]);

  return { isVisible, toggle, showAll, hideAll, hiddenIds };
}

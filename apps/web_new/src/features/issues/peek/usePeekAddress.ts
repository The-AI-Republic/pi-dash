// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Peek address for a layout (parity/specs/issues.md §14, ISS-167). A route
// that validates the peek params hands a layout the peeked id, the peeked
// project and a writer; the layout reports every open, nested change and
// close through that writer and never keeps the address on its own. A
// layout mounted without such a host still syncs: it reads the address on
// mount (a shared link opens), writes each change in place so Back leaves
// the page instead of walking the peek stack, and keeps the panel it opened
// mounted while a stacked child holds the address.

import * as React from "react";

import {
  peekSearchFor,
  peekSearchFromLocation,
  peekSelectionFromSearch,
  replacePeekSearch,
  stripPeekParams,
  type PeekRequest,
  type PeekSelection,
} from "./peekUrl.js";

export { PEEK_ISSUE_PARAM } from "./peekUrl.js";
export type { PeekRequest, PeekSelection } from "./peekUrl.js";

/**
 * The one address seam every issues layout accepts (ISS-165/167). A layout
 * spreads these straight into `usePeekAddress`, so a new layout cannot
 * compile without the address, and there is a single implementation of the
 * state machine rather than one per layout.
 */
export interface LayoutPeekProps {
  /** Controlled peeked id from the route; undefined leaves the layout in charge. */
  peekKey?: string | null | undefined;
  /** Controlled peeked project, written only when it differs from the route. */
  peekProjectId?: string | null | undefined;
  /** Controlled writer the route validates the address with. */
  onPeekChange?: ((request: PeekRequest) => void) | undefined;
}

export interface PeekAddressOptions {
  /** Project of the mounting route; the peeked project defaults here. */
  projectId: string;
  /** Controlled peeked id from the route; undefined leaves the layout in charge. */
  peekKey?: string | null | undefined;
  /** Controlled peeked project, written only when it differs from the route. */
  peekProjectId?: string | null | undefined;
  /** Controlled writer the route validates the address with. */
  onPeekChange?: ((request: PeekRequest) => void) | undefined;
}

export interface PeekAddress {
  /** Value the peek mount resolves: a work-item id, or a deep-linked key. */
  peeked: string | null;
  /** Project the peek reads from when it is not the route's own. */
  peekProjectId: string | null;
  /** Open the peek on a row the layout holds. */
  open: (selection: PeekSelection) => void;
  /** Report what the panel's nested open or child close now names. */
  report: (selection: PeekSelection) => void;
  /** Close the peek and remove its params. */
  close: () => void;
}

/**
 * State and writers for one layout's peek. Controlled: the route's params
 * decide what is mounted. Self-synced: the layout mounts the panel it
 * opened and mirrors each change into the address with `replaceState`.
 */
export function usePeekAddress({ projectId, peekKey, peekProjectId, onPeekChange }: PeekAddressOptions): PeekAddress {
  // Either half of the seam hands the address to the host: a route that
  // validates the key, or one that only takes the reports. Only a layout
  // given neither keeps the address (and a deep link) to itself.
  const controlled = peekKey !== undefined || onPeekChange !== undefined;
  const [mounted, setMounted] = React.useState<PeekSelection | null>(null);
  const writeAddress = React.useCallback(
    (next: PeekSelection | null): void => {
      const search = peekSearchFromLocation();
      replacePeekSearch(next === null ? stripPeekParams(search) : peekSearchFor(search, next, projectId));
    },
    [projectId]
  );

  // A shared link opens the peek; it never overrides a panel already up.
  React.useEffect(() => {
    if (controlled || mounted !== null) return;
    const linked = peekSelectionFromSearch(peekSearchFromLocation(), projectId);
    if (linked !== null) setMounted(linked);
  }, [controlled, mounted, projectId]);

  const open = React.useCallback(
    (selection: PeekSelection): void => {
      if (!controlled) setMounted(selection);
      if (controlled) {
        onPeekChange?.(selection);
        return;
      }
      writeAddress(selection);
    },
    [controlled, onPeekChange, writeAddress]
  );

  // The address follows a nested open or a child close; the mounted panel
  // stays, so the parent holds the stack until the child closes.
  const report = React.useCallback(
    (selection: PeekSelection): void => {
      if (controlled) {
        onPeekChange?.(selection);
        return;
      }
      writeAddress(selection);
    },
    [controlled, onPeekChange, writeAddress]
  );

  const close = React.useCallback((): void => {
    if (!controlled) setMounted(null);
    if (controlled) {
      onPeekChange?.(null);
      return;
    }
    writeAddress(null);
  }, [controlled, onPeekChange, writeAddress]);

  const peeked = controlled ? (peekKey ?? null) : (mounted?.issueId ?? null);
  const activeProject = controlled
    ? (peekProjectId ?? null)
    : mounted === null || mounted.projectId === projectId
      ? null
      : mounted.projectId;
  return React.useMemo(
    () => ({ peeked, peekProjectId: activeProject, open, report, close }),
    [activeProject, close, open, peeked, report]
  );
}

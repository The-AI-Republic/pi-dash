// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail header lock control (parity/specs/pages.md §2.3 and §7.3,
// PAGE-058). A thin face over the shared lock write helper: the state
// flips optimistically in the caches and rolls back with a notice on
// failure. Unlocking passes through a brief transitional label before
// settling on resting; locking settles straight onto the locked label.
// Non-privileged viewers never mount this — the screen decides.

import { Lock, LockOpen } from "lucide-react";
import * as React from "react";

import { IconButton } from "@pidash/kit";
import type { PageRow as PageRowData } from "@pidash/api-client";

import { usePageLockWrite } from "../api/rowMutations.js";

/** Pinned accessible names; the oracle reads all three literally. */
export const PAGE_LOCK_LABEL = "Lock";
export const PAGE_LOCKED_LABEL = "Locked";
export const PAGE_UNLOCKED_LABEL = "Unlocked";

/** How long the control reports the transitional phase before resting. */
export const PAGE_UNLOCK_TRANSITION_MS = 600;

export interface PageDetailLockControlProps {
  workspaceSlug: string;
  projectId: string;
  page: Pick<PageRowData, "id" | "name" | "is_locked">;
}

export function PageDetailLockControl({
  workspaceSlug,
  projectId,
  page,
}: PageDetailLockControlProps): React.ReactElement {
  const { pending, toggleLock } = usePageLockWrite({ workspaceSlug, projectId, page });
  const [transitional, setTransitional] = React.useState(false);
  const timerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  React.useEffect(
    () => () => {
      if (timerRef.current !== null) clearTimeout(timerRef.current);
    },
    []
  );
  // A refused unlock rolls the cached row back to locked; a transitional
  // control must follow it instead of reporting a state that never landed.
  React.useEffect(() => {
    if (page.is_locked) setTransitional(false);
  }, [page.is_locked]);

  const label = transitional ? PAGE_UNLOCKED_LABEL : page.is_locked ? PAGE_LOCKED_LABEL : PAGE_LOCK_LABEL;

  function handleToggle(): void {
    // No pending guard here: the button is disabled while the write flies,
    // and the shared write ignores a press that slips through anyway.
    const unlocking = page.is_locked;
    toggleLock(!page.is_locked);
    if (unlocking) {
      setTransitional(true);
      if (timerRef.current !== null) clearTimeout(timerRef.current);
      timerRef.current = setTimeout(() => setTransitional(false), PAGE_UNLOCK_TRANSITION_MS);
    }
  }

  return (
    <IconButton
      label={label}
      aria-busy={pending}
      data-testid="page-detail-lock"
      size="small"
      disabled={pending}
      onClick={handleToggle}
    >
      {page.is_locked && !transitional ? (
        <Lock size={16} strokeWidth={1.5} aria-hidden="true" />
      ) : (
        <LockOpen size={16} strokeWidth={1.5} aria-hidden="true" />
      )}
    </IconButton>
  );
}

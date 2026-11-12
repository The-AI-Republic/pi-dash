// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Press routing for the modules list row and board card (parity/specs/
// modules.md §3.5, §3.6, §5, MOD-012, MOD-019, MOD-020). Both surfaces are
// partly a navigation target — a live row's name, a card's body — and
// partly a tray of controls. A press on a control must keep that control's
// own meaning and never also navigate or open the peek, so both ask this
// one question before acting on a press.

import type * as React from "react";

/**
 * Whether a press landed on a control (or a real anchor) rather than on
 * the surface's own body. A null target (a synthetic or detached event)
 * counts as the body.
 */
export function modulePressHitControl(event: React.SyntheticEvent<HTMLElement>): boolean {
  const target = event.target;
  return target instanceof Element && target.closest("a,button,[role='button']") !== null;
}

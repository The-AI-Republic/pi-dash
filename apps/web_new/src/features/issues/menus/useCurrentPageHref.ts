// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The address of the page the whole-list ellipsis sits on
// (parity/specs/issues.md §7, ISS-067). Kept apart from the component so
// surfaces mounted outside a router can still render the header and so
// tests can pin what gets copied.

import { useRouterState } from "@tanstack/react-router";

import { currentPagePath } from "./ListPageQuickActions.js";

export function useCurrentPageHref(): string {
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const search = useRouterState({ select: (state) => state.location.searchStr });
  return currentPagePath(pathname, search);
}

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Fetch policy for workspace-settings reads (parity/specs/workspace-settings.md
// §38, WSET-038). Settings lists read on mount only: a returning focus
// must not re-read them, and nothing opens a socket. Page slices merge
// these options into their query options.

import { LIST_STALE_TIME_MS } from "../../../core/query/index.js";

export const WORKSPACE_SETTINGS_QUERY_POLICY = {
  staleTime: LIST_STALE_TIME_MS,
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
  refetchOnMount: true,
  refetchInterval: false,
} as const;

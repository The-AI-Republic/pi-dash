// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Fetch policy (WSET-038, spec §38): settings lists read on mount and
// never on a returning focus, a reconnect or a timer.
import { describe, expect, it } from "vitest";

import { LIST_STALE_TIME_MS } from "../../../core/query/index.js";
import { WORKSPACE_SETTINGS_QUERY_POLICY } from "./settingsQuery.js";

describe("WORKSPACE_SETTINGS_QUERY_POLICY", () => {
  it("reads once on mount", () => {
    expect(WORKSPACE_SETTINGS_QUERY_POLICY.refetchOnMount).toBe(true);
    expect(WORKSPACE_SETTINGS_QUERY_POLICY.staleTime).toBe(LIST_STALE_TIME_MS);
  });

  it("never re-reads from focus, reconnects or a poll", () => {
    expect(WORKSPACE_SETTINGS_QUERY_POLICY.refetchOnWindowFocus).toBe(false);
    expect(WORKSPACE_SETTINGS_QUERY_POLICY.refetchOnReconnect).toBe(false);
    expect(WORKSPACE_SETTINGS_QUERY_POLICY.refetchInterval).toBe(false);
  });
});

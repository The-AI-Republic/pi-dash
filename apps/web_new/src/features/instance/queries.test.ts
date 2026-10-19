// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { ApiClient } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { appInstanceQueryOptions, instanceAppKeys } from "./queries.js";

describe("appInstanceQueryOptions", () => {
  it("keeps the startup read in its own key and reads once", () => {
    expect(instanceAppKeys.bootstrap).toEqual(["instance", "app", "bootstrap"]);
    const options = appInstanceQueryOptions({} as ApiClient);
    expect(options.queryKey).toEqual(instanceAppKeys.bootstrap);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.refetchInterval).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });
});

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { legacyRedirectOptions } from "./legacyRedirect.js";

describe("legacyRedirectOptions", () => {
  it("carries the target address and keeps the query", () => {
    expect(legacyRedirectOptions("/settings", "?tab=profile")).toEqual({
      to: "/settings/profile/general?tab=profile",
      search: true,
    });
  });

  it("answers null outside the legacy families", () => {
    expect(legacyRedirectOptions("/acme/projects/p1", "")).toBeNull();
  });
});

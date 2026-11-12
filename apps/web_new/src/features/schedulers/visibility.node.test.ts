// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rail visibility outside a browser (node rendering / tests): there is no
// page storage to read, so the choice stays in memory and nothing throws.
import { describe, expect, it } from "vitest";

import { browserStorage, readHiddenSchedulers, writeHiddenSchedulers } from "./visibility.js";

describe("visibility without a page", () => {
  it("has no storage to read or write", () => {
    expect(globalThis.window).toBeUndefined();
    expect(browserStorage()).toBeNull();
    expect(readHiddenSchedulers(browserStorage(), "acme", "p1")).toEqual([]);
    expect(() => writeHiddenSchedulers(browserStorage(), "acme", "p1", ["s1"])).not.toThrow();
  });
});

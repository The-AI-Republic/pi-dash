// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { spaceKeys } from "./keys.js";

describe("space query keys", () => {
  it("roots every public key at spaces, never the workspace cache", () => {
    expect(spaceKeys.instance).toEqual(["spaces", "instance"]);
    expect(spaceKeys.anchor("a1")).toEqual(["spaces", "anchor", "a1"]);
    expect(spaceKeys.settings("a1")).toEqual(["spaces", "anchor", "a1", "settings"]);
    expect(spaceKeys.issues("a1")).toEqual(["spaces", "anchor", "a1", "issues"]);
    expect(spaceKeys.states("a1")).toEqual(["spaces", "anchor", "a1", "states"]);
    expect(spaceKeys.meta("a1")).toEqual(["spaces", "anchor", "a1", "meta"]);
    expect(spaceKeys.legacy("acme", "p1")).toEqual(["spaces", "legacy", "acme", "p1"]);
    for (const key of [spaceKeys.all, spaceKeys.instance, spaceKeys.anchor("a1")]) {
      expect(key[0]).toBe("spaces");
    }
  });
});

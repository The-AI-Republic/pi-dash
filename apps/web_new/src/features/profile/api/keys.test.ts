// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile query keys (parity/specs/profile.md §1–4): workspace-scoped,
// with the member and ownership selector keeping the tabs apart.
import { describe, expect, it } from "vitest";

import { profileKeys } from "./keys.js";

describe("profileKeys", () => {
  it("scopes every key under the workspace and profile subtree", () => {
    const all = profileKeys.all("acme");
    expect(all[all.length - 1]).toBe("profile");
    expect(profileKeys.segregation("acme", "u-1").slice(0, all.length)).toEqual([...all]);
    expect(profileKeys.stats("acme", "u-1").slice(0, all.length)).toEqual([...all]);
    expect(profileKeys.issues("acme", "u-1", "assigned").slice(0, all.length)).toEqual([...all]);
    expect(profileKeys.activity("acme", "u-1", 100).slice(0, all.length)).toEqual([...all]);
  });

  it("separates the three issue tabs and the page sizes", () => {
    expect(profileKeys.issues("acme", "u-1", "assigned")).not.toEqual(profileKeys.issues("acme", "u-1", "created"));
    expect(profileKeys.issues("acme", "u-1", "subscribed")).not.toEqual(
      profileKeys.issues("acme", "u-2", "subscribed")
    );
    expect(profileKeys.activity("acme", "u-1", 10)).not.toEqual(profileKeys.activity("acme", "u-1", 100));
    expect(profileKeys.stats("acme", "u-1")).not.toEqual(profileKeys.stats("beta", "u-1"));
  });
});

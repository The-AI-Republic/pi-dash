// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { intakeFeatureSettingsHref, intakeHref, retiredInboxHref } from "./paths.js";

describe("intake addresses (INT-002)", () => {
  it("builds the canonical screen address", () => {
    expect(intakeHref("acme", "p-1")).toBe("/acme/projects/p-1/intake");
  });

  it("builds the retired inbox address", () => {
    expect(retiredInboxHref("acme", "p-1")).toBe("/acme/projects/p-1/inbox");
  });

  it("targets the feature leaf, never the bare index", () => {
    expect(intakeFeatureSettingsHref("acme", "p-1")).toBe("/acme/settings/projects/p-1/features/intake");
  });
});

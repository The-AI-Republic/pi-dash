// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { canEditModule, canShowModuleStar } from "./modulePermissions.js";

describe("canEditModule", () => {
  it.each([20, 15])("lets a live-module role %i edit", (memberRole) => {
    expect(canEditModule({ memberRole, archived: false })).toBe(true);
  });

  it.each([5, 0, null])("refuses a live-module role %s", (memberRole) => {
    expect(canEditModule({ memberRole, archived: false })).toBe(false);
  });

  it.each([20, 15, 5, null])("refuses every role %s on an archived row", (memberRole) => {
    expect(canEditModule({ memberRole, archived: true })).toBe(false);
  });
});

describe("canShowModuleStar", () => {
  it.each([20, 15])("shows the star to a live-module role %i", (memberRole) => {
    expect(canShowModuleStar({ memberRole, archived: false })).toBe(true);
  });

  it.each([5, 0, null])("hides the star from a live-module role %s", (memberRole) => {
    expect(canShowModuleStar({ memberRole, archived: false })).toBe(false);
  });

  it.each([20, 15, 5, null])("hides the star on an archived row for role %s", (memberRole) => {
    expect(canShowModuleStar({ memberRole, archived: true })).toBe(false);
  });
});

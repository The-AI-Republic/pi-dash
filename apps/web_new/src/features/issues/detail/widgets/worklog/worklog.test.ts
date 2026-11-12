// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { edition } from "../../../../../core/edition/index.js";
import { isWorklogEnabled, WORKLOG_FLAG } from "./worklog.js";

describe("worklog edition seam", () => {
  it("is off in the community build", () => {
    expect(edition.flags[WORKLOG_FLAG]).toBeUndefined();
    expect(isWorklogEnabled()).toBe(false);
    expect(isWorklogEnabled({})).toBe(false);
  });

  it("turns on only for an explicit true flag", () => {
    expect(isWorklogEnabled({ [WORKLOG_FLAG]: true })).toBe(true);
    expect(isWorklogEnabled({ [WORKLOG_FLAG]: false })).toBe(false);
  });
});

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { isNotificationsPath } from "./sidebarAutohide.js";

describe("isNotificationsPath", () => {
  it("matches the workspace notifications route only", () => {
    expect(isNotificationsPath("/acme/notifications/")).toBe(true);
    expect(isNotificationsPath("/acme/")).toBe(false);
    expect(isNotificationsPath("/acme/projects/")).toBe(false);
    expect(isNotificationsPath("/acme/notifications")).toBe(false);
  });
});

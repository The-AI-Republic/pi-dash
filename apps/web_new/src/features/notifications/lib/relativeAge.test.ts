// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { formatNotificationAge } from "./relativeAge.js";

const NOW = Date.parse("2026-10-17T12:00:00Z");

describe("formatNotificationAge", () => {
  it.each([
    ["just posted", "2026-10-17T11:59:30Z", "just now"],
    ["minutes old", "2026-10-17T11:58:00Z", "2 minutes ago"],
    ["one minute old", "2026-10-17T11:59:00Z", "1 minute ago"],
    ["hours old", "2026-10-17T09:00:00Z", "3 hours ago"],
    ["one hour old", "2026-10-17T11:00:00Z", "1 hour ago"],
    ["days old", "2026-10-14T12:00:00Z", "3 days ago"],
    ["one day old", "2026-10-16T12:00:00Z", "1 day ago"],
    ["weeks old", "2026-09-26T12:00:00Z", "3 weeks ago"],
    ["one week old", "2026-10-10T12:00:00Z", "1 week ago"],
    ["months old", "2026-07-17T12:00:00Z", "3 months ago"],
    ["one month old", "2026-09-10T12:00:00Z", "1 month ago"],
    ["years old", "2024-10-17T12:00:00Z", "2 years ago"],
    ["one year old", "2025-10-17T12:00:00Z", "1 year ago"],
    ["unparseable stamp", "not a date", "just now"],
  ])("%s", (_label, createdAt, expected) => {
    expect(formatNotificationAge(createdAt, NOW)).toBe(expected);
  });
});

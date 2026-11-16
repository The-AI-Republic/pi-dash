// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics tab model tests (parity/specs/analytics.md §1, ANL-001).
import { describe, expect, it } from "vitest";

import {
  ANALYTICS_TAB_IDS,
  ANALYTICS_TAB_LABELS,
  DEFAULT_ANALYTICS_TAB,
  analyticsTabHref,
  defaultAnalyticsTabHref,
  isAnalyticsTabId,
  selectAnalyticsTab,
} from "./tabs.js";

describe("analytics tabs", () => {
  it("lists overview first, then work-items", () => {
    expect([...ANALYTICS_TAB_IDS]).toEqual(["overview", "work-items"]);
    expect(DEFAULT_ANALYTICS_TAB).toBe("overview");
    expect(ANALYTICS_TAB_LABELS).toEqual({ overview: "Overview", "work-items": "Work items" });
  });

  it("recognizes only the two known ids", () => {
    expect(isAnalyticsTabId("overview")).toBe(true);
    expect(isAnalyticsTabId("work-items")).toBe(true);
    expect(isAnalyticsTabId("no-such-tab")).toBe(false);
    expect(isAnalyticsTabId("")).toBe(false);
  });

  it("resolves a segment to its tab, or null when unknown", () => {
    expect(selectAnalyticsTab("overview")).toBe("overview");
    expect(selectAnalyticsTab("work-items")).toBe("work-items");
    expect(selectAnalyticsTab("no-such-tab")).toBeNull();
  });

  it("builds shareable tab addresses", () => {
    expect(analyticsTabHref("acme", "overview")).toBe("/acme/analytics/overview");
    expect(analyticsTabHref("acme", "no-such-tab")).toBe("/acme/analytics/no-such-tab");
  });

  it("forwards the bare address to the default tab with the query kept", () => {
    expect(defaultAnalyticsTabHref("acme", "")).toBe("/acme/analytics/overview");
    expect(defaultAnalyticsTabHref("acme", "?project_ids=a%2Cb")).toBe("/acme/analytics/overview?project_ids=a%2Cb");
  });
});

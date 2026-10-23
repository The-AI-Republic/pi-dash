// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { makeView } from "../testing.js";
import { savedViewDocumentTitle, savedViewLayout, savedViewTitle, showsPrivateLock } from "./model.js";

describe("saved view detail model", () => {
  it("builds the tab title from the project and view names", () => {
    expect(savedViewTitle("Website", "Triage")).toBe("Website - Triage");
  });

  it("leaves the project half out while it is unknown", () => {
    expect(savedViewTitle(undefined, "Triage")).toBe("Triage");
    expect(savedViewTitle("", "Triage")).toBe("Triage");
  });

  it("uses the view name alone once the record resolves", () => {
    expect(savedViewDocumentTitle("Website", { name: "Triage" })).toBe("Website - Triage");
    expect(savedViewDocumentTitle(undefined, { name: "Triage" })).toBe("Triage");
  });

  it("falls back to the project or the area name while there is no record", () => {
    expect(savedViewDocumentTitle("Website", null)).toBe("Website");
    expect(savedViewDocumentTitle(undefined, null)).toBe("Views");
    expect(savedViewDocumentTitle("", null)).toBe("Views");
  });

  it("reads every stored layout key and defaults unknown values to list", () => {
    expect(savedViewLayout(makeView({ display_filters: { layout: "kanban" } }))).toBe("kanban");
    expect(savedViewLayout(makeView({ display_filters: { layout: "calendar" } }))).toBe("calendar");
    expect(savedViewLayout(makeView({ display_filters: { layout: "spreadsheet" } }))).toBe("spreadsheet");
    expect(savedViewLayout(makeView({ display_filters: { layout: "gantt_chart" } }))).toBe("gantt_chart");
    expect(savedViewLayout(makeView({ display_filters: { layout: "list" } }))).toBe("list");
    expect(savedViewLayout(makeView({ display_filters: { layout: "timeline" } }))).toBe("list");
    expect(savedViewLayout(makeView({ display_filters: {} }))).toBe("list");
  });

  it("shows the lock for every owner-only access value", () => {
    expect(showsPrivateLock({ access: 0 })).toBe(true);
    expect(showsPrivateLock({ access: 2 })).toBe(true);
    expect(showsPrivateLock({ access: 1 })).toBe(false);
  });
});

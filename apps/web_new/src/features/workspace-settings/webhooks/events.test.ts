// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook event rules (parity/specs/workspace-settings.md §19/§21,
// WSET-020/022): the default, the mode round trip, toggles and submit.
import { describe, expect, it } from "vitest";

import type { WorkspaceWebhook } from "@pidash/api-client";
import {
  ALL_EVENTS,
  eventSelectionFor,
  initialEventSelection,
  normalizeWebhookUrl,
  NO_EVENTS,
  selectMode,
  submittedFlags,
  toggleEvent,
  usesAllEvents,
  WEBHOOK_EVENT_OPTIONS,
} from "./events.js";

function row(overrides: Partial<WorkspaceWebhook> = {}): WorkspaceWebhook {
  return {
    id: "223e4567-e89b-12d3-a456-426614174001",
    url: "https://example.com/post",
    is_active: true,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    project: true,
    issue: true,
    cycle: true,
    module: true,
    issue_comment: true,
    ...overrides,
  };
}

describe("webhook event selection", () => {
  it("starts in all-events mode with all five toggles on", () => {
    const selection = initialEventSelection();
    expect(selection.mode).toBe("all");
    expect(selection.flags).toEqual(ALL_EVENTS);
    expect(Object.keys(selection.flags)).toHaveLength(5);
  });

  it("orders the options projects, cycles, work items, modules, comments", () => {
    expect(WEBHOOK_EVENT_OPTIONS.map((option) => option.label)).toEqual([
      "Projects",
      "Cycles",
      "Work items",
      "Modules",
      "Work item comments",
    ]);
    expect(WEBHOOK_EVENT_OPTIONS.every((option) => option.description !== "")).toBe(true);
  });

  it("derives all-events mode from a stored all-on row", () => {
    expect(eventSelectionFor(row())).toEqual({ mode: "all", flags: ALL_EVENTS });
  });

  it("derives individual mode and the stored flags from a partial row", () => {
    const selection = eventSelectionFor(
      row({ project: true, cycle: false, module: false, issue: false, issue_comment: false })
    );
    expect(selection.mode).toBe("individual");
    expect(selection.flags).toEqual({ ...NO_EVENTS, project: true });
    expect(usesAllEvents(selection.flags)).toBe(false);
  });

  it("turns every flag on when switching to all-events", () => {
    const next = selectMode({ mode: "individual", flags: { ...NO_EVENTS, issue: true } }, "all");
    expect(next).toEqual({ mode: "all", flags: ALL_EVENTS });
  });

  it("keeps picks when returning to individual mode", () => {
    const next = selectMode({ mode: "all", flags: { ...NO_EVENTS, cycle: true } }, "individual");
    expect(next.mode).toBe("individual");
    expect(next.flags).toEqual({ ...NO_EVENTS, cycle: true });
  });

  it("starts individual mode all on when nothing was picked", () => {
    expect(selectMode({ mode: "all", flags: ALL_EVENTS }, "individual")).toEqual({
      mode: "individual",
      flags: ALL_EVENTS,
    });
  });

  it("starts individual mode all on when every toggle is off", () => {
    expect(selectMode({ mode: "all", flags: { ...NO_EVENTS } }, "individual")).toEqual({
      mode: "individual",
      flags: ALL_EVENTS,
    });
  });

  it("returns the same selection when the mode is unchanged", () => {
    const selection = { mode: "individual" as const, flags: { ...NO_EVENTS } };
    expect(selectMode(selection, "individual")).toBe(selection);
  });

  it("flips one flag without mutating the input", () => {
    const flags = { ...ALL_EVENTS };
    const next = toggleEvent(flags, "module");
    expect(next.module).toBe(false);
    expect(next.issue).toBe(true);
    expect(flags.module).toBe(true);
  });

  it("submits all flags in all-events mode and the toggles in individual mode", () => {
    expect(submittedFlags({ mode: "all", flags: NO_EVENTS })).toEqual(ALL_EVENTS);
    expect(submittedFlags({ mode: "individual", flags: { ...NO_EVENTS, issue_comment: true } })).toEqual({
      ...NO_EVENTS,
      issue_comment: true,
    });
  });

  it("trims the target URL", () => {
    expect(normalizeWebhookUrl("  https://example.com/post \n")).toBe("https://example.com/post");
    expect(normalizeWebhookUrl("   ")).toBe("");
  });
});

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The group a row lands in, and the marker word its lifecycle reads as.

import { describe, expect, it } from "vitest";

import { CYCLE_GROUP_HEADINGS, groupOfCycle, lifecycleLabel, lifecycleTone } from "./cycleGroups.js";

describe("cycle groups", () => {
  it("pins the singular disclosure headings", () => {
    expect(CYCLE_GROUP_HEADINGS.upcoming).toBe("Upcoming cycle");
    expect(CYCLE_GROUP_HEADINGS.completed).toBe("Completed cycle");
  });

  it("groups upcoming and draft rows together and finished rows apart", () => {
    expect(groupOfCycle("upcoming")).toBe("upcoming");
    expect(groupOfCycle("draft")).toBe("upcoming");
    expect(groupOfCycle("completed")).toBe("completed");
  });

  it("keeps current cycles out of the groups for the hero", () => {
    expect(groupOfCycle("current")).toBeNull();
  });

  it("compares markers case-insensitively and reads a missing marker as a draft row", () => {
    expect(groupOfCycle("UPCOMING")).toBe("upcoming");
    expect(groupOfCycle("Completed")).toBe("completed");
    expect(groupOfCycle("CURRENT")).toBeNull();
    expect(groupOfCycle(undefined)).toBe("upcoming");
    expect(groupOfCycle("")).toBe("upcoming");
  });

  it("labels every lifecycle for the row marker", () => {
    expect(lifecycleLabel("current")).toBe("In progress");
    expect(lifecycleLabel("upcoming")).toBe("Yet to start");
    expect(lifecycleLabel("completed")).toBe("Completed");
    expect(lifecycleLabel("draft")).toBe("Draft");
  });

  it("labels missing and unknown markers as drafts", () => {
    expect(lifecycleLabel(undefined)).toBe("Draft");
    expect(lifecycleLabel("")).toBe("Draft");
    expect(lifecycleLabel("CURRENT")).toBe("In progress");
  });

  it("tones each lifecycle for the marker badge", () => {
    expect(lifecycleTone("current")).toBe("accent");
    expect(lifecycleTone("upcoming")).toBe("warning");
    expect(lifecycleTone("completed")).toBe("success");
    expect(lifecycleTone("draft")).toBe("neutral");
    expect(lifecycleTone(undefined)).toBe("neutral");
    expect(lifecycleTone("CURRENT")).toBe("accent");
  });
});

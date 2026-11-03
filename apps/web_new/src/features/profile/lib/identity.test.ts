// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Identity presentation rules (parity/specs/profile.md §1 and §4).
import { describe, expect, it } from "vitest";

import {
  OWN_PROFILE_BREADCRUMB,
  formatJoinDate,
  profileBreadcrumbLabel,
  profileClock,
  profileDisplayName,
  profileFullName,
  profileInitial,
  projectCompletion,
  projectCompletionRatio,
} from "./identity.js";

describe("profile identity", () => {
  it("labels your own breadcrumb as your work and names someone else", () => {
    expect(OWN_PROFILE_BREADCRUMB).toBe("Your work");
    expect(profileBreadcrumbLabel({ first_name: "Opal", last_name: "Whelan" }, true)).toBe("Your work");
    expect(profileBreadcrumbLabel({ first_name: "Opal", last_name: "Whelan" }, false)).toBe("Opal Whelan Work");
    expect(profileBreadcrumbLabel({ first_name: "Opal", last_name: "" }, false)).toBe("Opal Work");
    expect(profileBreadcrumbLabel({ first_name: "", last_name: "" }, false)).toBe("Work");
  });

  it("collapses whitespace in the full name", () => {
    expect(profileFullName({ first_name: "  Opal ", last_name: " Whelan  " })).toBe("Opal Whelan");
    expect(profileFullName({ first_name: "Opal", last_name: "" })).toBe("Opal");
  });

  it("parenthesises a display name and hides a blank one", () => {
    expect(profileDisplayName("Opal W.")).toBe("(Opal W.)");
    expect(profileDisplayName("  Opal W. ")).toBe("(Opal W.)");
    expect(profileDisplayName("   ")).toBeNull();
    expect(profileDisplayName("")).toBeNull();
  });

  it("derives the avatar initial from the display name, falling back to the first name", () => {
    expect(profileInitial({ display_name: "opal", first_name: "Zed" })).toBe("O");
    expect(profileInitial({ display_name: "  ", first_name: "zed" })).toBe("Z");
    expect(profileInitial({ display_name: "", first_name: "" })).toBe("");
  });

  it("formats the join date with the injected formatter", () => {
    const date = new Date("2026-03-09T12:00:00Z");
    expect(formatJoinDate("2026-03-09T12:00:00Z", (value) => `${value.getTime()}`)).toBe(`${date.getTime()}`);
    expect(formatJoinDate("2026-03-09T12:00:00Z")).toMatch(/^[A-Z][a-z]{2} \d{1,2}, \d{4}$/);
  });

  it("reads the clock as HH:MM plus the timezone, folding midnight", () => {
    const at = new Date("2026-03-09T12:34:00Z");
    expect(profileClock("UTC", at)).toBe("12:34 UTC");
    const midnight = { format: () => "24:05" } as unknown as Intl.DateTimeFormat;
    expect(profileClock("Pacific/Kiritimati", at, midnight)).toBe("00:05 Pacific/Kiritimati");
  });

  it("computes project completion, guarding zero and out-of-range counts", () => {
    expect(projectCompletion(4, 1)).toBe("25%");
    expect(projectCompletion(3, 1)).toBe("33%");
    expect(projectCompletion(0, 0)).toBe("0%");
    expect(projectCompletion(-1, 4)).toBe("0%");
    expect(projectCompletionRatio(4, 1)).toBe(25);
    expect(projectCompletionRatio(0, 3)).toBe(0);
    expect(projectCompletionRatio(2, 4)).toBe(100);
    expect(projectCompletionRatio(2, -4)).toBe(0);
  });
});

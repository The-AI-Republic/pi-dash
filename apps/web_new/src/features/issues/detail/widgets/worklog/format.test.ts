// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { formatLoggedOn, totalWorklogMinutes, worklogActorLabel } from "./format.js";

const VIEWER = "123e4567-e89b-12d3-a456-426614174000";
const OTHER = "223e4567-e89b-12d3-a456-426614174001";

describe("totalWorklogMinutes", () => {
  it("sums the entries", () => {
    expect(totalWorklogMinutes([{ minutes: 90 }, { minutes: 30 }, { minutes: 0 }])).toBe(120);
  });

  it("reads an empty list as zero", () => {
    expect(totalWorklogMinutes([])).toBe(0);
  });
});

describe("formatLoggedOn", () => {
  it("renders a logged-on day as month, day and year", () => {
    expect(formatLoggedOn("2026-10-20")).toBe("Oct 20, 2026");
    expect(formatLoggedOn("2026-01-05")).toBe("Jan 5, 2026");
  });

  it("passes malformed values through untouched", () => {
    expect(formatLoggedOn("soon")).toBe("soon");
    expect(formatLoggedOn("2026-13-40")).toBe("2026-13-40");
    expect(formatLoggedOn("")).toBe("");
  });
});

describe("worklogActorLabel", () => {
  it("returns null for the viewer's own entries", () => {
    expect(worklogActorLabel({ actor: VIEWER, actor_detail: { display_name: "Me" } } as never, VIEWER)).toBeNull();
  });

  it("names someone else's entries", () => {
    expect(worklogActorLabel({ actor: OTHER, actor_detail: { display_name: "Ada L" } } as never, VIEWER)).toBe(
      "Ada L · "
    );
  });

  it("reads unnamed actors as Someone", () => {
    expect(worklogActorLabel({ actor: OTHER, actor_detail: null }, VIEWER)).toBe("Someone · ");
    expect(worklogActorLabel({ actor: OTHER, actor_detail: { display_name: "  " } } as never, VIEWER)).toBe(
      "Someone · "
    );
  });

  it("attributes entries when the viewer is unknown", () => {
    expect(worklogActorLabel({ actor: null, actor_detail: null }, null)).toBe("Someone · ");
  });
});

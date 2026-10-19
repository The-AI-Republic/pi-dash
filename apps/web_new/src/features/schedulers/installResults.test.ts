// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import type { Scheduler } from "@pidash/api-client";

import {
  bulkFailureTitle,
  bulkSelectionSummary,
  bulkSuccessTitle,
  eligibleDefinitions,
  filterProjects,
  firstFailureMessage,
  partitionInstallOutcomes,
  toggleAllBulkTargets,
  toggleBulkTarget,
} from "./installResults.js";

function scheduler(overrides: Partial<Scheduler> = {}): Scheduler {
  return {
    id: "s1",
    workspace: "w1",
    slug: "weekly",
    name: "Weekly review",
    description: "",
    prompt: "Review",
    color: "#5e6ad2",
    source: "builtin",
    is_enabled: true,
    active_binding_count: 0,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    ...overrides,
  };
}

describe("eligibleDefinitions", () => {
  it("keeps enabled, uninstalled definitions in list order", () => {
    const rows = [
      scheduler({ id: "a", slug: "a" }),
      scheduler({ id: "b", slug: "b", is_enabled: false }),
      scheduler({ id: "c", slug: "c" }),
    ];
    expect(eligibleDefinitions(rows, ["c"]).map((row) => row.id)).toEqual(["a"]);
    expect(eligibleDefinitions(rows, []).map((row) => row.id)).toEqual(["a", "c"]);
  });
});

describe("partitionInstallOutcomes", () => {
  it("splits successes and failures in target order", () => {
    const partition = partitionInstallOutcomes(
      ["a", "b", "c"],
      [
        { status: "fulfilled", value: null },
        { status: "rejected", reason: { fields: { rrule: ["Bad rule"], scheduler: ["Also bad"] } } },
        { status: "rejected", reason: new Error("Already installed") },
      ]
    );
    expect(partition.succeeded).toEqual(["a"]);
    expect(partition.failed).toEqual([
      { targetId: "b", ok: false, message: "Bad rule" },
      { targetId: "c", ok: false, message: "Already installed" },
    ]);
    expect(firstFailureMessage(partition)).toBe("Bad rule");
  });

  it("falls back through empty fields, blank messages, strings and unknown reasons", () => {
    const partition = partitionInstallOutcomes(
      ["a", "b", "c", "d", "e"],
      [
        { status: "rejected", reason: { fields: { project: [] }, message: "  " } },
        { status: "rejected", reason: "plain string" },
        { status: "rejected", reason: 42 },
        { status: "rejected", reason: null },
        { status: "rejected", reason: { fields: { project: ["No standing"] }, message: "ignored" } },
      ]
    );
    expect(partition.failed.map((row) => row.message)).toEqual([
      "The install was refused.",
      "plain string",
      "The install was refused.",
      "The install was refused.",
      "No standing",
    ]);
    expect(firstFailureMessage(partitionInstallOutcomes(["a"], [{ status: "fulfilled", value: null }]))).toBeNull();
  });

  it("treats a missing result as a refusal", () => {
    const partition = partitionInstallOutcomes(["a", "b"], [{ status: "fulfilled", value: null }]);
    expect(partition.failed).toEqual([{ targetId: "b", ok: false, message: "The install was refused." }]);
  });
});

describe("notice copy", () => {
  it("pluralises counts", () => {
    expect(bulkSuccessTitle(1)).toBe("Installed on 1 project");
    expect(bulkSuccessTitle(3)).toBe("Installed on 3 projects");
    expect(bulkFailureTitle(1)).toBe("1 project failed");
    expect(bulkFailureTitle(2)).toBe("2 projects failed");
    expect(bulkSelectionSummary(0)).toBe("Select projects");
    expect(bulkSelectionSummary(1)).toBe("1 project selected");
    expect(bulkSelectionSummary(4)).toBe("4 projects selected");
  });
});

describe("filterProjects", () => {
  const projects = [
    { id: "1", name: "Alpha launch", identifier: "ALP" },
    { id: "2", name: "Beta cleanup", identifier: "BET" },
  ];

  it("matches on name or identifier, case-insensitively", () => {
    expect(filterProjects(projects, "").length).toBe(2);
    expect(filterProjects(projects, "  bet ")).toEqual([projects[1]]);
    expect(filterProjects(projects, "alpha")).toEqual([projects[0]]);
    expect(filterProjects(projects, "nothing")).toEqual([]);
    expect(filterProjects([{ id: "3", name: "No identifier" }], "no identifier").length).toBe(1);
  });
});

describe("bulk selection", () => {
  it("toggles unlocked targets and refuses installed ones", () => {
    const base = { picked: [], installed: ["b"] };
    expect(toggleBulkTarget(base, "a")).toEqual({ picked: ["a"], installed: ["b"] });
    expect(toggleBulkTarget({ picked: ["a"], installed: ["b"] }, "a")).toEqual({ picked: [], installed: ["b"] });
    expect(toggleBulkTarget(base, "b")).toBe(base);
  });

  it("selects every unlocked target, then clears", () => {
    const base = { picked: [], installed: ["b"] };
    const all = toggleAllBulkTargets(base, ["a", "b", "c"]);
    expect(all.picked).toEqual(["a", "c"]);
    expect(toggleAllBulkTargets(all, ["a", "b", "c"]).picked).toEqual([]);
    expect(toggleAllBulkTargets(base, ["b"]).picked).toEqual([]);
  });
});

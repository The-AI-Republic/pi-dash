/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it } from "vitest";
import {
  extractBindingError,
  filterProjects,
  partitionInstallResults,
} from "@/components/schedulers/install-scheduler-helpers";

const fulfilled = (value: unknown = {}): PromiseFulfilledResult<unknown> => ({ status: "fulfilled", value });
const rejected = (reason: unknown): PromiseRejectedResult => ({ status: "rejected", reason });

describe("extractBindingError", () => {
  it("returns the top-level error message when present", () => {
    expect(extractBindingError({ error: "Project admin required." })).toBe("Project admin required.");
  });

  it("falls back through field errors in precedence order", () => {
    expect(extractBindingError({ rrule: ["Invalid RRULE."] })).toBe("Invalid RRULE.");
    expect(extractBindingError({ dtstart: ["Bad start."] })).toBe("Bad start.");
    expect(extractBindingError({ tzid: ["Unknown tz."] })).toBe("Unknown tz.");
    expect(extractBindingError({ scheduler: ["Gone."] })).toBe("Gone.");
    expect(extractBindingError({ pod: ["No pod."] })).toBe("No pod.");
  });

  it("prefers error over field errors", () => {
    expect(extractBindingError({ error: "Top.", rrule: ["Lower."] })).toBe("Top.");
  });

  it("returns null for shapes it cannot read", () => {
    expect(extractBindingError(null)).toBeNull();
    expect(extractBindingError(undefined)).toBeNull();
    expect(extractBindingError("a string")).toBeNull();
    expect(extractBindingError({ unrelated: true })).toBeNull();
    expect(extractBindingError({ rrule: [] })).toBeNull();
  });
});

describe("partitionInstallResults", () => {
  it("splits succeeded and failed ids positionally", () => {
    const targetIds = ["p1", "p2", "p3"];
    const results = [fulfilled(), rejected({ error: "boom" }), fulfilled()];

    const { succeededIds, failedIds } = partitionInstallResults(targetIds, results);

    expect(succeededIds).toEqual(["p1", "p3"]);
    expect(failedIds).toEqual(["p2"]);
  });

  it("captures the first surfaced backend error across failures", () => {
    const targetIds = ["p1", "p2", "p3"];
    const results = [rejected({}), rejected({ rrule: ["Invalid RRULE."] }), rejected({ error: "Later." })];

    const { firstError } = partitionInstallResults(targetIds, results);

    // p1 yields no detail (null), so the first non-null detail is p2's.
    expect(firstError).toBe("Invalid RRULE.");
  });

  it("reports null firstError when everything succeeds", () => {
    const { succeededIds, failedIds, firstError } = partitionInstallResults(["p1", "p2"], [fulfilled(), fulfilled()]);

    expect(succeededIds).toEqual(["p1", "p2"]);
    expect(failedIds).toEqual([]);
    expect(firstError).toBeNull();
  });

  it("reports null firstError when failures carry no readable detail", () => {
    const { failedIds, firstError } = partitionInstallResults(["p1"], [rejected("opaque")]);

    expect(failedIds).toEqual(["p1"]);
    expect(firstError).toBeNull();
  });
});

describe("filterProjects", () => {
  const projects = [
    { name: "Mobile App", identifier: "MOB" },
    { name: "Website", identifier: "WEB" },
    { name: "Internal Tools", identifier: null },
  ];

  it("returns a copy of the full list for an empty or whitespace query", () => {
    expect(filterProjects(projects, "")).toEqual(projects);
    expect(filterProjects(projects, "   ")).toEqual(projects);
    expect(filterProjects(projects, "")).not.toBe(projects);
  });

  it("matches on project name, case-insensitively", () => {
    expect(filterProjects(projects, "mobile").map((p) => p.identifier)).toEqual(["MOB"]);
    expect(filterProjects(projects, "WEBSITE").map((p) => p.identifier)).toEqual(["WEB"]);
  });

  it("matches on identifier", () => {
    expect(filterProjects(projects, "web").map((p) => p.name)).toEqual(["Website"]);
  });

  it("tolerates a null identifier without throwing", () => {
    expect(filterProjects(projects, "internal").map((p) => p.name)).toEqual(["Internal Tools"]);
  });

  it("returns an empty list when nothing matches", () => {
    expect(filterProjects(projects, "zzz")).toEqual([]);
  });
});

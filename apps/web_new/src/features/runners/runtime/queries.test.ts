// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, getProject: vi.fn() };
});

import { getProject, type ProjectDetail } from "@pidash/api-client";

import { isManagedRunnerAvailable, projectAvailabilityQueryOptions, runtimeKeys } from "./queries.js";

describe("runtimeKeys", () => {
  it("scopes availability under the workspace", () => {
    expect(runtimeKeys.projectAvailability("acme", "p1")).toEqual([
      "ws",
      "acme",
      "runners",
      "runtime",
      "project-availability",
      "p1",
    ]);
  });
});

describe("projectAvailabilityQueryOptions", () => {
  it("reads one project row with no stale time", async () => {
    const row = { identifier: "ACME" } as ProjectDetail;
    vi.mocked(getProject).mockResolvedValue(row);
    const options = projectAvailabilityQueryOptions("client" as never, "acme", "p1");
    expect(options.queryKey).toEqual(runtimeKeys.projectAvailability("acme", "p1"));
    expect(options.staleTime).toBe(0);
    if (!options.queryFn) throw new Error("missing queryFn");
    await expect(options.queryFn({} as never)).resolves.toBe(row);
    expect(getProject).toHaveBeenCalledWith("client", "acme", "p1");
  });
});

describe("isManagedRunnerAvailable", () => {
  it("is false without a project", () => {
    expect(isManagedRunnerAvailable(undefined)).toBe(false);
  });

  it("is true when the managed runner is available", () => {
    const project = {
      agent_executor_options: [
        { kind: "local_runner", available: true, reason_code: "" },
        { kind: "managed_runner", available: true, reason_code: "" },
      ],
    } as ProjectDetail;
    expect(isManagedRunnerAvailable(project)).toBe(true);
  });

  it("is false when the managed runner is missing or unavailable", () => {
    const missing = { agent_executor_options: [{ kind: "local_runner", available: true, reason_code: "" }] };
    expect(isManagedRunnerAvailable(missing as ProjectDetail)).toBe(false);
    const unavailable = {
      agent_executor_options: [{ kind: "managed_runner", available: false, reason_code: "offline" }],
    };
    expect(isManagedRunnerAvailable(unavailable as ProjectDetail)).toBe(false);
    const empty = { agent_executor_options: [] as ProjectDetail["agent_executor_options"] };
    expect(isManagedRunnerAvailable(empty as ProjectDetail)).toBe(false);
  });
});

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ApiError } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { agentReasonMessage } from "../../../shared/lib/agentReasons.js";
import { parseRunRefusal, selectOwnRunner } from "./runRefusal.js";

function conflict(body: unknown, code = "conflict") {
  return new ApiError({ status: 409, code, message: "refused", body });
}

describe("parseRunRefusal", () => {
  it("reads the code, the server sentence and the assigned runner", () => {
    expect(
      parseRunRefusal(
        conflict({ error: "Work laptop is offline", code: "assigned_runner_offline", assigned_runner: "r-1" })
      )
    ).toEqual({ code: "assigned_runner_offline", message: "Work laptop is offline", assignedRunnerId: "r-1" });
  });

  it("falls back to the shared sentence and to the error code", () => {
    expect(parseRunRefusal(conflict({ code: "assigned_runner_not_owner", error: " " }))).toEqual({
      code: "assigned_runner_not_owner",
      message: agentReasonMessage("assigned_runner_not_owner"),
      assignedRunnerId: null,
    });
    expect(parseRunRefusal(conflict(null, "assigned_runner_offline"))?.code).toBe("assigned_runner_offline");
  });

  it("ignores every other failure", () => {
    expect(parseRunRefusal(conflict({ error: "could not dispatch" }))).toBeNull();
    expect(parseRunRefusal(new Error("boom"))).toBeNull();
    expect(parseRunRefusal(undefined)).toBeNull();
  });
});

describe("selectOwnRunner", () => {
  const base = { pod: "p", revoked_at: null, provisioning: "manual" as const };
  const runners = [
    { ...base, id: "theirs", owner: "u-2", provisioning: "desktop_bundled" as const },
    { ...base, id: "mine-own", owner: "u-1" },
    { ...base, id: "mine-engine", owner: "u-1", provisioning: "desktop_bundled" as const },
    {
      ...base,
      id: "mine-revoked",
      owner: "u-1",
      provisioning: "desktop_bundled" as const,
      revoked_at: "2026-01-01T00:00:00Z",
    },
  ];

  it("prefers the caller's engine, then any runner of theirs", () => {
    expect(selectOwnRunner(runners, "u-1")?.id).toBe("mine-engine");
    expect(
      selectOwnRunner(
        runners.filter((runner) => runner.id !== "mine-engine"),
        "u-1"
      )?.id
    ).toBe("mine-own");
  });

  it("never picks a revoked or someone else's runner", () => {
    expect(
      selectOwnRunner(
        runners.filter((runner) => runner.id.startsWith("mine-r") || runner.id === "theirs"),
        "u-1"
      )
    ).toBeNull();
    expect(selectOwnRunner(runners, "u-9")).toBeNull();
    expect(selectOwnRunner(runners, undefined)).toBeNull();
    expect(selectOwnRunner(undefined, "u-1")).toBeNull();
  });
});

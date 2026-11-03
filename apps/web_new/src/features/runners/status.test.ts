// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner status presentation (NEWFRONT-540, RUN-011). Four display states,
// one tone map; the server status renders verbatim.
import { describe, expect, it } from "vitest";

import { RUNNER_STATUSES, RUNNER_STATUS_LABELS, RUNNER_STATUS_TONES, runnerDisplayStatus } from "./status.js";

describe("runner status vocabulary", () => {
  it("labels and tones every state", () => {
    expect(RUNNER_STATUSES).toEqual(["online", "busy", "offline", "revoked"]);
    for (const status of RUNNER_STATUSES) {
      expect(RUNNER_STATUS_LABELS[status]).toBe(status);
    }
    expect(RUNNER_STATUS_TONES).toEqual({
      online: "success",
      busy: "accent",
      offline: "neutral",
      revoked: "warning",
    });
    // The four states never share a tone.
    expect(new Set(Object.values(RUNNER_STATUS_TONES)).size).toBe(RUNNER_STATUSES.length);
  });
});

describe("runnerDisplayStatus", () => {
  it("reports the server status verbatim", () => {
    for (const status of RUNNER_STATUSES) {
      expect(runnerDisplayStatus({ status })).toBe(status);
    }
  });

  it("has no enrolment gate: the old surfaces render raw status (NEWFRONT-557)", () => {
    // Never-enrolled rows read as offline because the server reports
    // them offline; a live row reports its own state whatever enrolled_at
    // says, matching the oracle-pinned chat fixtures.
    expect(runnerDisplayStatus({ status: "online" })).toBe("online");
    expect(runnerDisplayStatus({ status: "busy" })).toBe("busy");
    expect(runnerDisplayStatus({ status: "offline" })).toBe("offline");
  });
});

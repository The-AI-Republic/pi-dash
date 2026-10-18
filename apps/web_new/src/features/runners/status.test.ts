// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner status presentation (NEWFRONT-540, RUN-011). Four display states,
// one tone map; a never-enrolled row reads offline.
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
  it("reports the server status once enrolled", () => {
    for (const status of RUNNER_STATUSES) {
      expect(runnerDisplayStatus({ status, enrolled_at: "2024-01-01T00:00:00Z" })).toBe(status);
    }
  });

  it("reads a never-enrolled row as offline whatever its server status", () => {
    expect(runnerDisplayStatus({ status: "online", enrolled_at: null })).toBe("offline");
    expect(runnerDisplayStatus({ status: "busy", enrolled_at: null })).toBe("offline");
  });
});

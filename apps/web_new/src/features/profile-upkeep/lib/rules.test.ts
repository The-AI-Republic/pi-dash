// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Optimistic upkeep rules (parity/specs/profile.md §11, PROF-024).
import type { UpkeepSettings } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { UPKEEP_FAILED_FALLBACK } from "./copy.js";
import {
  jobSwitchDisabled,
  showsEmptyExplanation,
  upkeepFailureMessage,
  withJobToggled,
  withMasterPaused,
} from "./rules.js";

function settings(overrides: Partial<UpkeepSettings> = {}): UpkeepSettings {
  return {
    enabled: true,
    jobs: [
      { slug: "triage", name: "Triage", description: "Sort new issues.", interval_label: "Daily", enabled: true },
      { slug: "digest", name: "Digest", description: "Summarise.", interval_label: "Weekly", enabled: false },
    ],
    ...overrides,
  };
}

describe("upkeep rules", () => {
  it("leaves an absent payload absent", () => {
    expect(withMasterPaused(undefined, true)).toBeUndefined();
    expect(withJobToggled(undefined, "triage", false)).toBeUndefined();
  });

  it("replaces only the master flag", () => {
    const before = settings();
    const paused = withMasterPaused(before, true);
    expect(paused).toEqual({ enabled: false, jobs: before.jobs });
    expect(before.enabled).toBe(true);
    expect(withMasterPaused(before, false)?.enabled).toBe(true);
  });

  it("replaces only the named job", () => {
    const before = settings();
    const next = withJobToggled(before, "triage", false);
    expect(next?.jobs[0]?.enabled).toBe(false);
    expect(next?.jobs[1]).toBe(before.jobs[1]);
    expect(before.jobs[0]?.enabled).toBe(true);
  });

  it("toggles the other way too, and leaves unknown slugs alone", () => {
    const before = settings();
    expect(withJobToggled(before, "digest", true)?.jobs[1]?.enabled).toBe(true);
    expect(withJobToggled(before, "ghost", false)?.jobs).toEqual(before.jobs);
  });

  it("disables job switches only while the master is paused", () => {
    expect(jobSwitchDisabled(settings({ enabled: true }))).toBe(false);
    expect(jobSwitchDisabled(settings({ enabled: false }))).toBe(true);
  });

  it("explains an instance with no jobs, and stays quiet before the read", () => {
    expect(showsEmptyExplanation(settings({ jobs: [] }))).toBe(true);
    expect(showsEmptyExplanation(settings())).toBe(false);
    expect(showsEmptyExplanation(undefined)).toBe(false);
  });

  it("prefers the detail, then the code, then the fallback", () => {
    expect(upkeepFailureMessage({ body: { error: "code", detail: "detail" } })).toBe("detail");
    expect(upkeepFailureMessage({ body: { error: "code" } })).toBe("code");
    expect(upkeepFailureMessage({ body: { error: "   " } })).toBe(UPKEEP_FAILED_FALLBACK);
    expect(upkeepFailureMessage(new Error("boom"))).toBe(UPKEEP_FAILED_FALLBACK);
    expect(upkeepFailureMessage(null)).toBe(UPKEEP_FAILED_FALLBACK);
  });
});

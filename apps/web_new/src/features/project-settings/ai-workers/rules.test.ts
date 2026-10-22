// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import {
  buildLostRunnerRulePayload,
  DEFAULT_LOST_RUNNER_RULE,
  LOST_RUNNER_RULE_OPTIONS,
  LOST_RUNNER_RULE_ORDER,
  lostRunnerRuleDescription,
  lostRunnerRuleFrom,
  lostRunnerRuleLabel,
} from "./rules.js";

describe("lostRunnerRuleFrom", () => {
  it("reads the stored rule and defaults everything else", () => {
    expect(DEFAULT_LOST_RUNNER_RULE).toBe("fail_to_backlog");
    expect(lostRunnerRuleFrom("fall_back")).toBe("fall_back");
    expect(lostRunnerRuleFrom("fail_to_backlog")).toBe("fail_to_backlog");
    expect(lostRunnerRuleFrom(undefined)).toBe("fail_to_backlog");
    expect(lostRunnerRuleFrom("tomorrow_rule")).toBe("fail_to_backlog");
  });
});

describe("lostRunnerRuleLabel", () => {
  it("labels both rules", () => {
    expect(lostRunnerRuleLabel("fail_to_backlog")).toContain("Backlog");
    expect(lostRunnerRuleLabel("fall_back")).toContain("pod");
  });
});

describe("lostRunnerRuleDescription", () => {
  it("explains both rules", () => {
    expect(lostRunnerRuleDescription("fail_to_backlog")).toContain("Backlog");
    expect(lostRunnerRuleDescription("fall_back")).toContain("pod");
  });
});

describe("LOST_RUNNER_RULE_OPTIONS", () => {
  it("offers the default first", () => {
    expect(LOST_RUNNER_RULE_ORDER).toEqual(["fail_to_backlog", "fall_back"]);
    for (const rule of LOST_RUNNER_RULE_ORDER) {
      expect(LOST_RUNNER_RULE_OPTIONS[rule].label.length).toBeGreaterThan(0);
      expect(LOST_RUNNER_RULE_OPTIONS[rule].description.length).toBeGreaterThan(0);
    }
  });
});

describe("buildLostRunnerRulePayload", () => {
  it("writes the setting column", () => {
    expect(buildLostRunnerRulePayload("fall_back")).toEqual({ dedicated_runner_lost_action: "fall_back" });
    expect(buildLostRunnerRulePayload("fail_to_backlog")).toEqual({
      dedicated_runner_lost_action: "fail_to_backlog",
    });
  });
});

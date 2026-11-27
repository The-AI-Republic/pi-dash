// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import {
  buildAiWorkersPayload,
  buildLostRunnerRulePayload,
  DEFAULT_ISSUE_RUN_POLICY,
  DEFAULT_LOST_RUNNER_RULE,
  ISSUE_RUN_POLICY_OPTIONS,
  ISSUE_RUN_POLICY_ORDER,
  issueRunPolicyDescription,
  issueRunPolicyFrom,
  issueRunPolicyLabel,
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

describe("issueRunPolicyFrom", () => {
  it("reads the stored policy and defaults everything else", () => {
    expect(DEFAULT_ISSUE_RUN_POLICY).toBe("creator_and_assignees");
    expect(issueRunPolicyFrom("project_members")).toBe("project_members");
    expect(issueRunPolicyFrom("creator_and_assignees")).toBe("creator_and_assignees");
    expect(issueRunPolicyFrom(undefined)).toBe("creator_and_assignees");
    expect(issueRunPolicyFrom("bogus")).toBe("creator_and_assignees");
  });
});

describe("issue run policy labels", () => {
  it("labels and explains both policies, distinct from the workflow switch", () => {
    expect(ISSUE_RUN_POLICY_ORDER).toEqual(["creator_and_assignees", "project_members"]);
    expect(issueRunPolicyLabel("creator_and_assignees")).toBe("Issue creator and assignees");
    expect(issueRunPolicyLabel("project_members")).toBe("All project members");
    expect(issueRunPolicyDescription("project_members")).toContain("Guests still cannot");
    for (const policy of ISSUE_RUN_POLICY_ORDER) {
      expect(ISSUE_RUN_POLICY_OPTIONS[policy].label.length).toBeGreaterThan(0);
      expect(ISSUE_RUN_POLICY_OPTIONS[policy].description.length).toBeGreaterThan(0);
    }
  });
});

describe("buildAiWorkersPayload", () => {
  it("writes only the fields the admin changed", () => {
    expect(buildAiWorkersPayload("fail_to_backlog", "fall_back", undefined, "creator_and_assignees")).toEqual({
      dedicated_runner_lost_action: "fall_back",
    });
    expect(buildAiWorkersPayload("fail_to_backlog", "fail_to_backlog", undefined, "project_members")).toEqual({
      settings: { issue_run_policy: "project_members" },
    });
    expect(
      buildAiWorkersPayload("fail_to_backlog", "fail_to_backlog", "creator_and_assignees", "creator_and_assignees")
    ).toEqual({});
  });
});

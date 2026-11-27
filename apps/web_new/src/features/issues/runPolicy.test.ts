// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { canRunIssue, RUN_POLICY_BLOCKED_REASON, runBlockedReason } from "./runPolicy.js";

const CREATOR = "11111111-1111-4111-8111-111111111111";
const ASSIGNEE = "22222222-2222-4222-8222-222222222222";
const OTHER = "33333333-3333-4333-8333-333333333333";

describe("canRunIssue", () => {
  it("defaults to creator-or-assignees when the policy is absent", () => {
    expect(canRunIssue({ viewerId: CREATOR, creatorId: CREATOR, assigneeIds: [ASSIGNEE] })).toBe(true);
    expect(canRunIssue({ viewerId: ASSIGNEE, creatorId: CREATOR, assigneeIds: [ASSIGNEE] })).toBe(true);
    expect(canRunIssue({ viewerId: OTHER, creatorId: CREATOR, assigneeIds: [ASSIGNEE] })).toBe(false);
  });

  it("gives a project admin no bypass in default mode", () => {
    expect(canRunIssue({ viewerId: OTHER, creatorId: CREATOR, assigneeIds: [], memberRole: 20 })).toBe(false);
  });

  it("denies a removed assignee immediately (absent from the list)", () => {
    expect(canRunIssue({ viewerId: ASSIGNEE, creatorId: CREATOR, assigneeIds: [] })).toBe(false);
  });

  it("denies when the viewer is unknown", () => {
    expect(canRunIssue({ viewerId: null, creatorId: CREATOR, assigneeIds: [ASSIGNEE] })).toBe(false);
  });

  it("admits any Member or Admin in project_members mode, never guests", () => {
    expect(canRunIssue({ policy: "project_members", viewerId: OTHER, creatorId: CREATOR, memberRole: 15 })).toBe(true);
    expect(canRunIssue({ policy: "project_members", viewerId: OTHER, creatorId: CREATOR, memberRole: 20 })).toBe(true);
    expect(canRunIssue({ policy: "project_members", viewerId: OTHER, creatorId: CREATOR, memberRole: 5 })).toBe(false);
    expect(canRunIssue({ policy: "project_members", viewerId: OTHER, creatorId: CREATOR, memberRole: null })).toBe(
      false
    );
  });

  it("reads unknown policies as the default", () => {
    expect(canRunIssue({ policy: "bogus", viewerId: CREATOR, creatorId: CREATOR })).toBe(true);
    expect(canRunIssue({ policy: "bogus", viewerId: OTHER, creatorId: CREATOR })).toBe(false);
  });
});

describe("runBlockedReason", () => {
  it("carries the server reason when refused, none when allowed", () => {
    expect(runBlockedReason({ viewerId: CREATOR, creatorId: CREATOR })).toBeUndefined();
    expect(runBlockedReason({ viewerId: OTHER, creatorId: CREATOR })).toBe(RUN_POLICY_BLOCKED_REASON);
  });
});

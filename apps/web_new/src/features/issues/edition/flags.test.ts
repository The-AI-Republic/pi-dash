// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition flags for the issues area (ISS-231–237): a capability is on only
// when its named flag is exactly true, and the community build carries none.
import { describe, expect, it } from "vitest";

import { edition, ISSUE_DEDUPE_FLAG as CORE_DEDUPE_FLAG } from "../../../core/edition/index.js";
import {
  DETAIL_BADGES_FLAG,
  EPICS_FLAG,
  GANTT_DEPENDENCIES_FLAG,
  ISSUE_DEDUPE_FLAG,
  ISSUE_TYPE_FILTER_FLAG,
  ISSUE_TYPES_FLAG,
  isEditionFlagOn,
  isIssueDedupeEnabled,
  TEAM_PROJECT_FILTER_FLAG,
  WORKFLOW_FLAG,
} from "./flags.js";

describe("edition flags", () => {
  it("keeps every capability off in the community build", () => {
    expect(edition.flags).toEqual({});
    for (const flag of [
      EPICS_FLAG,
      WORKFLOW_FLAG,
      TEAM_PROJECT_FILTER_FLAG,
      ISSUE_TYPE_FILTER_FLAG,
      ISSUE_TYPES_FLAG,
      DETAIL_BADGES_FLAG,
      GANTT_DEPENDENCIES_FLAG,
      ISSUE_DEDUPE_FLAG,
    ]) {
      expect(isEditionFlagOn(flag)).toBe(false);
    }
    expect(isIssueDedupeEnabled()).toBe(false);
  });

  it("names each capability once", () => {
    const names = [
      EPICS_FLAG,
      WORKFLOW_FLAG,
      TEAM_PROJECT_FILTER_FLAG,
      ISSUE_TYPE_FILTER_FLAG,
      ISSUE_TYPES_FLAG,
      DETAIL_BADGES_FLAG,
      GANTT_DEPENDENCIES_FLAG,
      ISSUE_DEDUPE_FLAG,
    ];
    expect(new Set(names).size).toBe(names.length);
  });

  it("shares the create dialog's de-dupe flag rather than forking it", () => {
    expect(ISSUE_DEDUPE_FLAG).toBe(CORE_DEDUPE_FLAG);
  });

  it("turns a capability on only for an explicit true", () => {
    expect(isEditionFlagOn(EPICS_FLAG, { [EPICS_FLAG]: true })).toBe(true);
    expect(isEditionFlagOn(EPICS_FLAG, { [EPICS_FLAG]: false })).toBe(false);
    expect(isIssueDedupeEnabled({ [ISSUE_DEDUPE_FLAG]: true })).toBe(true);
    expect(isIssueDedupeEnabled({ [ISSUE_DEDUPE_FLAG]: false })).toBe(false);
  });

  it("reads the live edition record by default", () => {
    edition.flags[EPICS_FLAG] = true;
    try {
      expect(isEditionFlagOn(EPICS_FLAG)).toBe(true);
      edition.flags[EPICS_FLAG] = false;
      expect(isEditionFlagOn(EPICS_FLAG)).toBe(false);
    } finally {
      delete edition.flags[EPICS_FLAG];
    }
    expect(isEditionFlagOn(EPICS_FLAG)).toBe(false);
  });
});

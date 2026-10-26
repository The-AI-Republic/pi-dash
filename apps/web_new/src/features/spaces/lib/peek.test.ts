// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { SpaceBoardSearch } from "./address.js";
import {
  formatSpacePropertyDate,
  isSpaceDueOverdue,
  isSpaceIssueId,
  spacePeekCloseHref,
  spacePeekOpenHref,
} from "./peek.js";

const ID_A = "11111111-1111-4111-8111-111111111111";
const ID_B = "22222222-2222-4222-8222-222222222222";

describe("isSpaceIssueId", () => {
  it("accepts only a uuid and rejects everything else", () => {
    expect(isSpaceIssueId(ID_A)).toBe(true);
    expect(isSpaceIssueId(ID_A.toUpperCase())).toBe(true);
    expect(isSpaceIssueId(undefined)).toBe(false);
    expect(isSpaceIssueId("")).toBe(false);
    expect(isSpaceIssueId("SPC-42")).toBe(false);
    expect(isSpaceIssueId("../../../etc/passwd")).toBe(false);
  });
});

describe("space property date", () => {
  it("formats a stored date and rejects a missing or unreadable one", () => {
    expect(formatSpacePropertyDate("2020-01-15")).toBe("Jan 15, 2020");
    expect(formatSpacePropertyDate("2030-06-30")).toBe("Jun 30, 2030");
    expect(formatSpacePropertyDate(null)).toBeNull();
    expect(formatSpacePropertyDate("not-a-date")).toBeNull();
  });
});

describe("isSpaceDueOverdue", () => {
  const now = new Date("2026-10-25T12:00:00Z");

  it("marks a past or today date overdue on an open issue", () => {
    expect(isSpaceDueOverdue("2020-01-15", "unstarted", now)).toBe(true);
    expect(isSpaceDueOverdue("2026-10-25", "started", now)).toBe(true);
    expect(isSpaceDueOverdue("2026-10-26", "started", now)).toBe(false);
  });

  it("never marks a completed or cancelled issue overdue", () => {
    expect(isSpaceDueOverdue("2020-01-15", "completed", now)).toBe(false);
    expect(isSpaceDueOverdue("2020-01-15", "cancelled", now)).toBe(false);
  });

  it("stands down without a date, a state group, or a readable date", () => {
    expect(isSpaceDueOverdue(null, "started", now)).toBe(false);
    expect(isSpaceDueOverdue("2020-01-15", null, now)).toBe(false);
    expect(isSpaceDueOverdue("bad", "started", now)).toBe(false);
  });
});

describe("overlay addresses", () => {
  const search: SpaceBoardSearch = {
    board: "kanban",
    state: [ID_A],
    priority: ["high"],
    labels: [ID_B],
  };

  it("opens over the current board position keeping view and filters (SPC-030)", () => {
    expect(spacePeekOpenHref("anchor", search, ID_A)).toBe(
      `/spaces/issues/anchor/?board=kanban&peekId=${ID_A}&priority=high&state=${ID_A}&labels=${ID_B}`
    );
    expect(spacePeekOpenHref("anchor", {}, ID_A)).toBe(`/spaces/issues/anchor/?peekId=${ID_A}`);
  });

  it("closes by dropping only peekId, keeping view and filters (SPC-042)", () => {
    expect(spacePeekCloseHref("anchor", { ...search, peekId: ID_A })).toBe(
      `/spaces/issues/anchor/?board=kanban&priority=high&state=${ID_A}&labels=${ID_B}`
    );
    expect(spacePeekCloseHref("anchor", { peekId: ID_A })).toBe("/spaces/issues/anchor/");
  });
});

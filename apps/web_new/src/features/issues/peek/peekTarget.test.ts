// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

import { describe, expect, it } from "vitest";

import { isIssueKey, resolvePeekTarget } from "./peekTarget.js";

const ROWS = [
  { id: "issue-1", project_id: "project-1", sequence_id: 42 },
  { id: "issue-2", project_id: "project-2", sequence_id: 7 },
];

describe("isIssueKey", () => {
  it.each([
    ["WEB-42", true],
    ["A1-1", true],
    ["550e8400-e29b-41d4-a716-446655440000", false],
    ["WEB-", false],
    ["-42", false],
    ["WEB-4F", false],
    ["", false],
  ])("classifies %s as key=%s", (value, expected) => {
    expect(isIssueKey(value)).toBe(expected);
  });
});

describe("resolvePeekTarget", () => {
  const scope = { rows: ROWS, projectIdentifier: "WEB", routeProjectId: "project-1" };

  it("resolves a held row to its id and project", () => {
    expect(resolvePeekTarget("WEB-42", scope)).toEqual({
      issueId: "issue-1",
      projectId: "project-1",
      issueKey: "WEB-42",
    });
  });

  it("keeps a cross-project row's own project", () => {
    expect(resolvePeekTarget("WEB-7", scope)).toEqual({
      issueId: "issue-2",
      projectId: "project-2",
      issueKey: "WEB-7",
    });
  });

  it("reads a raw id under the route project", () => {
    expect(resolvePeekTarget("123e4567-e89b-12d3-a456-426614174000", scope)).toEqual({
      issueId: "123e4567-e89b-12d3-a456-426614174000",
      projectId: "project-1",
      issueKey: null,
    });
  });

  it("pins an id that a held row owns to that row's project", () => {
    expect(resolvePeekTarget("issue-2", scope)).toEqual({
      issueId: "issue-2",
      projectId: "project-2",
      issueKey: null,
    });
  });

  it("prefers the peeked project for a raw id", () => {
    expect(resolvePeekTarget("123e4567-e89b-12d3-a456-426614174000", { ...scope, peekProjectId: "project-2" })).toEqual(
      {
        issueId: "123e4567-e89b-12d3-a456-426614174000",
        projectId: "project-2",
        issueKey: null,
      }
    );
  });

  it("defers an unmatched key to the lite read", () => {
    expect(resolvePeekTarget("WEB-404", scope)).toBeNull();
  });

  it("rejects a blank value", () => {
    expect(resolvePeekTarget("  ", scope)).toBeNull();
  });
});

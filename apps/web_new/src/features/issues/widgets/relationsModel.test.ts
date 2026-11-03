// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { IssueRelations, RelatedIssue } from "@pidash/api-client";
import {
  HIDDEN_RELATION_TYPES,
  INVERSE_RELATION,
  RELATION_KINDS,
  RELATION_KIND_SPECS,
  isRelationKind,
  relationCount,
  relationRows,
  visibleRelationGroups,
} from "./relationsModel.js";

function row(overrides: Partial<RelatedIssue> = {}): RelatedIssue {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "Row",
    state_id: null,
    sort_order: 1,
    priority: "none",
    sequence_id: 5,
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    label_ids: [],
    assignee_ids: [],
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    relation_type: "relates_to",
    ...overrides,
  };
}

function relations(overrides: Partial<IssueRelations> = {}): IssueRelations {
  return {
    blocking: [],
    blocked_by: [],
    duplicate: [],
    relates_to: [],
    start_after: [],
    start_before: [],
    finish_after: [],
    finish_before: [],
    ...overrides,
  };
}

describe("relations model", () => {
  it("lists the four user-facing kinds in menu order", () => {
    expect(RELATION_KINDS).toEqual(["relates_to", "duplicate", "blocked_by", "blocking"]);
    expect(RELATION_KINDS.map((kind) => RELATION_KIND_SPECS[kind].label)).toEqual([
      "Relates to",
      "Duplicate of",
      "Blocked by",
      "Blocking",
    ]);
  });

  it("tones blocked as danger and blocking as warning", () => {
    expect(RELATION_KIND_SPECS.blocked_by.tone).toBe("danger");
    expect(RELATION_KIND_SPECS.blocking.tone).toBe("warning");
    expect(RELATION_KIND_SPECS.relates_to.tone).toBe("neutral");
    expect(RELATION_KIND_SPECS.duplicate.tone).toBe("neutral");
  });

  it("maps each kind to the reciprocal kind the other side shows", () => {
    expect(INVERSE_RELATION.blocked_by).toBe("blocking");
    expect(INVERSE_RELATION.blocking).toBe("blocked_by");
    expect(INVERSE_RELATION["relates_to"]).toBe("relates_to");
    expect(INVERSE_RELATION["duplicate"]).toBe("duplicate");
  });

  it("renders only non-empty kinds and keeps the fixed order", () => {
    const data = relations({ blocking: [row()], relates_to: [row({ id: "x" as string })] });
    expect(visibleRelationGroups(data).map((group) => group.spec.kind)).toEqual(["relates_to", "blocking"]);
  });

  it("counts only the four visible kinds", () => {
    const data = relations({
      blocking: [row()],
      blocked_by: [row(), row()],
      start_after: [row(), row(), row()],
    });
    expect(relationCount(data)).toBe(3);
    expect(relationCount(undefined)).toBe(0);
  });

  it("returns no rows before the read resolves", () => {
    expect(relationRows(undefined, "blocking")).toEqual([]);
    expect(relationRows(relations(), "duplicate")).toEqual([]);
    expect(visibleRelationGroups(undefined)).toEqual([]);
  });

  it("guards menu values and documents the hidden timeline kinds", () => {
    expect(isRelationKind("blocking")).toBe(true);
    expect(isRelationKind("start_after")).toBe(false);
    expect(HIDDEN_RELATION_TYPES).toContain("start_before");
  });
});

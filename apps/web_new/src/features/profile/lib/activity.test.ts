// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Activity row presentation (parity/specs/profile.md §1 and §3).
import { describe, expect, it } from "vitest";

import type { IssueActivity } from "@pidash/api-client";

import {
  activityActorName,
  activityIssueName,
  activityIssueRef,
  activitySummary,
  appendActivityPage,
} from "./activity.js";

type Row = IssueActivity;

const actor = {
  id: "u-1",
  first_name: "Opal",
  last_name: "Whelan",
  avatar: "",
  avatar_url: null,
  display_name: "Opal W.",
  is_bot: false,
};

function row(overrides: Partial<Row> = {}): Row {
  return {
    id: "a-1",
    issue: "i-1",
    project: "p-1",
    workspace: "w-1",
    actor: "u-1",
    verb: "updated",
    comment: "",
    attachments: [],
    issue_comment: null,
    old_identifier: null,
    new_identifier: null,
    epoch: null,
    source_data: null,
    created_at: "2026-10-18T10:00:00Z",
    updated_at: "2026-10-18T10:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    field: null,
    old_value: null,
    new_value: null,
    actor_detail: actor,
    issue_detail: { id: "i-1", name: "Fix the thing", sequence_id: 42 },
    project_detail: { id: "p-1", identifier: "PP", name: "Prof Proj" },
    ...overrides,
  };
}

describe("activityIssueRef", () => {
  it("builds the browse key and address from the serialised issue and project", () => {
    expect(activityIssueRef(row(), "acme")).toEqual({ key: "PP-42", href: "/acme/browse/PP-42" });
  });

  it("declines a link when either projection is missing or the identifier is blank", () => {
    expect(activityIssueRef(row({ issue_detail: null }), "acme")).toBeNull();
    expect(activityIssueRef(row({ issue_detail: undefined }), "acme")).toBeNull();
    expect(activityIssueRef(row({ project_detail: null }), "acme")).toBeNull();
    expect(activityIssueRef(row({ project_detail: { id: "p-1", identifier: "", name: "x" } }), "acme")).toBeNull();
  });

  it("reads the issue name, or null when not serialised", () => {
    expect(activityIssueName(row())).toBe("Fix the thing");
    expect(activityIssueName(row({ issue_detail: null }))).toBeNull();
  });
});

describe("activityActorName", () => {
  it("names the signed-in member You", () => {
    expect(activityActorName(row(), "u-1")).toBe("You");
  });

  it("uses the display name, then the full name, then Someone", () => {
    expect(activityActorName(row(), "u-9")).toBe("Opal W.");
    expect(activityActorName(row({ actor_detail: { ...actor, display_name: "  " } }), null)).toBe("Opal Whelan");
    expect(
      activityActorName(row({ actor_detail: { ...actor, display_name: "", first_name: "", last_name: "" } }), null)
    ).toBe("Someone");
    expect(activityActorName(row({ actor_detail: null }), "u-1")).toBe("Someone");
  });
});

describe("activitySummary", () => {
  it("paraphrases known fields", () => {
    expect(activitySummary({ field: "name", old_value: "", new_value: "New" })).toBe("added the title");
    expect(activitySummary({ field: "priority", old_value: "low", new_value: "urgent" })).toBe("changed the priority");
    expect(activitySummary({ field: "labels", old_value: "a", new_value: "" })).toBe("removed the labels");
    expect(activitySummary({ field: "description", old_value: null, new_value: null })).toBe("added the description");
  });

  it("treats archive and comment fields as their own sentences", () => {
    expect(activitySummary({ field: "archived_at", old_value: null, new_value: "restore" })).toBe(
      "restored the work item"
    );
    expect(activitySummary({ field: "archived_at", old_value: null, new_value: "archive" })).toBe(
      "archived the work item"
    );
    expect(activitySummary({ field: "comment", old_value: null, new_value: null })).toBe("commented on the work item");
  });

  it("humanises an unknown field and falls back to an update", () => {
    expect(activitySummary({ field: "target_date", old_value: "a", new_value: "b" })).toBe("changed the target date");
    expect(activitySummary({ field: "custom_field", old_value: "a", new_value: "b" })).toBe("changed custom field");
    expect(activitySummary({ field: null, old_value: "a", new_value: "b" })).toBe("updated the work item");
  });
});

describe("appendActivityPage", () => {
  it("appends unseen rows in page order and drops repeats across the boundary", () => {
    const first = [row({ id: "a-1" }), row({ id: "a-2" })];
    const second = [row({ id: "a-2" }), row({ id: "a-3" })];
    expect(appendActivityPage(first, second).map((entry) => entry.id)).toEqual(["a-1", "a-2", "a-3"]);
    expect(appendActivityPage([], second).map((entry) => entry.id)).toEqual(["a-2", "a-3"]);
  });
});

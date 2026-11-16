// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Own-history row model (parity/specs/profile.md §7, PROF-019): every
// server change shape resolves to one sentence, a reference, a workspace
// and a link — or drops the link when the item is gone.
import type { IssueActivity } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { describeOwnActivity, MISSING_ITEM_TITLE, ownActivityRow, ownActivityRows } from "./rows.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

function activity(patch: Partial<IssueActivity> = {}): IssueActivity {
  return {
    id: UUID,
    issue: UUID2,
    project: UUID,
    workspace: UUID2,
    actor: UUID,
    actor_detail: null,
    verb: "updated",
    field: "state",
    old_value: null,
    new_value: null,
    comment: "",
    attachments: [],
    issue_comment: null,
    old_identifier: null,
    new_identifier: null,
    epoch: null,
    source_data: null,
    created_at: "2026-10-17T04:00:09Z",
    updated_at: "2026-10-17T04:00:09Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    issue_detail: { id: UUID2, name: "Fix the clock", sequence_id: 7 },
    project_detail: { id: UUID, identifier: "CT", name: "Control" },
    workspace_detail: { id: UUID2, name: "Acme", slug: "acme" },
    ...patch,
  };
}

describe("describeOwnActivity", () => {
  it("excerpts comments from the stored text", () => {
    expect(
      describeOwnActivity(activity({ field: "comment", issue_comment: UUID2, new_value: "<p>Hello <b>there</b></p>" }))
    ).toBe("Commented: Hello there");
    expect(describeOwnActivity(activity({ field: "comment", new_value: "plain" }))).toBe("Commented: plain");
    expect(describeOwnActivity(activity({ field: "comment", new_value: "" }))).toBe("Commented");
    expect(describeOwnActivity(activity({ field: "comment", new_value: null }))).toBe("Commented");
  });

  it("falls back to the old value when an edit cleared the comment text", () => {
    expect(describeOwnActivity(activity({ field: "comment", new_value: "", old_value: "earlier text" }))).toBe(
      "Commented: earlier text"
    );
  });

  it("excerpts description edits", () => {
    expect(describeOwnActivity(activity({ field: "description", new_value: "  <p>A <i>new</i> body</p> " }))).toBe(
      "Edited the description: A new body"
    );
    expect(describeOwnActivity(activity({ field: "description", new_value: "" }))).toBe("Edited the description");
    expect(describeOwnActivity(activity({ field: "description", new_value: null }))).toBe("Edited the description");
  });

  it("names state moves", () => {
    expect(describeOwnActivity(activity({ field: "state", new_value: "Started" }))).toBe("Moved the state to Started");
    expect(describeOwnActivity(activity({ field: "state", new_value: "" }))).toBe("Changed the state");
  });

  it("names assignee changes in both directions", () => {
    expect(describeOwnActivity(activity({ field: "assignees", new_value: "Ada" }))).toBe("Added an assignee: Ada");
    expect(describeOwnActivity(activity({ field: "assignee", new_value: "Ada" }))).toBe("Added an assignee: Ada");
    expect(describeOwnActivity(activity({ field: "assignees", old_value: "Ada" }))).toBe("Removed an assignee: Ada");
    expect(describeOwnActivity(activity({ field: "assignee" }))).toBe("Changed the assignees");
  });

  it("names archive and restore", () => {
    expect(describeOwnActivity(activity({ field: "archived_at", new_value: "restore" }))).toBe("Restored the item");
    expect(describeOwnActivity(activity({ field: "archived_at" }))).toBe("Archived the item");
  });

  it("paraphrases any other field from its name and new value", () => {
    expect(describeOwnActivity(activity({ field: "target_date", new_value: "2026-11-01" }))).toBe(
      "Changed the target date to 2026-11-01"
    );
    expect(describeOwnActivity(activity({ field: "priority", new_value: "" }))).toBe("Cleared the priority");
    expect(describeOwnActivity(activity({ field: "estimate_point", new_value: null }))).toBe(
      "Cleared the estimate point"
    );
  });

  it("falls back to the verb when no field changed", () => {
    expect(describeOwnActivity(activity({ field: null, verb: "created" }))).toBe("Updated an item (created)");
    expect(describeOwnActivity(activity({ field: "", verb: "deleted" }))).toBe("Updated an item (deleted)");
    expect(describeOwnActivity(activity({ field: null, verb: "" }))).toBe("Changed an item");
  });
});

describe("ownActivityRow", () => {
  it("resolves the reference, title, workspace, sentence and link", () => {
    expect(ownActivityRow(activity({ new_value: "Started" }))).toEqual({
      id: UUID,
      reference: "CT-7",
      title: "Fix the clock",
      workspace: { slug: "acme", name: "Acme" },
      summary: "Moved the state to Started",
      createdAt: "2026-10-17T04:00:09Z",
      href: "/acme/browse/CT-7",
    });
  });

  it("keeps the change verb and drops the link for a deleted item", () => {
    const row = ownActivityRow(activity({ issue_detail: null, new_value: "Started" }));
    expect(row.reference).toBeNull();
    expect(row.title).toBe(MISSING_ITEM_TITLE);
    expect(row.href).toBeNull();
    expect(row.summary).toBe("Moved the state to Started");
  });

  it("drops the reference when the project or sequence is gone", () => {
    expect(ownActivityRow(activity({ project_detail: null })).reference).toBeNull();
    expect(ownActivityRow(activity({ issue_detail: null })).reference).toBeNull();
    expect(
      ownActivityRow(activity({ issue_detail: { id: UUID2, name: "x", sequence_id: null as never } })).reference
    ).toBeNull();
  });

  it("drops the workspace line and link when the workspace is gone", () => {
    const row = ownActivityRow(activity({ workspace_detail: null }));
    expect(row.workspace).toBeNull();
    expect(row.href).toBeNull();
    expect(row.reference).toBe("CT-7");
  });

  it("keeps the reference without a link when the issue is gone but the project is not", () => {
    const row = ownActivityRow(activity({ issue_detail: null }));
    expect(row.reference).toBeNull();
    expect(row.href).toBeNull();
  });
});

describe("ownActivityRows", () => {
  it("flattens pages in server order", () => {
    const rows = ownActivityRows([
      { results: [activity({ id: UUID }), activity({ id: UUID2 })] },
      { results: [activity({ id: UUID, new_value: "Done" })] },
    ]);
    expect(rows.map((row) => row.id)).toEqual([UUID, UUID2, UUID]);
    expect(rows[2]?.summary).toBe("Moved the state to Done");
  });

  it("returns nothing for no pages", () => {
    expect(ownActivityRows([])).toEqual([]);
  });
});

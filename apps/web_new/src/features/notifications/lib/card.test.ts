// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { NotificationItem } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import type { NotificationChange } from "./card.js";
import { describeNotificationChange, resolveNotificationCard, resolveNotificationCards } from "./card.js";

const ACTOR = "Ada Lovelace";

function change(overrides: Partial<NotificationChange> = {}): NotificationChange {
  return { field: "comment", verb: "created", oldValue: "", newValue: "", excerpt: "", ...overrides };
}

describe("describeNotificationChange", () => {
  it.each([
    ["mention with excerpt", true, change({ excerpt: "see this" }), "Ada Lovelace mentioned you: see this"],
    ["mention without excerpt", true, change({}), "Ada Lovelace mentioned you"],
    ["comment with excerpt", false, change({ excerpt: "nice fix" }), "Ada Lovelace commented: nice fix"],
    ["comment without excerpt", false, change({}), "Ada Lovelace commented"],
    [
      "description with excerpt",
      false,
      change({ field: "description", verb: "updated", excerpt: "new intro" }),
      "Ada Lovelace edited the description: new intro",
    ],
    [
      "description without excerpt",
      false,
      change({ field: "description", verb: "updated" }),
      "Ada Lovelace edited the description",
    ],
    [
      "state",
      false,
      change({ field: "state", verb: "updated", oldValue: "Todo", newValue: "Done" }),
      "Ada Lovelace set the state to Done",
    ],
    [
      "assignee assigned",
      false,
      change({ field: "assignee", verb: "updated", newValue: "Grace" }),
      "Ada Lovelace assigned Grace",
    ],
    [
      "assignees assigned",
      false,
      change({ field: "assignees", verb: "updated", newValue: "Grace" }),
      "Ada Lovelace assigned Grace",
    ],
    [
      "assignee unassigned",
      false,
      change({ field: "assignee", verb: "updated", oldValue: "Grace" }),
      "Ada Lovelace unassigned Grace",
    ],
    [
      "assignees unassigned",
      false,
      change({ field: "assignees", verb: "updated", oldValue: "Grace" }),
      "Ada Lovelace unassigned Grace",
    ],
    [
      "assignee swapped",
      false,
      change({ field: "assignee", verb: "updated", oldValue: "Grace", newValue: "Alan" }),
      "Ada Lovelace assigned Alan",
    ],
    [
      "assignees unchanged",
      false,
      change({ field: "assignees", verb: "updated" }),
      "Ada Lovelace changed the assignees",
    ],
    [
      "start date",
      false,
      change({ field: "start_date", verb: "updated", newValue: "2026-10-20" }),
      "Ada Lovelace changed the start date to 2026-10-20",
    ],
    [
      "start date cleared",
      false,
      change({ field: "start_date", verb: "updated", oldValue: "2026-10-20" }),
      "Ada Lovelace removed the start date",
    ],
    [
      "due date",
      false,
      change({ field: "target_date", verb: "updated", newValue: "2026-10-21" }),
      "Ada Lovelace changed the due date to 2026-10-21",
    ],
    [
      "due date cleared",
      false,
      change({ field: "target_date", verb: "updated", oldValue: "2026-10-21" }),
      "Ada Lovelace removed the due date",
    ],
    [
      "label added",
      false,
      change({ field: "labels", verb: "updated", newValue: "bug" }),
      "Ada Lovelace added the bug label",
    ],
    [
      "label removed",
      false,
      change({ field: "labels", verb: "updated", oldValue: "bug" }),
      "Ada Lovelace removed the bug label",
    ],
    ["labels changed", false, change({ field: "labels", verb: "updated" }), "Ada Lovelace changed the labels"],
    [
      "parent set",
      false,
      change({ field: "parent", verb: "updated", newValue: "ENG-12" }),
      "Ada Lovelace set the parent to ENG-12",
    ],
    ["parent cleared", false, change({ field: "parent", verb: "updated" }), "Ada Lovelace removed the parent"],
    [
      "relation linked",
      false,
      change({ field: "link", verb: "created", newValue: "ENG-13" }),
      "Ada Lovelace linked ENG-13",
    ],
    [
      "relation blocked",
      false,
      change({ field: "blocks", verb: "created", newValue: "ENG-13" }),
      "Ada Lovelace linked ENG-13",
    ],
    [
      "link address updated",
      false,
      change({
        field: "link",
        verb: "updated",
        oldValue: "https://old.test/spec",
        newValue: "https://new.test/spec",
      }),
      "Ada Lovelace linked https://new.test/spec",
    ],
    [
      "relation unlinked",
      false,
      change({ field: "link", verb: "deleted", oldValue: "ENG-13" }),
      "Ada Lovelace unlinked ENG-13",
    ],
    ["relations changed", false, change({ field: "link", verb: "updated" }), "Ada Lovelace changed the relations"],
    ["attachment added", false, change({ field: "attachment", verb: "created" }), "Ada Lovelace attached a file"],
    [
      "attachment removed",
      false,
      change({ field: "attachment", verb: "deleted" }),
      "Ada Lovelace removed an attachment",
    ],
    [
      "attachment detached",
      false,
      change({ field: "attachment", verb: "removed" }),
      "Ada Lovelace removed an attachment",
    ],
    [
      "attachments changed",
      false,
      change({ field: "attachment", verb: "updated" }),
      "Ada Lovelace changed the attachments",
    ],
    [
      "archived",
      false,
      change({ field: "archived_at", verb: "updated", newValue: "manual_archive" }),
      "Ada Lovelace archived this issue",
    ],
    [
      "restored",
      false,
      change({ field: "archived_at", verb: "updated", oldValue: "archive", newValue: "restore" }),
      "Ada Lovelace restored this issue",
    ],
    [
      "estimate points",
      false,
      change({ field: "estimate_points", verb: "updated", oldValue: "1", newValue: "3" }),
      "Ada Lovelace set the estimate to 3",
    ],
    [
      "estimate hours",
      false,
      change({ field: "estimate_hours", verb: "updated", newValue: "5" }),
      "Ada Lovelace set the estimate to 5",
    ],
    [
      "unknown kind with a value",
      false,
      change({ field: "priority", verb: "updated", oldValue: "low", newValue: "high" }),
      "Ada Lovelace updated priority: high",
    ],
    ["unknown kind without a value", false, change({ field: "vote", verb: "deleted" }), "Ada Lovelace deleted vote"],
  ])("%s", (_label, isMention, input, expected) => {
    expect(describeNotificationChange(ACTOR, input, isMention)).toBe(expected);
  });

  it("gives mentions their own sentence regardless of the underlying field", () => {
    expect(
      describeNotificationChange(
        ACTOR,
        change({ field: "state", verb: "updated", oldValue: "Todo", newValue: "Done" }),
        true
      )
    ).toBe("Ada Lovelace mentioned you");
  });
});

const ISSUE_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const WORKSPACE_ID = "323e4567-e89b-12d3-a456-426614174002";
const ACTOR_ID = "423e4567-e89b-12d3-a456-426614174003";
const ROW_ID = "523e4567-e89b-12d3-a456-426614174004";

function row(overrides: Partial<NotificationItem> = {}, data: unknown = undefined): NotificationItem {
  return {
    id: ROW_ID,
    workspace: WORKSPACE_ID,
    project: PROJECT_ID,
    data:
      data === undefined
        ? {
            issue: { id: ISSUE_ID, name: "Ship it", identifier: "ENG", sequence_id: 12 },
            issue_activity: {
              id: "623e4567-e89b-12d3-a456-426614174005",
              verb: "created",
              field: "comment",
              actor: ACTOR_ID,
              new_value: "",
              old_value: "",
              issue_comment: "looks good",
            },
          }
        : data,
    entity_identifier: ISSUE_ID,
    entity_name: "issue",
    title: "",
    message: null,
    message_html: "",
    message_stripped: null,
    sender: "in_app:issue_activities:default",
    triggered_by: ACTOR_ID,
    triggered_by_details: {
      id: ACTOR_ID,
      first_name: "Ada",
      last_name: "Lovelace",
      avatar: "",
      avatar_url: "https://cdn.test/ada.png",
      display_name: ACTOR,
      is_bot: false,
    },
    receiver: "723e4567-e89b-12d3-a456-426614174006",
    read_at: null,
    snoozed_till: null,
    archived_at: null,
    is_mentioned_notification: false,
    created_at: "2026-10-16T12:00:00Z",
    updated_at: "2026-10-16T12:00:00Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

describe("resolveNotificationCard", () => {
  it("resolves a comment row to its card", () => {
    expect(resolveNotificationCard(row())).toEqual({
      id: ROW_ID,
      issueId: ISSUE_ID,
      projectId: PROJECT_ID,
      actorName: ACTOR,
      actorAvatarUrl: "https://cdn.test/ada.png",
      summary: "Ada Lovelace commented: looks good",
      reference: "ENG-12",
      title: "Ship it",
      createdAt: "2026-10-16T12:00:00Z",
      unread: true,
      mentioned: false,
      triage: false,
      archivedAt: null,
      snoozedTill: null,
      streamFlags: {
        is_inbox_issue: undefined,
        is_intake_issue: undefined,
        is_mentioned_notification: false,
      },
    });
  });

  it("carries the archive, snooze and stream flags the actions reuse", () => {
    const card = resolveNotificationCard(
      row({
        archived_at: "2026-10-16T13:00:00Z",
        snoozed_till: "2026-10-19T17:30:00Z",
        is_inbox_issue: true,
        is_intake_issue: false,
        is_mentioned_notification: true,
      })
    );
    expect(card?.archivedAt).toBe("2026-10-16T13:00:00Z");
    expect(card?.snoozedTill).toBe("2026-10-19T17:30:00Z");
    expect(card?.streamFlags).toEqual({
      is_inbox_issue: true,
      is_intake_issue: false,
      is_mentioned_notification: true,
    });
    expect(card?.mentioned).toBe(true);
  });

  it("marks cards with a read timestamp as read", () => {
    expect(resolveNotificationCard(row({ read_at: "2026-10-16T13:00:00Z" }))?.unread).toBe(false);
  });

  it.each([
    ["flagged mention", true, "Ada Lovelace mentioned you: looks good"],
    ["plain row", false, "Ada Lovelace commented: looks good"],
    ["null flag", null, "Ada Lovelace commented: looks good"],
    ["absent flag", undefined, "Ada Lovelace commented: looks good"],
  ])("summarizes %s", (_label, flag, expected) => {
    const card = resolveNotificationCard(row({ is_mentioned_notification: flag }));
    expect(card?.summary).toBe(expected);
    expect(card?.mentioned).toBe(flag === true);
  });

  it.each([
    ["inbox row", { is_inbox_issue: true }, true],
    ["intake row", { is_intake_issue: true }, true],
    ["ordinary row", { is_inbox_issue: false, is_intake_issue: false }, false],
  ])("classifies an %s for the detail host", (_label, overrides, expected) => {
    expect(resolveNotificationCard(row(overrides))?.triage).toBe(expected);
  });

  it("strips markup from description excerpts", () => {
    const card = resolveNotificationCard(
      row(
        {},
        {
          issue: { id: ISSUE_ID, name: "Ship it", identifier: "ENG", sequence_id: 12 },
          issue_activity: {
            verb: "updated",
            field: "description",
            old_value: "<p>before</p>",
            new_value: "<h1>Shipped</h1><p>it  went   out</p>",
          },
        }
      )
    );
    expect(card?.summary).toBe("Ada Lovelace edited the description: Shipped it went out");
  });

  it("treats non-string values as absent", () => {
    const card = resolveNotificationCard(
      row(
        {},
        {
          issue: { id: ISSUE_ID, name: "Ship it", identifier: "ENG", sequence_id: 12 },
          issue_activity: { verb: "updated", field: "state", old_value: 1, new_value: 2 },
        }
      )
    );
    expect(card?.summary).toBe("Ada Lovelace set the state to ");
  });

  it.each([
    ["missing workspace", { workspace: "" }, undefined],
    ["missing project", { project: null }, undefined],
    ["missing actor", { triggered_by_details: null }, undefined],
    ["null payload", {}, null],
    ["non-object payload", {}, "oops"],
    ["missing item", {}, { issue_activity: { verb: "created", field: "comment" } }],
    ["non-object item", {}, { issue: "ENG-12", issue_activity: { verb: "created", field: "comment" } }],
    [
      "item without an id",
      {},
      {
        issue: { name: "Ship it", identifier: "ENG", sequence_id: 12 },
        issue_activity: { verb: "created", field: "comment" },
      },
    ],
    [
      "unnamed item",
      {},
      {
        issue: { id: ISSUE_ID, name: "", identifier: "ENG", sequence_id: 12 },
        issue_activity: { verb: "created", field: "comment" },
      },
    ],
    [
      "item without a reference key",
      {},
      {
        issue: { id: ISSUE_ID, name: "Ship it", identifier: "", sequence_id: 12 },
        issue_activity: { verb: "created", field: "comment" },
      },
    ],
    [
      "item without a sequence number",
      {},
      {
        issue: { id: ISSUE_ID, name: "Ship it", identifier: "ENG", sequence_id: null },
        issue_activity: { verb: "created", field: "comment" },
      },
    ],
    ["missing activity", {}, { issue: { id: ISSUE_ID, name: "Ship it", identifier: "ENG", sequence_id: 12 } }],
    [
      "non-object activity",
      {},
      {
        issue: { id: ISSUE_ID, name: "Ship it", identifier: "ENG", sequence_id: 12 },
        issue_activity: "commented",
      },
    ],
    [
      "activity without a field",
      {},
      {
        issue: { id: ISSUE_ID, name: "Ship it", identifier: "ENG", sequence_id: 12 },
        issue_activity: { verb: "created", field: "" },
      },
    ],
    [
      "activity without a verb",
      {},
      {
        issue: { id: ISSUE_ID, name: "Ship it", identifier: "ENG", sequence_id: 12 },
        issue_activity: { verb: "", field: "comment" },
      },
    ],
  ])("skips a row with %s", (_label, overrides, data) => {
    expect(resolveNotificationCard(row(overrides, data))).toBeNull();
  });
});

describe("resolveNotificationCards", () => {
  it("keeps server order and drops unresolvable rows", () => {
    const first = row({ id: "823e4567-e89b-12d3-a456-426614174007" });
    const broken = row({ project: null });
    const second = row({ id: "923e4567-e89b-12d3-a456-426614174008" });
    const cards = resolveNotificationCards([first, broken, second]);
    expect(cards.map((card) => card.id)).toEqual([
      "823e4567-e89b-12d3-a456-426614174007",
      "923e4567-e89b-12d3-a456-426614174008",
    ]);
  });

  it("resolves an empty page to no cards", () => {
    expect(resolveNotificationCards([])).toEqual([]);
  });
});

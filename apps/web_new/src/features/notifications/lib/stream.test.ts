// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { NotificationItem } from "@pidash/api-client";

import { selectUnreadBadgeCount } from "./badgeCount.js";
import {
  DEFAULT_STREAM_MODES,
  isHiddenByStreamMode,
  isModeEnabled,
  markAllReadScope,
  notificationStreamQuery,
  NOTIFICATION_MODES,
  NOTIFICATION_MODE_LABELS,
  NOTIFICATION_ORIGINS,
  NOTIFICATION_ORIGIN_LABELS,
  NOTIFICATION_TABS,
  notificationTabQuery,
  removeOrigin,
  toggleMode,
  toggleOrigin,
  type StreamModes,
} from "./stream.js";

describe("notification tabs", () => {
  it("offers exactly the full stream and the mentions stream, in order", () => {
    expect([...NOTIFICATION_TABS]).toEqual(["all", "mentions"]);
  });

  it("queries the default stream with explicit exclusion flags", () => {
    expect(notificationTabQuery("all")).toEqual({ snoozed: false, archived: false });
  });

  it("queries the mentions stream with the mentioned-only flag", () => {
    expect(notificationTabQuery("mentions")).toEqual({ snoozed: false, archived: false, mentioned: true });
  });
});

describe("unread badge selection (NTF-003)", () => {
  it("prefers the mention total while any mention is unread", () => {
    expect(selectUnreadBadgeCount({ total_unread_notifications_count: 7, mention_unread_notifications_count: 2 })).toBe(
      2
    );
  });

  it("falls back to the overall total when no mention is unread", () => {
    expect(selectUnreadBadgeCount({ total_unread_notifications_count: 7, mention_unread_notifications_count: 0 })).toBe(
      7
    );
  });

  it("reads zero at zero and while the totals are unknown", () => {
    expect(selectUnreadBadgeCount({ total_unread_notifications_count: 0, mention_unread_notifications_count: 0 })).toBe(
      0
    );
    expect(selectUnreadBadgeCount(undefined)).toBe(0);
  });
});

describe("origin filter (NTF-015)", () => {
  it("offers the three origins in the fixed join order", () => {
    expect([...NOTIFICATION_ORIGINS]).toEqual(["assigned", "created", "subscribed"]);
    expect(Object.keys(NOTIFICATION_ORIGIN_LABELS)).toEqual(["assigned", "created", "subscribed"]);
    for (const origin of NOTIFICATION_ORIGINS) {
      expect(NOTIFICATION_ORIGIN_LABELS[origin].length).toBeGreaterThan(0);
    }
  });

  it("joins active origins into the type parameter in canonical order", () => {
    expect(toggleOrigin([], "subscribed")).toEqual(["subscribed"]);
    expect(toggleOrigin(["subscribed"], "assigned")).toEqual(["assigned", "subscribed"]);
    expect(toggleOrigin(["subscribed", "assigned"], "created")).toEqual(["assigned", "created", "subscribed"]);
  });

  it("toggles an active origin back off and returns a fresh array", () => {
    const before = ["assigned", "created"] as const;
    const after = toggleOrigin(before, "assigned");
    expect(after).toEqual(["created"]);
    expect(after).not.toBe(before);
    expect(before).toEqual(["assigned", "created"]);
  });

  it("removes a chip's origin and keeps the rest in order", () => {
    expect(removeOrigin(["assigned", "created", "subscribed"], "created")).toEqual(["assigned", "subscribed"]);
    expect(removeOrigin(["assigned"], "subscribed")).toEqual(["assigned"]);
  });
});

describe("stream modes (NTF-016)", () => {
  it("lists the three overflow modes with wording, unread first", () => {
    expect([...NOTIFICATION_MODES]).toEqual(["unread", "archived", "snoozed"]);
    expect(NOTIFICATION_MODE_LABELS.unread).toMatch(/unread/i);
    expect(NOTIFICATION_MODE_LABELS.archived).toMatch(/archiv/i);
    expect(NOTIFICATION_MODE_LABELS.snoozed).toMatch(/snooz/i);
  });

  it("starts with every mode off", () => {
    expect(DEFAULT_STREAM_MODES).toEqual({ unreadOnly: false, archived: false, snoozed: false });
    for (const mode of NOTIFICATION_MODES) {
      expect(isModeEnabled(DEFAULT_STREAM_MODES, mode)).toBe(false);
    }
  });

  it("flips unread-only without touching the exclusions", () => {
    const next = toggleMode({ unreadOnly: false, archived: true, snoozed: false }, "unread");
    expect(next).toEqual({ unreadOnly: true, archived: true, snoozed: false });
    expect(toggleMode(next, "unread")).toEqual({ unreadOnly: false, archived: true, snoozed: false });
  });

  it("turns snoozed off when archived turns on, and the reverse", () => {
    expect(toggleMode({ unreadOnly: false, archived: false, snoozed: true }, "archived")).toEqual({
      unreadOnly: false,
      archived: true,
      snoozed: false,
    });
    expect(toggleMode({ unreadOnly: false, archived: true, snoozed: false }, "snoozed")).toEqual({
      unreadOnly: false,
      archived: false,
      snoozed: true,
    });
  });

  it("turns a mode off without enabling the other", () => {
    expect(toggleMode({ unreadOnly: false, archived: true, snoozed: false }, "archived")).toEqual(DEFAULT_STREAM_MODES);
    expect(toggleMode({ unreadOnly: true, archived: false, snoozed: true }, "snoozed")).toEqual({
      unreadOnly: true,
      archived: false,
      snoozed: false,
    });
  });

  it("never enables both exclusions, whatever order the user picks", () => {
    const sequences: ("archived" | "snoozed")[][] = [
      ["archived", "snoozed"],
      ["snoozed", "archived"],
    ];
    for (const sequence of sequences) {
      const modes = sequence.reduce<StreamModes>((current, mode) => toggleMode(current, mode), DEFAULT_STREAM_MODES);
      expect(modes.archived && modes.snoozed).toBe(false);
    }
  });
});

describe("stream query (NTF-015-017)", () => {
  it("sends the default stream's explicit exclusion flags and nothing else", () => {
    expect(notificationStreamQuery("all", [], DEFAULT_STREAM_MODES)).toEqual({ snoozed: false, archived: false });
  });

  it("adds the origin type and the read flag as the controls narrow the stream", () => {
    expect(notificationStreamQuery("all", ["assigned", "created"], DEFAULT_STREAM_MODES)).toEqual({
      snoozed: false,
      archived: false,
      type: "assigned,created",
    });
    expect(notificationStreamQuery("all", [], { unreadOnly: true, archived: false, snoozed: false })).toEqual({
      snoozed: false,
      archived: false,
      read: false,
    });
  });

  it("sends archived true with snoozed false, and the mirror", () => {
    expect(notificationStreamQuery("all", [], { unreadOnly: false, archived: true, snoozed: false })).toEqual({
      snoozed: false,
      archived: true,
    });
    expect(notificationStreamQuery("all", [], { unreadOnly: false, archived: false, snoozed: true })).toEqual({
      snoozed: true,
      archived: false,
    });
  });

  it("keeps the mentioned flag on top of filters and modes", () => {
    expect(
      notificationStreamQuery("mentions", ["subscribed"], { unreadOnly: true, archived: true, snoozed: false })
    ).toEqual({ snoozed: false, archived: true, type: "subscribed", read: false, mentioned: true });
  });
});

describe("mark-all-read scope (NTF-014)", () => {
  it("posts the current origin and exclusion flags", () => {
    expect(markAllReadScope([], DEFAULT_STREAM_MODES)).toEqual({ snoozed: false, archived: false });
    expect(markAllReadScope(["assigned", "subscribed"], { unreadOnly: true, archived: false, snoozed: true })).toEqual({
      type: "assigned,subscribed",
      snoozed: true,
      archived: false,
    });
  });

  it("never carries the read flag", () => {
    expect("read" in markAllReadScope([], { unreadOnly: true, archived: false, snoozed: false })).toBe(false);
  });
});

function row(overrides: Partial<NotificationItem> = {}): NotificationItem {
  return {
    id: "523e4567-e89b-12d3-a456-426614174004",
    workspace: "323e4567-e89b-12d3-a456-426614174002",
    project: null,
    data: null,
    entity_identifier: null,
    entity_name: "issue",
    title: "",
    message: null,
    message_html: "",
    message_stripped: null,
    sender: "in_app:issue_activities:default",
    triggered_by: null,
    triggered_by_details: null,
    receiver: "723e4567-e89b-12d3-a456-426614174006",
    read_at: null,
    snoozed_till: null,
    archived_at: null,
    created_at: "2026-10-16T12:00:00Z",
    updated_at: "2026-10-16T12:00:00Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

describe("stream hiding (NTF-017)", () => {
  const now = Date.parse("2026-10-17T12:00:00Z");

  it("hides a row snoozed into the future from the default stream", () => {
    expect(isHiddenByStreamMode(row({ snoozed_till: "2026-10-20T12:00:00Z" }), DEFAULT_STREAM_MODES, now)).toBe(true);
  });

  it("shows unsnoozed rows and rows whose snooze already elapsed", () => {
    expect(isHiddenByStreamMode(row(), DEFAULT_STREAM_MODES, now)).toBe(false);
    expect(isHiddenByStreamMode(row({ snoozed_till: "2026-10-17T11:00:00Z" }), DEFAULT_STREAM_MODES, now)).toBe(false);
  });

  it("shows a row that just became due", () => {
    expect(isHiddenByStreamMode(row({ snoozed_till: "2026-10-17T12:00:00Z" }), DEFAULT_STREAM_MODES, now)).toBe(false);
  });

  it("leaves the archived and snoozed modes' rows as the server sent them", () => {
    const snoozed = row({ snoozed_till: "2026-10-20T12:00:00Z" });
    expect(isHiddenByStreamMode(snoozed, { unreadOnly: false, archived: true, snoozed: false }, now)).toBe(false);
    expect(isHiddenByStreamMode(snoozed, { unreadOnly: false, archived: false, snoozed: true }, now)).toBe(false);
  });

  it("still hides future snoozes in unread-only mode", () => {
    expect(
      isHiddenByStreamMode(
        row({ snoozed_till: "2026-10-20T12:00:00Z" }),
        { unreadOnly: true, archived: false, snoozed: false },
        now
      )
    ).toBe(true);
  });
});
